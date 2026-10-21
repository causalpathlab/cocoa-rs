use super::*;

#[test]
fn the_status_counts_the_runs_and_says_what_went_wrong() {
    let s = [State::Done, State::Failed("x".into()), State::Waiting];
    assert_eq!(status(&s, false), "runs going: 1 of 3 done, 1 failed");
    assert_eq!(status(&[State::Done], true), "runs finished: 1 of 1 done");
}

#[test]
fn a_step_of_the_log_is_what_the_run_is_doing() {
    let l = "[2026-10-21T13:00:00Z INFO  cocoa::run_diff] Fitting the stage-1 baseline...";
    assert_eq!(doing(l).as_deref(), Some("Fitting the stage-1 baseline"));
    assert_eq!(
        doing("[.. INFO  x] topic 3: max exposure shift removed = 0.04"),
        None
    );
}

#[test]
fn the_script_runs_straight_through_from_its_folder() {
    let job = Job {
        command: "diff".into(),
        argv: [
            "diff",
            crate::runs::BATCH,
            "s.zarr.zip",
            "-i",
            "s.samples.gz",
            "--output",
            "d",
        ]
        .map(String::from)
        .to_vec(),
        dir: PathBuf::from("/w/sim"),
        out: PathBuf::from("/w/sim/d"),
        outputs: vec![PathBuf::from("/w/sim/d.effect.parquet")],
    };
    assert_eq!(job.script(), PathBuf::from("/w/sim/d.cmd.sh"));
    assert_eq!(job.out_name(), "d");
    let text = with_script(&job, script::text);
    assert!(text.contains("Run it again with: bash d.cmd.sh"), "{text}");
    assert!(text.contains("if [ -e d.effect.parquet ]"), "{text}");
    assert!(
        text.contains(
            "\"${COCOA:-cocoa}\" diff --batch-process \\\n  s.zarr.zip \\\n  -i s.samples.gz"
        ),
        "{text}"
    );
    assert_eq!(relative(Path::new("/a/b"), Path::new("/c")), "/a/b");
}

#[test]
fn a_tap_given_at_the_start_hears_the_first_line() {
    let tmp = tempfile::tempdir().unwrap();
    let job = Job {
        command: "diff".into(),
        argv: ["-c", "echo hello >&2"].map(String::from).to_vec(),
        dir: tmp.path().to_path_buf(),
        out: tmp.path().join("d"),
        outputs: vec![],
    };
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let h = heard.clone();
    let tap: Tap = Box::new(move |l| h.lock().unwrap().push(l.into()));
    let q = Queue::start(
        vec![job],
        PathBuf::from("sh"),
        Default::default(),
        Some(tap),
    );
    let t = std::time::Instant::now();
    while !q.finished() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let heard = heard.lock().unwrap();
    assert!(heard[0].starts_with("── cocoa diff"), "{heard:?}");
    assert!(heard.iter().any(|l| l == "hello"), "{heard:?}");
}
