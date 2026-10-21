//! `cocoa mcp`: cocoa's commands as Model Context Protocol tools, served
//! over stdio with `legume-mcp`, so an agent can simulate data, collapse
//! pseudobulks, run the confounder-adjusted differential analysis and read
//! its tables back.
//!
//! A run is set up as cocoa parses it (the command line built from the
//! agent's flags and checked by cocoa's own parser before anything runs),
//! never over an existing run, and run as a child `cocoa`: stdout carries
//! the protocol, and its log lines come back as progress. Outputs the agent
//! does not name go to today's results folder. `legume_mcp::project` keeps
//! the project's records after Noble (PLoS Comput Biol 5(7): e1000424,
//! 2009, doi:10.1371/journal.pcbi.1000424): every run goes into the day's
//! `runall.sh`, so the work can be redone without the agent.

use anyhow::Context;
use clap::CommandFactory;
use legume_mcp::{Call, Content, Handler, ServerInfo, ToolSpec};
use legume_numeric::matrix::table::TableReader;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str = "\
cocoa estimates exposure effects on single-cell gene expression across \
individuals, adjusted for confounders (counterfactual confounder adjustment, \
Park & Kellis 2021). `cocoa_commands` lists the commands (diff, collapse, \
simulate-one, simulate-collider) and, with one, its flags. `cocoa_run` runs \
one: data files go in `data`, flags by their long name in `flags`; the \
command line is checked as cocoa parses it before anything runs, and an \
existing run is never written over. A diff on a large data set takes a while: \
set `background` and follow the job with `job_status` / `job_wait`. \
`cocoa_read` lists a run's tables (effect, delta, contrast, stage1, perm, \
...) and, with a table, returns its top rows sorted by a column (p-values \
ascending, other columns by absolute value), or the rows of given genes. \
`docs` prints the method write-ups: what the estimator identifies and \
assumes. Relative paths are read from the project folder; results \
give the paths written.";

/// The commands an agent may run, in the order `cocoa --help` lists them.
const COMMANDS: &[&str] = &["diff", "collapse", "simulate-one", "simulate-collider"];

/// The method write-ups: topic, text.
const DOCS: &[(&str, &str)] = &[
    ("readme", include_str!("../README.md")),
    ("identification", include_str!("../docs/identification.md")),
    (
        "changes_since_cocoa_diff",
        include_str!("../docs/changes_since_cocoa_diff.md"),
    ),
];

/// Rows `cocoa_read` returns unless asked for another number.
const TOP: usize = 20;

pub fn serve() -> anyhow::Result<()> {
    // The project's folder, so the paths an agent gives are read from the
    // project, as the records are.
    let root = std::env::var_os(legume_mcp::project::ROOT_ENV).filter(|r| !r.is_empty());
    if let Some(root) = root {
        std::env::set_current_dir(&root)
            .with_context(|| format!("project {}", Path::new(&root).display()))?;
    }
    let info = ServerInfo {
        name: "cocoa".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        instructions: Some(INSTRUCTIONS.into()),
    };
    legume_mcp::serve(info, Cocoa)
}

struct Cocoa;

impl Handler for Cocoa {
    fn tools(&self) -> Vec<ToolSpec> {
        tools()
    }

    fn call(&mut self, name: &str, args: Value, cx: &Call) -> anyhow::Result<Vec<Content>> {
        match name {
            "cocoa_commands" => commands(parse(args)?),
            "cocoa_run" => run(parse(args)?, cx),
            "cocoa_read" => read(parse(args)?),
            "docs" => docs(parse(args)?),
            _ => unreachable!("the server answers unknown tools itself"),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(args: Value) -> anyhow::Result<T> {
    serde_json::from_value(args).map_err(|e| anyhow::anyhow!("arguments: {e}"))
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn tools() -> Vec<ToolSpec> {
    let command = json!({ "type": "string", "enum": COMMANDS });
    vec![
        ToolSpec::new(
            "cocoa_commands",
            "cocoa's commands, each with what it does; with `command`, its flags: help, default, choices, whether required, whether a switch, and the positional data files.",
            schema(json!({ "command": command.clone() }), &[]),
        ),
        ToolSpec::new(
            "cocoa_run",
            "Run a cocoa command as a child `cocoa`, checked as cocoa parses it first, never over an existing run. Returns the output prefix and the files it wrote (or, with `background`, a job id at once).",
            schema(
                json!({
                    "command": command,
                    "data": { "type": "array", "items": { "type": "string" }, "description": "Single-cell data files (.zarr / .h5), for diff and collapse" },
                    "flags": { "type": "object", "description": "Flags by long name (without --): a value, a list (joined as the flag takes it), or true/false for a switch. cocoa_commands lists them" },
                    "out": { "type": "string", "description": "Output prefix; the next free {command} in today's results folder when omitted" },
                    "background": { "type": "boolean", "description": "Run as a background job: the call returns its id at once; follow it with job_wait or job_status, and read the tables only once it is done" },
                }),
                &["command"],
            ),
        ),
        ToolSpec::new(
            "cocoa_read",
            "A run's tables: without `table`, each table's columns and rows; with one, its top rows sorted by `sort` (default: the first p-value column, else the first contrast), or the rows of `genes`.",
            schema(
                json!({
                    "prefix": { "type": "string", "description": "The run's output prefix, or one of its .parquet files" },
                    "table": { "type": "string", "description": "effect, delta, contrast, stage1, propensity, perm, or a collapse run's topic" },
                    "sort": { "type": "string", "description": "Column to sort by" },
                    "order": { "type": "string", "enum": ["asc", "desc", "abs"], "description": "asc for p-values and q-values, abs (largest magnitude first) otherwise" },
                    "top": { "type": "integer", "description": "Rows returned (20)" },
                    "genes": { "type": "array", "items": { "type": "string" }, "description": "Only these rows, by their first column" },
                }),
                &["prefix"],
            ),
        ),
        ToolSpec::new(
            "docs",
            "cocoa's method write-ups; without a topic, the list of topics.",
            schema(
                json!({ "topic": { "type": "string", "enum": DOCS.iter().map(|(t, _)| *t).collect::<Vec<_>>() } }),
                &[],
            ),
        ),
    ]
}

/// cocoa's command line, built, so each command carries the global flags.
fn cli() -> clap::Command {
    let mut cli = crate::Cli::command();
    cli.build();
    cli
}

/// Command `name` of the built command line `cli`.
fn subcommand<'a>(cli: &'a clap::Command, name: &str) -> anyhow::Result<&'a clap::Command> {
    anyhow::ensure!(
        COMMANDS.contains(&name),
        "command {name:?}: one of {}",
        COMMANDS.join(", ")
    );
    Ok(cli
        .find_subcommand(name)
        .expect("every command is a subcommand"))
}

/// The flags an agent sets: all but help.
fn flags(sub: &clap::Command) -> impl Iterator<Item = &clap::Arg> {
    sub.get_arguments().filter(|a| a.get_id() != "help")
}

/// A flag as the command line spells it: `--long`, else `-s`; a
/// positional by its name.
fn flag_name(arg: &clap::Arg) -> String {
    match (arg.get_long(), arg.get_short()) {
        (Some(long), _) => format!("--{long}"),
        (None, Some(short)) => format!("-{short}"),
        (None, None) => arg.get_id().as_str().replace('_', "-"),
    }
}

fn is_switch(arg: &clap::Arg) -> bool {
    !arg.get_action().takes_values()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandsArgs {
    command: Option<String>,
}

fn commands(a: CommandsArgs) -> anyhow::Result<Vec<Content>> {
    let cli = cli();
    let about = |sub: &clap::Command| sub.get_about().map(|s| s.to_string());
    let Some(name) = a.command else {
        let commands = COMMANDS
            .iter()
            .map(|&c| Ok(json!({ "command": c, "about": about(subcommand(&cli, c)?) })))
            .collect::<anyhow::Result<Vec<_>>>()?;
        return Ok(vec![Content::Json(json!({ "commands": commands }))]);
    };
    let sub = subcommand(&cli, &name)?;
    let flags: Vec<Value> = flags(sub)
        .map(|arg| {
            let mut flag = json!({
                "flag": flag_name(arg),
                "help": arg.get_help().map(|h| h.to_string()),
            });
            if is_switch(arg) {
                flag["switch"] = true.into();
            } else {
                let defaults: Vec<String> = arg
                    .get_default_values()
                    .iter()
                    .map(|v| v.to_string_lossy().into_owned())
                    .collect();
                if !defaults.is_empty() {
                    flag["default"] = defaults.join(",").into();
                }
                let choices: Vec<String> = arg
                    .get_possible_values()
                    .iter()
                    .filter(|v| !v.is_hide_set())
                    .map(|v| v.get_name().to_string())
                    .collect();
                if !choices.is_empty() {
                    flag["choices"] = choices.into();
                }
            }
            if arg.is_positional() {
                flag["positional"] = true.into();
            }
            if arg.is_required_set() {
                flag["required"] = true.into();
            }
            flag
        })
        .collect();
    Ok(vec![Content::Json(
        json!({ "command": name, "about": about(sub), "flags": flags }),
    )])
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    command: String,
    #[serde(default)]
    data: Vec<String>,
    #[serde(default)]
    flags: Map<String, Value>,
    out: Option<String>,
    #[serde(default)]
    background: bool,
}

/// A flag's value as it goes on the command line.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

/// The command line for `command` from the agent's data files and flags,
/// without `--output`: each flag as one `--flag=value` word, so a value
/// starting with `-` stays a value.
fn command_line(sub: &clap::Command, a: &RunArgs) -> anyhow::Result<Vec<String>> {
    let mut argv = vec![a.command.clone()];
    if !a.data.is_empty() {
        anyhow::ensure!(
            sub.get_positionals().next().is_some(),
            "{} takes no data files",
            a.command
        );
        argv.extend(a.data.iter().cloned());
    }
    for (key, value) in &a.flags {
        let key = key.trim_start_matches('-');
        let arg = flags(sub)
            .find(|f| {
                !f.is_positional()
                    && (f.get_long() == Some(key)
                        || f.get_id().as_str() == key.replace('-', "_")
                        || f.get_short().is_some_and(|s| key == s.to_string()))
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} has no flag --{key}: cocoa_commands lists them",
                    a.command
                )
            })?;
        anyhow::ensure!(
            arg.get_id() != "output",
            "give the output prefix as out, not as a flag"
        );
        let flag = flag_name(arg);
        if is_switch(arg) {
            match value {
                Value::Bool(true) => argv.push(flag),
                Value::Bool(false) | Value::Null => {}
                v => anyhow::bail!("{flag} is a switch: true or false, not {v}"),
            }
            continue;
        }
        let values: Vec<String> = match value {
            Value::Null => continue,
            Value::Array(vs) => vs.iter().map(text).collect(),
            v => vec![text(v)],
        };
        if let Some(delim) = arg.get_value_delimiter() {
            argv.push(format!("{flag}={}", values.join(&delim.to_string())));
        } else {
            argv.extend(values.iter().map(|v| format!("{flag}={v}")));
        }
    }
    Ok(argv)
}

/// The files a run of `argv` with output prefix `out` writes, as the
/// command lists them, once cocoa's parser has accepted the command line.
fn plan(cli: &clap::Command, argv: &[String], out: &str) -> anyhow::Result<Vec<PathBuf>> {
    use crate::{run_collapse::*, run_diff::*, run_sim_collider::*, run_sim_one_type::*};
    use clap::FromArgMatches;
    let words = std::iter::once("cocoa".to_string())
        .chain(argv.iter().cloned())
        .chain([format!("--output={out}")]);
    let matches = cli
        .clone()
        .try_get_matches_from(words)
        .map_err(|e| anyhow::anyhow!("{}", e.render()))?;
    let (command, m) = matches.subcommand().expect("the command was parsed");
    let files = match command {
        "diff" => DiffArgs::from_arg_matches(m)?.output_files(),
        "collapse" => CollapseArgs::from_arg_matches(m)?.output_files(),
        "simulate-one" => SimOneTypeArgs::from_arg_matches(m)?.output_files()?,
        "simulate-collider" => SimColliderArgs::from_arg_matches(m)?.output_files()?,
        _ => unreachable!("one of COMMANDS"),
    };
    Ok(files.into_iter().map(PathBuf::from).collect())
}

fn run(a: RunArgs, cx: &Call) -> anyhow::Result<Vec<Content>> {
    let cli = cli();
    let argv = command_line(subcommand(&cli, &a.command)?, &a)?;
    let (out, outputs) = match &a.out {
        Some(out) => {
            let outputs = plan(&cli, &argv, out)?;
            if let Some(found) = taken(Path::new(out), &outputs) {
                anyhow::bail!(
                    "{} exists: cocoa_run never writes over a run; choose another out",
                    found.display()
                );
            }
            (out.clone(), outputs)
        }
        None => free_prefix(&cli, &a.command, &argv, cx)?,
    };

    let mut cmd = legume_mcp::own_command(argv.iter().chain([&format!("--output={out}")]))?;
    cmd.env("RUST_LOG", "info");
    let summary = format!("cocoa {} to {out}", a.command);
    let shown: Vec<String> = outputs.iter().map(|p| shown_path(p)).collect();
    let mut result =
        json!({ "command": a.command, "out": shown_path(Path::new(&out)), "outputs": shown });
    if a.background {
        result["job"] = cx.spawn_job(cmd, &outputs, &summary)?.into();
    } else {
        cx.run_making(cmd, &outputs, &summary)?;
    }
    Ok(vec![Content::Json(result)])
}

/// A file of a run at `prefix`, when there is one: one of its `outputs`,
/// or a table of it (a collapse writes one per topic).
fn taken(prefix: &Path, outputs: &[PathBuf]) -> Option<PathBuf> {
    outputs.iter().find(|p| p.exists()).cloned().or_else(|| {
        tables(&prefix.to_string_lossy())
            .into_iter()
            .next()
            .map(|(_, f)| f)
    })
}

/// The next free `{command}`, `{command}-2`, ... in today's results folder,
/// and what a run there writes: no file of it there, and none held by a job.
fn free_prefix(
    cli: &clap::Command,
    command: &str,
    argv: &[String],
    cx: &Call,
) -> anyhow::Result<(String, Vec<PathBuf>)> {
    let dir = cx
        .results_dir()
        .context("the server keeps no records here: give out")?;
    for k in 1.. {
        let p = match k {
            1 => dir.join(command),
            k => dir.join(format!("{command}-{k}")),
        };
        let out = p.to_string_lossy().into_owned();
        let outputs = plan(cli, argv, &out)?;
        if taken(&p, &outputs).is_none() && cx.ensure_free(&outputs).is_ok() {
            return Ok((out, outputs));
        }
    }
    unreachable!("an unbounded search ends in a free name")
}

/// `p` as results give paths: from the root.
fn shown_path(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Order {
    Asc,
    Desc,
    Abs,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    prefix: String,
    table: Option<String>,
    sort: Option<String>,
    order: Option<Order>,
    top: Option<usize>,
    genes: Option<Vec<String>>,
}

/// The run's tables under `prefix`: name and file, by name. A name with a
/// dot in it is another run's (`{prefix}.v2.effect`), not this one's.
fn tables(prefix: &str) -> Vec<(String, PathBuf)> {
    let prefix = Path::new(prefix);
    let Some(base) = prefix.file_name().map(|b| b.to_string_lossy() + ".") else {
        return vec![];
    };
    let parent = match prefix.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let table = name.strip_prefix(base.as_ref())?.strip_suffix(".parquet")?;
            (!table.contains('.')).then(|| (table.to_string(), e.path()))
        })
        .collect();
    found.sort();
    found
}

/// A cell as JSON: a number where it reads as one (NaN as null).
fn cell(s: &str) -> Value {
    match s.parse::<f64>() {
        Ok(x) if x.is_finite() => json!(x),
        Ok(_) => Value::Null,
        Err(_) => json!(s),
    }
}

fn is_probability(column: &str) -> bool {
    column.contains("pvalue") || column.contains("qvalue")
}

/// The run's tables under `prefix`, each with its columns and rows.
fn list_tables(prefix: &str, found: &[(String, PathBuf)]) -> anyhow::Result<Vec<Content>> {
    let listed = found
        .iter()
        .map(|(t, f)| {
            let reader = TableReader::open(&f.to_string_lossy())?;
            let n_rows = reader.rows(&[0])?.count();
            Ok(json!({ "table": t, "file": shown_path(f), "columns": reader.header(), "n_rows": n_rows }))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(vec![Content::Json(
        json!({ "prefix": prefix, "tables": listed }),
    )])
}

fn read(a: ReadArgs) -> anyhow::Result<Vec<Content>> {
    let ReadArgs {
        prefix,
        table,
        sort,
        order,
        top,
        genes,
    } = a;
    let file = if prefix.ends_with(".parquet") && Path::new(&prefix).is_file() {
        PathBuf::from(&prefix)
    } else {
        let found = tables(&prefix);
        anyhow::ensure!(!found.is_empty(), "no cocoa tables under {prefix}");
        let Some(table) = &table else {
            return list_tables(&prefix, &found);
        };
        found
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, f)| f.clone())
            .ok_or_else(|| {
                let names: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
                anyhow::anyhow!("no table {table:?}: {prefix} has {}", names.join(", "))
            })?
    };

    let reader = TableReader::open(&file.to_string_lossy())?;
    let header = reader.header();
    let all: Vec<usize> = (0..header.len()).collect();
    let mut n_rows = 0;
    let mut rows = Vec::new();
    for row in reader.rows(&all)? {
        let row = row?;
        n_rows += 1;
        let wanted = genes.as_ref().is_none_or(|genes| {
            row.first()
                .is_some_and(|g| genes.iter().any(|x| x == g.as_ref()))
        });
        if wanted {
            rows.push(row);
        }
    }
    let sort = match &sort {
        Some(s) => Some(reader.select(&[s])?[0]),
        None => header
            .iter()
            .position(|h| is_probability(h))
            .or_else(|| header.iter().position(|h| h.starts_with("contrast"))),
    };
    if let Some(j) = sort {
        let order = order.unwrap_or(if is_probability(&header[j]) {
            Order::Asc
        } else {
            Order::Abs
        });
        let key = |r: &Vec<Box<str>>| {
            let x = r[j].parse::<f64>().unwrap_or(f64::NAN);
            match order {
                Order::Asc => x,
                Order::Desc => -x,
                Order::Abs => -x.abs(),
            }
        };
        let mut keyed: Vec<(f64, Vec<Box<str>>)> = rows.into_iter().map(|r| (key(&r), r)).collect();
        // unreadable and NaN values last
        keyed.sort_by(|(x, _), (y, _)| x.is_nan().cmp(&y.is_nan()).then(x.total_cmp(y)));
        rows = keyed.into_iter().map(|(_, r)| r).collect();
    }
    rows.truncate(top.unwrap_or(TOP));
    let rows: Vec<Value> = rows
        .iter()
        .map(|r| {
            let obj: Map<String, Value> = header
                .iter()
                .zip(r.iter())
                .map(|(h, v)| (h.to_string(), cell(v)))
                .collect();
            Value::Object(obj)
        })
        .collect();
    Ok(vec![Content::Json(json!({
        "file": shown_path(&file),
        "table": table,
        "columns": header,
        "n_rows": n_rows,
        "sorted_by": sort.map(|j| &header[j]),
        "rows": rows,
    }))])
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocsArgs {
    topic: Option<String>,
}

fn docs(a: DocsArgs) -> anyhow::Result<Vec<Content>> {
    let Some(topic) = a.topic else {
        let topics: Vec<Value> = DOCS
            .iter()
            .map(|(t, text)| {
                let title = text.lines().next().unwrap_or("").trim_start_matches("# ");
                json!({ "topic": t, "title": title })
            })
            .collect();
        return Ok(vec![Content::Json(json!({ "topics": topics }))]);
    };
    let (_, text) = DOCS.iter().find(|(t, _)| *t == topic).ok_or_else(|| {
        let names: Vec<&str> = DOCS.iter().map(|(t, _)| *t).collect();
        anyhow::anyhow!("topic {topic:?}: one of {}", names.join(", "))
    })?;
    Ok(vec![Content::Text(text.to_string())])
}
