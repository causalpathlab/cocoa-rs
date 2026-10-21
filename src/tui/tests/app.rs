use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn simulate_app(dir: &Path) -> App {
    let cli = runs::cocoa_cli();
    let sim = Sim::new(&cli, Model::Collider).unwrap();
    App::new(cli, dir.to_path_buf(), Some(sim), dir.join("sim")).unwrap()
}

fn key(app: &mut App, code: KeyCode) {
    let session = legume_tui::Session::plain(&TOOL);
    app.key(KeyEvent::new(code, KeyModifiers::NONE), &session);
}

/// `app` once the results it is reading are in.
fn read_in(app: &mut App) {
    let t = std::time::Instant::now();
    while app.opening.is_some() && t.elapsed() < Duration::from_secs(20) {
        app.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The screen drawn `w × h`, as text.
fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_simulation_queues_a_diff_on_its_outputs() {
    let tmp = tempfile::tempdir().unwrap();
    let app = simulate_app(tmp.path());
    let jobs = app.planned.as_ref().unwrap();
    assert_eq!(jobs.len(), 2);
    let (s, d) = (&jobs[0], &jobs[1]);
    assert_eq!(s.out, tmp.path().join("sim/collider"));
    assert_eq!(s.argv.last().map(String::as_str), Some("collider"));
    assert_eq!(d.out, tmp.path().join("sim/collider-diff"));
    // The diff names the simulation's files from their folder, where it runs.
    let line = d.argv.join(" ");
    assert!(
        line.starts_with("diff --batch-process collider.zarr.zip --indv-files collider.samples.gz"),
        "{line}"
    );
    assert!(
        line.contains("--topic-assignment-files collider.celltypes.gz"),
        "{line}"
    );
    assert!(line.ends_with("--output collider-diff"), "{line}");

    // d: the simulation alone.
    let mut app = app;
    key(&mut app, KeyCode::Char('d'));
    assert_eq!(app.planned.as_ref().unwrap().len(), 1);
}

#[test]
fn a_run_there_moves_the_next_one_on_and_a_typed_one_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("sim")).unwrap();
    std::fs::write(tmp.path().join("sim/collider.samples.gz"), b"").unwrap();
    let mut app = simulate_app(tmp.path());
    assert_eq!(
        app.planned.as_ref().unwrap()[0].out,
        tmp.path().join("sim/collider-2")
    );
    app.sim_out = "sim/collider".into();
    app.replan();
    let why = app.planned.as_ref().unwrap_err();
    assert!(why.contains("exists"), "{why}");
}

#[test]
fn a_bad_value_is_clap_s_complaint_and_blamed_on_its_row() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = simulate_app(tmp.path());
    let sim = app.sim.as_mut().unwrap();
    sim.form_mut().get_mut("n-genes").unwrap().value = "many".into();
    app.replan();
    let why = app.planned.as_ref().unwrap_err().clone();
    assert!(why.contains("many"), "{why}");
    app.page = Page::Flags;
    let text = screen(&mut app, 140, 40);
    assert!(text.contains("n-genes"), "{text}");
}

#[test]
fn a_diff_needs_its_inputs_and_the_model_tab_draws_its_dag() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = runs::cocoa_cli();
    let mut app = App::new(cli, tmp.path().to_path_buf(), None, tmp.path().join("sim")).unwrap();
    assert_eq!(app.page, Page::Inputs);
    let why = app.planned.as_ref().unwrap_err();
    assert!(why.contains("data, individuals, exposure"), "{why}");
    let text = screen(&mut app, 120, 30);
    assert!(text.contains("cocoa diff · needs data"), "{text}");

    let mut app = simulate_app(tmp.path());
    let text = screen(&mut app, 140, 40);
    for want in [
        "X ──→ Y ←─ U",
        "‹ collider ›",
        "X → A",
        "individuals   10",
        "Ctrl+R",
    ] {
        assert!(text.contains(want), "{want}\n{text}");
    }
}

#[test]
fn diff_on_a_simulation_takes_its_outputs_and_truth_as_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, diff) = super::results::tests::simulated_diff(tmp.path());
    let _ = diff;
    let mut app = simulate_app(tmp.path());
    let job = Job {
        command: "simulate-collider".into(),
        argv: vec![],
        dir: tmp.path().to_path_buf(),
        out: sim.clone(),
        outputs: vec![PathBuf::from(format!("{}.zarr.zip", sim.display()))],
    };
    app.queue = Some(Queue::start(
        vec![],
        PathBuf::from("true"),
        Default::default(),
        None,
    ));
    app.queue.as_mut().unwrap().jobs = vec![job];
    app.diff_on_sim();
    assert!(app.sim.is_none());
    assert_eq!(app.page, Page::Inputs);
    assert!(app.inputs.missing().is_empty());
    assert!(app.inputs.truth().is_some());
    // The diff there already: the next free prefix.
    let jobs = app.planned.as_ref().unwrap();
    assert_eq!(jobs[0].out, tmp.path().join("collider-diff-2"));

    app.open_results(&tmp.path().join("collider-diff"), None);
    read_in(&mut app);
    app.page = Page::Results;
    let text = screen(&mut app, 150, 50);
    for want in [
        "vs truth · collider.causal.gz",
        "AUROC",
        "log effect · top 5",
        "●",
    ] {
        assert!(text.contains(want), "{want}\n{text}");
    }
}

#[test]
fn keys_that_cannot_change_a_run_do_not_plan_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = simulate_app(tmp.path());
    // A plan only the next replan would replace.
    for page in [Page::Results, Page::Run] {
        app.page = page;
        app.planned = Err("as it was".into());
        key(&mut app, KeyCode::Down);
        assert_eq!(
            app.planned.as_ref().err().map(String::as_str),
            Some("as it was"),
            "{page:?}"
        );
    }
    // On the Model tab a key may change the run: it is planned again.
    app.page = Page::Model;
    key(&mut app, KeyCode::Down);
    assert!(app.planned.is_ok());
}

#[test]
fn the_files_a_run_writes_are_all_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = simulate_app(tmp.path());
    let n = app.planned.as_ref().unwrap()[0].outputs.len();
    assert!(n > 6, "{n}");
    let text = screen(&mut app, 150, 50);
    assert!(text.contains(&format!("… and {} more", n - 6)), "{text}");
}

#[test]
fn a_session_job_ends_as_its_run_did() {
    use legume_mcp::job::End;
    let ok = || Ok(());
    assert!(matches!(job_end(&ok(), Some(State::Done), "o/d"), End::Done(s) if s.contains("o/d")));
    assert!(matches!(
        job_end(&ok(), Some(State::Failed("bad".into())), "d"),
        End::Failed(s) if s == "bad"
    ));
    assert!(matches!(
        job_end(&ok(), Some(State::Stopped), "d"),
        End::Cancelled
    ));
    assert!(matches!(job_end(&ok(), None, "d"), End::Cancelled));
    let err = Err(anyhow::anyhow!("no terminal"));
    assert!(
        matches!(job_end(&err, Some(State::Done), "d"), End::Failed(s) if s.contains("no terminal"))
    );
}

#[test]
fn a_file_typed_in_a_flag_is_found_from_the_runs_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = runs::cocoa_cli();
    let here = tmp.path().to_path_buf();
    let mut app = App::new(cli, here.clone(), None, here.join("sim")).unwrap();
    let data = here.join("data");
    app.inputs.picked = vec![
        (data.join("d.zarr"), Role::Data),
        (data.join("i.gz"), Role::Individuals),
        (data.join("e.gz"), Role::Exposure),
    ];
    app.diff
        .get_mut("adjustment-data-files")
        .unwrap()
        .set_text("data/a.zarr,/abs/b.zarr");
    app.replan();
    let job = &app.planned.as_ref().unwrap()[0];
    assert_eq!(job.dir, data);
    let at = |flag: &str| {
        let i = job.argv.iter().position(|w| w == flag).unwrap();
        job.argv[i + 1].clone()
    };
    // The run starts in data/, as its script does.
    assert_eq!(at("--adjustment-data-files"), "a.zarr,/abs/b.zarr");
}

#[test]
fn a_session_job_that_cannot_start_ends_saying_why() {
    let tmp = tempfile::tempdir().unwrap();
    let here = tmp.path().to_path_buf();
    let app = App::new(runs::cocoa_cli(), here.clone(), None, here.join("sim")).unwrap();
    let session = legume_tui::Session::plain(&TOOL);
    let e = job(&session, app).unwrap_err();
    assert!(e.to_string().contains("Inputs tab"), "{e}");
}

#[test]
fn results_are_read_off_the_screen_and_taken_in_as_they_come() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, diff) = super::results::tests::simulated_diff(tmp.path());
    let mut app = simulate_app(tmp.path());
    app.open_results(&diff, None);
    assert!(app.results.is_none(), "read on the screen's thread");
    app.page = Page::Results;
    assert!(screen(&mut app, 100, 20).contains("reading"));
    read_in(&mut app);
    assert!(app.results.as_ref().is_some_and(|r| r.prefix == diff));
}

#[test]
fn a_diff_of_a_simulation_finds_its_truth_under_either_numbering() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("collider.causal.gz"), b"").unwrap();
    let truth = Some(tmp.path().join("collider.causal.gz"));
    // `-diff-2` is how runs number now; `-diff2`, how cocoa 0.6.0's view
    // numbered them, which projects still hold.
    for name in [
        "collider-diff",
        "collider-diff-2",
        "collider-diff2",
        "collider-diff-12",
    ] {
        assert_eq!(guess_truth(&tmp.path().join(name)), truth, "{name}");
    }
    for name in ["collider-fit", "collider-diff-"] {
        assert_eq!(guess_truth(&tmp.path().join(name)), None, "{name}");
    }
}
