//! `cocoa mcp` over the protocol: the real binary on stdio, so the tools
//! that run a child `cocoa` run it.

mod common;

use common::{build, Spec};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Each call's result, in order, from one server session in `project`.
fn calls(project: &Path, calls: &[(&str, Value)]) -> Vec<Value> {
    calls_from(project, project, calls)
}

/// [`calls`] from a server started in `cwd` with `project` as given.
fn calls_from(cwd: &Path, project: &Path, calls: &[(&str, Value)]) -> Vec<Value> {
    let mut input = String::new();
    for (i, (name, args)) in calls.iter().enumerate() {
        let m = json!({ "jsonrpc": "2.0", "id": i, "method": "tools/call",
                        "params": { "name": name, "arguments": args } });
        input.push_str(&format!("{m}\n"));
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_cocoa"))
        .arg("mcp")
        .current_dir(cwd)
        .env("LEGUME_MCP_PROJECT", project)
        .env("LEGUME_MCP_WATCH", "off")
        // A background diff hosts a session: here, not among the user's,
        // and not refused as it would be under a multiplexer.
        .env("LEGUME_TUI_DIR", project.join("sessions"))
        .env_remove("TMUX")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("run cocoa mcp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut results: Vec<(u64, Value)> = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter_map(|m| Some((m["id"].as_u64()?, m["result"].clone())))
        .collect();
    results.sort_by_key(|(i, _)| *i);
    assert_eq!(results.len(), calls.len(), "{text}");
    results.into_iter().map(|(_, r)| r).collect()
}

fn ok(r: &Value) -> &Value {
    assert_ne!(r["isError"], true, "{r}");
    &r["structuredContent"]
}

fn error_text(r: &Value) -> &str {
    assert_eq!(r["isError"], true, "{r}");
    r["content"][0]["text"].as_str().unwrap()
}

fn fixture() -> common::Fixture {
    build(&Spec {
        n_genes: 30,
        n_indv: 8,
        n_states: 2,
        cells_per_indv: 20,
        exposure_shifts_states: false,
    })
}

fn diff_args(fx: &common::Fixture, out: &str) -> Value {
    json!({
        "command": "diff",
        "data": [fx.data()],
        "flags": {
            "indv-files": [fx.samples()],
            "exposure-assignment-file": fx.exposures(),
            "topic-assignment-files": [fx.one_topic()],
            "preload-data": true,
        },
        "out": out,
    })
}

#[test]
fn lists_the_tools_and_each_commands_flags() {
    let dir = tempfile::tempdir().unwrap();
    let r = calls(
        dir.path(),
        &[
            ("cocoa_commands", json!({})),
            ("cocoa_commands", json!({ "command": "diff" })),
            ("cocoa_commands", json!({ "command": "mcp" })),
        ],
    );
    let names: Vec<&str> = ok(&r[0])["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["diff", "collapse", "simulate-one", "simulate-collider"]
    );
    let flags = ok(&r[1])["flags"].as_array().unwrap();
    let flag = |name: &str| flags.iter().find(|f| f["flag"] == name).unwrap();
    assert_eq!(flag("--output")["required"], true);
    assert_eq!(flag("--preload-data")["switch"], true);
    assert_eq!(flag("data-files")["positional"], true);
    assert!(error_text(&r[2]).contains("diff"));
}

#[test]
fn runs_diff_reads_it_back_and_never_writes_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let out = dir.path().join("fit").to_string_lossy().into_owned();
    let r = calls(
        dir.path(),
        &[
            ("cocoa_run", diff_args(&fx, &out)),
            ("cocoa_run", diff_args(&fx, &out)),
            ("cocoa_read", json!({ "prefix": out })),
            (
                "cocoa_read",
                json!({ "prefix": out, "table": "contrast", "top": 3 }),
            ),
            (
                "cocoa_read",
                json!({ "prefix": out, "table": "contrast", "top": 100 }),
            ),
            (
                "cocoa_read",
                json!({ "prefix": out, "table": "contrast", "genes": ["GENE2"] }),
            ),
            ("cocoa_read", json!({ "prefix": "fit" })),
            (
                "cocoa_read",
                json!({ "prefix": "fit", "table": "contrast", "top": 1 }),
            ),
        ],
    );
    // A read names its run whole, as a run does: whoever follows the
    // agent opens it from anywhere.
    for read in [&r[6], &r[7]] {
        let named = ok(read)["out"].as_str().unwrap().to_string();
        assert!(
            Path::new(&named).is_absolute() && named.ends_with("/fit"),
            "{named}"
        );
    }
    let run = ok(&r[0]);
    for table in ["effect", "delta", "contrast", "stage1"] {
        let f = format!("{out}.{table}.parquet");
        assert!(Path::new(&f).exists(), "{f}");
        assert!(run["outputs"].to_string().contains(&f), "{run}");
    }
    assert!(error_text(&r[1]).contains("exists"));

    let tables: Vec<&str> = ok(&r[2])["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["table"].as_str().unwrap())
        .collect();
    assert!(tables.contains(&"contrast"), "{tables:?}");

    let top = ok(&r[3]);
    assert_eq!(top["rows"].as_array().unwrap().len(), 3);
    assert_eq!(top["n_rows"], 30);
    assert_eq!(top["columns"][0], "gene");
    let by = top["sorted_by"].as_str().unwrap();
    assert!(by.starts_with("contrast"), "{by}");
    // largest magnitude first, the genes left out (no estimate) last
    let sizes: Vec<Option<f64>> = ok(&r[4])["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[by].as_f64().map(f64::abs))
        .collect();
    assert_eq!(sizes.len(), 30);
    assert!(
        sizes.windows(2).all(|w| match (w[0], w[1]) {
            (Some(a), Some(b)) => a >= b,
            (a, b) => a.is_some() || b.is_none(),
        }),
        "{sizes:?}"
    );
    let one = ok(&r[5])["rows"].as_array().unwrap().clone();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0]["gene"], "GENE2");

    // the project's driver script can redo the run without the agent
    let day = std::fs::read_dir(dir.path().join("results"))
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .unwrap()
        .path();
    let runall = std::fs::read_to_string(day.join("runall.sh")).unwrap();
    assert!(runall.contains("cocoa diff"), "{runall}");
}

#[test]
fn refuses_what_cocoa_would_refuse_before_running() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let out = dir.path().join("bad").to_string_lossy().into_owned();
    let mut unknown = diff_args(&fx, &out);
    unknown["flags"]["no-such-flag"] = json!(1);
    let mut missing = diff_args(&fx, &out);
    missing["flags"]
        .as_object_mut()
        .unwrap()
        .remove("exposure-assignment-file");
    let mut twice = diff_args(&fx, &out);
    twice["flags"]["output"] = json!(out);
    let r = calls(
        dir.path(),
        &[
            ("cocoa_run", unknown),
            ("cocoa_run", missing),
            ("cocoa_run", twice),
            ("cocoa_run", json!({ "command": "mcp" })),
        ],
    );
    assert!(error_text(&r[0]).contains("no-such-flag"));
    assert!(error_text(&r[1]).contains("exposure"));
    assert!(error_text(&r[2]).contains("out"));
    assert!(error_text(&r[3]).contains("diff"));
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("bad.")));
}

#[test]
fn a_table_read_as_a_file_names_its_run_by_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let out = dir.path().join("v1.2").join("fit");
    let out = out.to_string_lossy().into_owned();
    ok(&calls(dir.path(), &[("cocoa_run", diff_args(&fx, &out))])[0]);
    let table = dir.path().join("v1.2").join("table.parquet");
    std::fs::copy(format!("{out}.contrast.parquet"), &table).unwrap();
    let read = json!({ "prefix": table, "top": 1 });
    let r = calls(dir.path(), &[("cocoa_read", read)]);
    let named = ok(&r[0])["out"].as_str().unwrap().to_string();
    assert!(named.ends_with("/v1.2/table"), "{named}");
}

#[test]
fn a_background_diff_runs_in_a_session_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let out = dir.path().join("fit").to_string_lossy().into_owned();
    let mut diff = diff_args(&fx, &out);
    diff["background"] = true.into();
    let r = calls(
        dir.path(),
        &[
            ("cocoa_run", diff),
            ("job_wait", json!({ "id": "job-1", "seconds": 120 })),
        ],
    );
    let run = ok(&r[0]);
    assert_eq!(run["job"], "job-1", "{run}");
    assert!(
        run["session"].as_str().is_some_and(|s| !s.is_empty()),
        "{run}"
    );
    assert!(r[1].to_string().contains("done"), "{}", r[1]);
    // The view's log reaches the agent from its first line.
    let lines = r[1]["structuredContent"]["last_lines"].to_string();
    assert!(lines.contains("Writing"), "{}", r[1]);
    for table in ["effect", "delta", "contrast"] {
        let f = format!("{out}.{table}.parquet");
        assert!(Path::new(&f).exists(), "{f}");
    }
    let days = dir.path().join("results");
    let script = std::fs::read_dir(&days)
        .unwrap()
        .map(|d| d.unwrap().path().join("runall.sh"))
        .find(|p| p.exists())
        .expect("runall.sh");
    let text = std::fs::read_to_string(script).unwrap();
    // The record runs straight through, as a script does, not in the view.
    assert!(text.contains("cocoa diff --batch-process"), "{text}");
    assert!(text.contains("fit.effect.parquet"), "{text}");
    assert!(!text.contains("--job"), "{text}");
}

#[test]
fn default_output_goes_to_the_days_results() {
    let dir = tempfile::tempdir().unwrap();
    // a negative value stays a value; jobs hold their names
    let one = json!({
        "command": "simulate-one",
        "flags": { "n-genes": 20, "n-cells": 200, "n-causal-genes": 2,
                   "n-samples-per-exposure": 3, "rseed": 1,
                   "indv-dispersion-trend": [0.5, -0.5] },
        "background": true,
    });
    let collider = json!({
        "command": "simulate-collider",
        "flags": { "n-genes": 20, "n-cells": 200, "n-causal-genes": 2,
                   "n-cell-types": 2, "n-exposure": 2 },
    });
    let r = calls(
        dir.path(),
        &[
            ("cocoa_run", one.clone()),
            ("cocoa_run", one),
            ("job_wait", json!({ "id": "job-1", "seconds": 60 })),
            ("job_wait", json!({ "id": "job-2", "seconds": 60 })),
            ("cocoa_run", collider),
        ],
    );
    let (a, b) = (ok(&r[0]), ok(&r[1]));
    let (a, b) = (a["out"].as_str().unwrap(), b["out"].as_str().unwrap());
    assert_ne!(a, b, "a second run takes the next free name");
    assert!(a.contains("results"), "{a}");
    for w in &r[2..4] {
        assert!(w.to_string().contains("done"), "{w}");
    }
    let counts = format!("{a}.zarr.zip");
    assert!(Path::new(&counts).exists());
    assert!(ok(&r[0])["outputs"].to_string().contains(&counts));
    let c = ok(&r[4]);
    assert!(Path::new(&format!("{}.celltypes.gz", c["out"].as_str().unwrap())).exists());
}

#[test]
fn names_stay_text_when_they_read_as_numbers() {
    // the simulator names genes and individuals by number
    let dir = tempfile::tempdir().unwrap();
    let sim = dir.path().join("sim").to_string_lossy().into_owned();
    let fit = dir.path().join("fit").to_string_lossy().into_owned();
    let simulate = json!({
        "command": "simulate-one", "out": sim,
        "flags": { "n-genes": 20, "n-cells": 300, "n-causal-genes": 2,
                   "n-samples-per-exposure": 3, "rseed": 1 },
    });
    let r = calls(dir.path(), &[("cocoa_run", simulate)]);
    ok(&r[0]);
    let n_cells = common::read_gz(&format!("{sim}.samples.gz"))
        .lines()
        .count();
    let topics = format!("{sim}.topic.txt");
    std::fs::write(&topics, "0\n".repeat(n_cells)).unwrap();
    let diff = json!({
        "command": "diff", "data": [format!("{sim}.zarr.zip")], "out": fit,
        "flags": { "indv-files": [format!("{sim}.samples.gz")],
                   "exposure-assignment-file": format!("{sim}.exposures.gz"),
                   "topic-assignment-files": [topics] },
    });
    let r = calls(
        dir.path(),
        &[
            ("cocoa_run", diff),
            (
                "cocoa_read",
                json!({ "prefix": fit, "table": "contrast", "top": 100 }),
            ),
            (
                "cocoa_read",
                json!({ "prefix": fit, "table": "contrast", "genes": ["3", 4.0] }),
            ),
            (
                "cocoa_read",
                json!({ "prefix": fit, "table": "delta", "top": 1 }),
            ),
            ("cocoa_read", json!({ "prefix": fit })),
        ],
    );
    ok(&r[0]);
    let all = ok(&r[1])["rows"].as_array().unwrap().clone();
    assert!(all.iter().all(|r| r["gene"].is_string()), "{all:?}");
    let picked: Vec<&str> = ok(&r[2])["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["gene"].as_str().unwrap())
        .collect();
    assert_eq!(picked.len(), 2, "{picked:?}");
    let delta = &ok(&r[3])["rows"][0];
    let names = delta
        .as_object()
        .unwrap()
        .values()
        .filter(|v| v.is_string());
    assert!(names.count() >= 2, "gene and individual are names: {delta}");
    let listed = ok(&r[4])["tables"].as_array().unwrap().clone();
    let contrast = listed.iter().find(|t| t["table"] == "contrast").unwrap();
    assert_eq!(contrast["n_rows"], 20);
}

#[test]
fn refuses_unknown_arguments_and_output_as_a_flag() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let out = dir.path().join("x").to_string_lossy().into_owned();
    let mut typo = diff_args(&fx, &out);
    typo["backgound"] = json!(true);
    let mut short = diff_args(&fx, &out);
    short.as_object_mut().unwrap().remove("out");
    short["flags"]["o"] = json!(out);
    let r = calls(dir.path(), &[("cocoa_run", typo), ("cocoa_run", short)]);
    assert!(error_text(&r[0]).contains("backgound"));
    assert!(error_text(&r[1]).contains("out"));
}

#[test]
fn a_relative_project_is_read_from_where_the_server_starts() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::create_dir(parent.path().join("proj")).unwrap();
    let args = json!({
        "command": "simulate-one",
        "flags": { "n-genes": 20, "n-cells": 100, "n-causal-genes": 2,
                   "n-samples-per-exposure": 2 },
    });
    let r = calls_from(parent.path(), Path::new("proj"), &[("cocoa_run", args)]);
    let out = ok(&r[0])["out"].as_str().unwrap().to_string();
    let results = std::fs::canonicalize(parent.path().join("proj/results")).unwrap();
    let out = std::fs::canonicalize(Path::new(&out).parent().unwrap()).unwrap();
    assert!(
        out.starts_with(&results),
        "{} not under {}",
        out.display(),
        results.display()
    );
    assert!(!parent.path().join("proj/proj").exists());
}

#[test]
fn data_given_as_a_flag_is_pointed_to_data() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture();
    let mut args = diff_args(&fx, &dir.path().join("d").to_string_lossy());
    args["flags"]["data_files"] = json!([fx.data()]);
    let r = calls(dir.path(), &[("cocoa_run", args)]);
    assert!(error_text(&r[0]).contains("`data`"), "{}", r[0]);
}

#[test]
fn serves_the_method_write_ups() {
    let dir = tempfile::tempdir().unwrap();
    let r = calls(
        dir.path(),
        &[
            ("docs", json!({})),
            ("docs", json!({ "topic": "identification" })),
        ],
    );
    assert!(ok(&r[0])["topics"].to_string().contains("identification"));
    assert!(r[1]["content"][0]["text"].as_str().unwrap().len() > 100);
}

#[test]
fn a_simulation_lists_every_file_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let one = json!({ "n-genes": 20, "n-cells": 300, "n-causal-genes": 2,
                      "n-samples-per-exposure": 3, "rseed": 1 });
    let collider = json!({ "n-genes": 20, "n-cells": 300, "n-causal-genes": 2,
                           "n-cell-types": 2, "n-exposure": 2 });
    let mut one_more = one.clone();
    one_more["indv-dispersion-trend"] = json!([0.5, -0.5]);
    one_more["save-mtx"] = json!(true);
    let mut collider_more = collider.clone();
    collider_more["save-mtx"] = json!(true);
    let runs = [
        ("simulate-one", "one", one),
        ("simulate-one", "one_more", one_more),
        ("simulate-collider", "col", collider),
        ("simulate-collider", "col_more", collider_more),
    ];
    let r = calls(
        dir.path(),
        &runs
            .iter()
            .map(|(command, out, flags)| {
                let out = dir.path().join(out).to_string_lossy().into_owned();
                (
                    "cocoa_run",
                    json!({ "command": command, "out": out, "flags": flags }),
                )
            })
            .collect::<Vec<_>>(),
    );
    for ((_, out, _), r) in runs.iter().zip(&r) {
        let mut listed: Vec<String> = ok(r)["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap().rsplit('/').next().unwrap().to_string())
            .collect();
        listed.sort();
        let mut written: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|f| {
                f.strip_prefix(out)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
            .collect();
        written.sort();
        assert_eq!(listed, written, "{out}");
    }
}
