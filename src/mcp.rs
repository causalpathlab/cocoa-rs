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

use crate::runs::{cocoa_cli, plan, subcommand, tables, taken, BATCH, COMMANDS};
use anyhow::Context;
use legume_mcp::{cli, parse_args, shown_path, Call, Content, Handler, ServerInfo, ToolSpec};
use legume_numeric::matrix::table::TableReader;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The name `cocoa mcp` serves under, and that its followers find it by.
pub const SERVER: &str = "cocoa";

const INSTRUCTIONS: &str = "\
cocoa estimates exposure effects on single-cell gene expression across \
individuals, adjusted for confounders (counterfactual confounder adjustment, \
Park & Kellis 2021). `cocoa_commands` lists the commands (diff, collapse, \
simulate-one, simulate-collider) and, with one, its flags. `cocoa_run` runs \
one: data files go in `data`, flags by their long name in `flags`; the \
command line is checked as cocoa parses it before anything runs, and an \
existing run is never written over. A diff on a large data set takes a while: \
set `background` and follow the job with `job_status` / `job_wait`; a \
background diff runs on cocoa's own diff view in a session the user can \
attach to (`cocoa attach`), and it goes on if this server ends. \
`cocoa_read` lists a run's tables (effect, delta, contrast, stage1, perm, \
...) and, with a table, returns its top rows sorted by a column (p-values \
ascending, other columns by absolute value), or the rows of given genes. \
`docs` prints the method write-ups: what the estimator identifies and \
assumes. Relative paths are read from the project folder; results \
give the paths written.";

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
    let info = ServerInfo {
        name: SERVER.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        instructions: Some(INSTRUCTIONS.into()),
    };
    legume_mcp::serve_in_project(info, Cocoa)
}

struct Cocoa;

impl Handler for Cocoa {
    fn tools(&self) -> Vec<ToolSpec> {
        tools()
    }

    fn call(&mut self, name: &str, args: Value, cx: &Call) -> anyhow::Result<Vec<Content>> {
        match name {
            "cocoa_commands" => commands(parse_args(args)?),
            "cocoa_run" => run(parse_args(args)?, cx),
            "cocoa_read" => read(parse_args(args)?, cx),
            "docs" => docs(parse_args(args)?),
            _ => unreachable!("the server answers unknown tools itself"),
        }
    }
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
                    "background": { "type": "boolean", "description": "Run as a background job: the call returns its id at once (a diff's, with the session that shows it); follow it with job_wait or job_status, and read the tables only once it is done" },
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
                    "genes": { "type": "array", "items": { "type": "string" }, "description": "Only these rows, by name (the first text column, a gene)" },
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

/// The flags of `sub` an agent sets: its own, not the view's
/// (`--batch-process`, `--watch`, the session's), which its runs never
/// open.
fn agent_flags(sub: &clap::Command) -> impl Iterator<Item = &clap::Arg> {
    cli::settable(sub).filter(|a| !crate::tui::VIEW_IDS.contains(&a.get_id().as_str()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandsArgs {
    command: Option<String>,
}

fn commands(a: CommandsArgs) -> anyhow::Result<Vec<Content>> {
    let app = cocoa_cli();
    let about = |sub: &clap::Command| sub.get_about().map(|s| s.to_string());
    let Some(name) = a.command else {
        let commands = COMMANDS
            .iter()
            .map(|&c| Ok(json!({ "command": c, "about": about(subcommand(&app, c)?) })))
            .collect::<anyhow::Result<Vec<_>>>()?;
        return Ok(vec![Content::Json(json!({ "commands": commands }))]);
    };
    let sub = subcommand(&app, &name)?;
    let flags: Vec<Value> = agent_flags(sub).map(cli::describe).collect();
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

/// The command line for `command` from the agent's data files and flags,
/// without `--output`; a diff runs straight through, as its script will.
fn command_line(sub: &clap::Command, a: &RunArgs) -> anyhow::Result<Vec<String>> {
    if let Some(view) = a.flags.keys().find(|k| {
        crate::tui::VIEW_IDS.contains(&k.trim_start_matches('-').replace('-', "_").as_str())
    }) {
        anyhow::bail!("{view} is the view's, not a flag a run takes");
    }
    let mut argv = vec![a.command.clone()];
    if !a.data.is_empty() {
        anyhow::ensure!(
            sub.get_positionals().next().is_some(),
            "{} takes no data files",
            a.command
        );
        argv.extend(a.data.iter().cloned());
    }
    let own = [("output", "out"), ("data_files", "data")];
    argv.extend(cli::to_argv(sub, &a.flags, &own, "cocoa_commands")?);
    Ok(argv)
}

/// The run the agent works on, published for the views that follow it
/// (`cocoa diff --watch`): the command, its output prefix and what it
/// writes, absolute, so a view reads it once it is all there.
fn publish(cx: &Call, command: &str, out: &str, outputs: &[PathBuf]) {
    let outputs: Vec<String> = outputs.iter().map(|o| absolute(o)).collect();
    cx.state(&json!({ "command": command, "out": absolute(Path::new(out)), "outputs": outputs }));
}

/// `p` whole, from the project folder the server works in.
fn absolute(p: &Path) -> String {
    legume_tui::browse::normalize(p)
        .to_string_lossy()
        .into_owned()
}

fn run(a: RunArgs, cx: &Call) -> anyhow::Result<Vec<Content>> {
    let app = cocoa_cli();
    let argv = command_line(subcommand(&app, &a.command)?, &a)?;
    let (out, outputs) = match &a.out {
        Some(out) => {
            let outputs = plan(&app, &argv, out)?;
            if let Some(found) = taken(Path::new(out), &outputs) {
                anyhow::bail!(
                    "{} exists: cocoa_run never writes over a run; choose another out",
                    found.display()
                );
            }
            (out.clone(), outputs)
        }
        None => cx.free_prefix(
            &a.command,
            |out| plan(&app, &argv, out),
            crate::runs::has_tables,
        )?,
    };

    // cocoa run on `argv` to `out`, `mode` after the command's name: a diff
    // runs straight through (BATCH) or on its view (--job).
    let cocoa = |mode: Option<&str>| -> anyhow::Result<std::process::Command> {
        let (name, rest) = argv.split_first().expect("a command's name");
        let output = format!("--output={out}");
        let words = std::iter::once(name.as_str())
            .chain(mode)
            .chain(rest.iter().map(String::as_str))
            .chain([output.as_str()]);
        let mut cmd = legume_mcp::own_command(words)?;
        cmd.env("RUST_LOG", "info");
        Ok(cmd)
    };
    let is_diff = a.command == "diff";
    let cmd = cocoa(is_diff.then_some(BATCH))?;
    let summary = format!("cocoa {} to {out}", a.command);
    let shown: Vec<String> = outputs.iter().map(|p| shown_path(p)).collect();
    let mut result =
        json!({ "command": a.command, "out": shown_path(Path::new(&out)), "outputs": shown });
    publish(cx, &a.command, &out, &outputs);
    if a.background && is_diff {
        // In a session of its own, on the diff view: `cocoa attach` shows it,
        // and it outlives this server.
        let started = cx.start_session_job(legume_mcp::SessionJob {
            tool: crate::tui::TOOL,
            screen: a.command.clone(),
            label: Path::new(&out)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            watch: cocoa(Some("--job"))?,
            record: cmd,
            outputs: outputs.clone(),
            summary,
        })?;
        result["job"] = started.job.into();
        result["session"] = started.session.into();
    } else if a.background {
        result["job"] = cx.spawn_job(cmd, &outputs, &summary)?.into();
    } else {
        cx.run_making(cmd, &outputs, &summary)?;
    }
    Ok(vec![Content::Json(result)])
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
    genes: Option<Vec<Value>>,
}

/// A parquet table's footer: its columns, which of them are text (names,
/// flags; a nested column is shown as text too), and its row count.
struct Footer {
    columns: Vec<String>,
    text: Vec<bool>,
    n_rows: usize,
}

fn footer(file: &Path) -> anyhow::Result<Footer> {
    use parquet::basic::{ConvertedType, Type};
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let shown = file.display();
    let f = std::fs::File::open(file).with_context(|| format!("opening {shown}"))?;
    let reader = SerializedFileReader::new(f).with_context(|| format!("reading {shown}"))?;
    let meta = reader.metadata().file_metadata();
    let fields = meta.schema().get_fields();
    let text = fields
        .iter()
        .map(|f| {
            !f.is_primitive()
                || (matches!(
                    f.get_physical_type(),
                    Type::BYTE_ARRAY | Type::FIXED_LEN_BYTE_ARRAY
                ) && f.get_basic_info().converted_type() != ConvertedType::DECIMAL)
        })
        .collect();
    Ok(Footer {
        columns: fields.iter().map(|f| f.name().to_string()).collect(),
        text,
        n_rows: usize::try_from(meta.num_rows()).with_context(|| format!("{shown}: row count"))?,
    })
}

/// The `genes` asked for, as names: text, or whole numbers (`4` and `4.0`
/// both name gene `4`).
fn gene_names(genes: &[Value]) -> anyhow::Result<Vec<String>> {
    genes
        .iter()
        .map(|g| match g {
            Value::String(g) => Ok(g.clone()),
            Value::Number(n) if n.is_i64() || n.is_u64() => Ok(n.to_string()),
            Value::Number(n) => match n.as_f64() {
                Some(x) if x.fract() == 0.0 && x.abs() < 2f64.powi(53) => Ok(format!("{x:.0}")),
                _ => anyhow::bail!("genes: {n} is not a gene name"),
            },
            g => anyhow::bail!("genes: {g} is not a gene name"),
        })
        .collect()
}

/// A numeric cell as JSON (NaN, missing and unreadable as null).
fn cell(s: &str) -> Value {
    match s.parse::<f64>() {
        Ok(x) if x.is_finite() => json!(x),
        _ => Value::Null,
    }
}

fn is_probability(column: &str) -> bool {
    column.contains("pvalue") || column.contains("qvalue")
}

/// The run's tables under `prefix`, each with its columns and rows.
fn list_tables(prefix: &str, found: &[(String, PathBuf)]) -> anyhow::Result<Vec<Content>> {
    let out = absolute(Path::new(prefix));
    let listed = found
        .iter()
        .map(|(t, f)| {
            let footer = footer(f)?;
            Ok(json!({ "table": t, "file": shown_path(f), "columns": footer.columns, "n_rows": footer.n_rows }))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(vec![Content::Json(
        json!({ "prefix": prefix, "out": out, "tables": listed }),
    )])
}

fn read(a: ReadArgs, cx: &Call) -> anyhow::Result<Vec<Content>> {
    let ReadArgs {
        prefix,
        table,
        sort,
        order,
        top,
        genes,
    } = a;
    let genes: Option<HashSet<String>> = genes
        .as_deref()
        .map(gene_names)
        .transpose()?
        .map(|g| g.into_iter().collect());
    // The run read, whole (a table given as a file: its prefix).
    let (file, out) = if prefix.ends_with(".parquet") && Path::new(&prefix).is_file() {
        let file = PathBuf::from(&prefix);
        // `fit.contrast.parquet` is of run `fit`: by the file's name alone.
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
        let name = name
            .as_deref()
            .unwrap_or_default()
            .trim_end_matches(".parquet");
        let run = file.with_file_name(name.rsplit_once('.').map_or(name, |(run, _)| run));
        let run = run.to_string_lossy().into_owned();
        publish(cx, "read", &run, &[]);
        (file, absolute(Path::new(&run)))
    } else {
        let found = tables(&prefix);
        anyhow::ensure!(!found.is_empty(), "no cocoa tables under {prefix}");
        publish(cx, "read", &prefix, &[]);
        let Some(table) = &table else {
            return list_tables(&prefix, &found);
        };
        let file = found
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, f)| f.clone())
            .ok_or_else(|| {
                let names: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
                anyhow::anyhow!("no table {table:?}: {prefix} has {}", names.join(", "))
            })?;
        (file, absolute(Path::new(&prefix)))
    };

    let reader = TableReader::open(&file.to_string_lossy())?;
    let header = reader.header();
    let footer = footer(&file)?;
    // rows are named by the first text column (a gene)
    let name = match footer.text.iter().position(|&t| t) {
        Some(j) => j,
        None if genes.is_some() => {
            anyhow::bail!("{}: no name column to pick genes by", file.display())
        }
        None => 0,
    };
    let all: Vec<usize> = (0..header.len()).collect();
    let mut rows = Vec::new();
    for row in reader.rows(&all)? {
        let row = row?;
        let wanted = genes
            .as_ref()
            .is_none_or(|genes| genes.contains(row[name].as_ref()));
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
                .zip(&footer.text)
                .map(|((h, v), &is_text)| {
                    let v = if is_text { json!(v) } else { cell(v) };
                    (h.to_string(), v)
                })
                .collect();
            Value::Object(obj)
        })
        .collect();
    Ok(vec![Content::Json(json!({
        "out": out,
        "file": shown_path(&file),
        "table": table,
        "columns": header,
        "n_rows": footer.n_rows,
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
