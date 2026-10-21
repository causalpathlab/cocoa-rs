use super::*;

fn words(ws: &[&str]) -> Vec<String> {
    ws.iter().map(|w| w.to_string()).collect()
}

#[test]
fn a_plan_lists_the_outputs_once_cocoa_accepts_the_command_line() {
    let app = cocoa_cli();
    let argv = words(&[
        "simulate-collider",
        "-r",
        "10",
        "-c",
        "20",
        "-a",
        "2",
        "-t",
        "2",
    ]);
    let out = plan(&app, &argv, "/x/sim").unwrap();
    assert_eq!(out[0], PathBuf::from("/x/sim.zarr.zip"));
    assert!(out.contains(&PathBuf::from("/x/sim.causal.gz")));
    let bad = plan(&app, &words(&["simulate-collider", "-r", "ten"]), "/x/sim").unwrap_err();
    assert!(bad.to_string().contains("ten"), "{bad}");
    // A diff takes the view's flag that runs it straight through.
    let diff = words(&["diff", BATCH, "d.zarr", "-i", "i.gz", "-e", "e.gz"]);
    assert!(plan(&app, &diff, "/x/d").is_ok());
}

#[test]
fn the_free_prefix_skips_a_run_already_there() {
    let tmp = tempfile::tempdir().unwrap();
    let app = cocoa_cli();
    let argv = words(&["simulate-one", "-r", "10", "-c", "20", "-a", "2"]);
    let first = free_prefix(&app, &argv, tmp.path(), "one").unwrap();
    assert_eq!(first.0, tmp.path().join("one").to_string_lossy());
    assert!(
        first.1.iter().any(|f| f.ends_with("one.samples.gz")),
        "{first:?}"
    );
    std::fs::write(tmp.path().join("one.samples.gz"), b"").unwrap();
    let next = free_prefix(&app, &argv, tmp.path(), "one").unwrap();
    // Numbered as an agent's runs are: one, one-2, ...
    assert_eq!(next.0, tmp.path().join("one-2").to_string_lossy());
    // A run's tables alone hold its name, whatever this one would write.
    std::fs::write(tmp.path().join("one-2.effect.parquet"), b"").unwrap();
    let third = free_prefix(&app, &argv, tmp.path(), "one").unwrap();
    assert_eq!(third.0, tmp.path().join("one-3").to_string_lossy());
    // A command line clap refuses says why, not which name is free.
    let bad = words(&["simulate-one", "--no-such-flag"]);
    let why = free_prefix(&app, &bad, tmp.path(), "one").unwrap_err();
    assert!(why.to_string().contains("no-such-flag"), "{why}");
    // A table of another run with a dotted name is not this one's.
    std::fs::write(tmp.path().join("d.v2.effect.parquet"), b"").unwrap();
    assert!(tables(&tmp.path().join("d").to_string_lossy()).is_empty());
}
