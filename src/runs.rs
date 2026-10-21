//! What a run of a cocoa command is, as `cocoa mcp` and the views set one
//! up alike: its command line checked by cocoa's own parser, the files it
//! writes, and whether a run is already there, so neither ever writes over
//! one.

use legume_mcp::cli;
use std::path::{Path, PathBuf};

/// The commands an agent or a view may run, in the order `cocoa --help`
/// lists them.
pub const COMMANDS: &[&str] = &["diff", "collapse", "simulate-one", "simulate-collider"];

/// The flag that runs `cocoa diff` straight through, without its view: on
/// every command line a run records, so its script runs as it did.
pub const BATCH: &str = "--batch-process";

/// cocoa's command line, built, so each command carries the global flags.
pub fn cocoa_cli() -> clap::Command {
    use clap::CommandFactory;
    let mut app = crate::Cli::command();
    app.build();
    app
}

/// Command `name` of the built command line `app`.
pub fn subcommand<'a>(app: &'a clap::Command, name: &str) -> anyhow::Result<&'a clap::Command> {
    anyhow::ensure!(
        COMMANDS.contains(&name),
        "command {name:?}: one of {}",
        COMMANDS.join(", ")
    );
    Ok(app
        .find_subcommand(name)
        .expect("every command is a subcommand"))
}

/// The files a run of `argv` (the command first) with output prefix `out`
/// writes, as the command lists them, once cocoa's parser has accepted the
/// command line.
pub fn plan(app: &clap::Command, argv: &[String], out: &str) -> anyhow::Result<Vec<PathBuf>> {
    use crate::{run_collapse::*, run_diff::*, run_sim_collider::*, run_sim_one_type::*};
    use clap::FromArgMatches;
    let words: Vec<String> = argv
        .iter()
        .cloned()
        .chain([format!("--output={out}")])
        .collect();
    let matches = cli::check(app, &words)?;
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

/// A file of a run at `prefix`, when there is one: one of its `outputs`,
/// or a table of it (a collapse writes one per topic).
pub fn taken(prefix: &Path, outputs: &[PathBuf]) -> Option<PathBuf> {
    outputs.iter().find(|p| p.exists()).cloned().or_else(|| {
        tables(&prefix.to_string_lossy())
            .into_iter()
            .next()
            .map(|(_, f)| f)
    })
}

/// The run's tables under `prefix`: name and file, by name. A name with a
/// dot in it is another run's (`{prefix}.v2.effect`), not this one's.
pub fn tables(prefix: &str) -> Vec<(String, PathBuf)> {
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

/// `p` with `suffix` added to its last part: `{p}{suffix}`.
#[must_use]
pub fn suffixed(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// The first `{stem}`, `{stem}-2`, `{stem}-3`, ... under `dir` that no run
/// of `argv` has written (its outputs as `plan` lists them, and its
/// tables), numbered as the agent's runs are, with those outputs; or why
/// there is none (the command line refused, every name taken).
pub fn free_prefix(
    app: &clap::Command,
    argv: &[String],
    dir: &Path,
    stem: &str,
) -> anyhow::Result<(String, Vec<PathBuf>)> {
    // The folder read once: a name is taken when a run's tables are there.
    let held: std::collections::HashSet<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().into_owned();
            let (run, _table) = name.strip_suffix(".parquet")?.rsplit_once('.')?;
            Some(dir.join(run).to_string_lossy().into_owned())
        })
        .collect();
    legume_mcp::free_prefix(
        dir,
        stem,
        |out| plan(app, argv, out),
        |out| held.contains(out),
    )
}

/// Whether a run's tables are under `prefix`: its name is taken, whatever
/// another run there would write.
pub fn has_tables(prefix: &str) -> bool {
    !tables(prefix).is_empty()
}

#[cfg(test)]
#[path = "runs/tests.rs"]
mod tests;
