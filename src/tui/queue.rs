//! The runs a view queued, one after another: each writes its
//! `{out}.cmd.sh`, then starts cocoa in the script's folder with the same
//! command, its log and progress bar kept for the screen. A run after one
//! that did not finish (a diff on a simulation) does not start.
//!
//! senna's queue (`senna run`), in one lane: the program outlives a
//! detached screen, so the runs go on while nobody looks.

use legume_tui::child::{run_one, Failed, Progress, Said, Stopper};
use legume_tui::form::script::{self, Script};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Log lines kept for the screen.
const KEEP: usize = 2000;

/// The log level a run starts with, in the script and from the screen
/// alike, unless `RUST_LOG` says otherwise.
pub const LOG_LEVEL: &str = "info";

/// One run.
#[derive(Clone, Debug)]
pub struct Job {
    /// The command: `diff`, `simulate-collider`.
    pub command: String,
    /// The command line without the program, paths relative to `dir`, its
    /// `--output` last.
    pub argv: Vec<String>,
    /// Where it runs and its script goes.
    pub dir: PathBuf,
    /// The output prefix, absolute.
    pub out: PathBuf,
    /// What it writes, absolute; the first is what a finished run leaves.
    pub outputs: Vec<PathBuf>,
}

impl Job {
    /// `{out}.cmd.sh`.
    #[must_use]
    pub fn script(&self) -> PathBuf {
        crate::runs::suffixed(&self.out, ".cmd.sh")
    }

    /// The output prefix as the command names it, in `dir`.
    #[must_use]
    pub fn out_name(&self) -> String {
        relative(&self.out, &self.dir)
    }
}

/// `path` as a command run in `base` names it: relative when it is under
/// `base`, else whole.
#[must_use]
pub fn relative(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.to_string_lossy().into_owned(),
        _ => path.to_string_lossy().into_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Waiting,
    Running,
    Done,
    Failed(String),
    Stopped,
}

impl State {
    /// The mark a queue row starts with.
    #[must_use]
    pub fn mark(&self) -> &'static str {
        match self {
            State::Waiting => "·",
            State::Running => "▸",
            State::Done => "✓",
            State::Failed(_) => "✗",
            State::Stopped => "■",
        }
    }
}

/// Told each log line as it comes: a session job's reporter.
pub type Tap = Box<dyn Fn(&str) + Send>;

/// What the screen reads while the queue runs.
#[derive(Default)]
pub struct Shared {
    pub states: Vec<State>,
    /// When each job started, and how long it took once it ended.
    pub started: Vec<Option<Instant>>,
    pub took: Vec<Option<Duration>>,
    /// Each job's log, the last [`KEEP`] lines.
    pub logs: Vec<VecDeque<String>>,
    /// The last `info` line of each job that says what it is doing.
    pub doing: Vec<String>,
    /// Where each job's progress bar stands, while one is drawn.
    pub progress: Vec<Option<Progress>>,
    pub tap: Option<Tap>,
}

/// The queue, running.
pub struct Queue {
    pub jobs: Vec<Job>,
    pub shared: Arc<Mutex<Shared>>,
    stopper: Arc<Stopper>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Queue {
    /// Start `jobs` in order with `program` (cocoa itself), started as the
    /// session starts its children (`env`), each log line told to `tap`.
    pub fn start(
        jobs: Vec<Job>,
        program: PathBuf,
        env: legume_tui::ChildEnv,
        tap: Option<Tap>,
    ) -> Self {
        let n = jobs.len();
        let shared = Arc::new(Mutex::new(Shared {
            states: vec![State::Waiting; n],
            started: vec![None; n],
            took: vec![None; n],
            logs: vec![VecDeque::new(); n],
            doing: vec![String::new(); n],
            progress: vec![None; n],
            tap,
        }));
        let stopper: Arc<Stopper> = Arc::default();
        let worker = {
            let (s, st, js) = (shared.clone(), stopper.clone(), jobs.clone());
            std::thread::spawn(move || run_all(&js, &program, &env, &s, &st))
        };
        Queue {
            jobs,
            shared,
            stopper,
            worker: Mutex::new(Some(worker)),
        }
    }

    /// Interrupt the run going, as Ctrl+C would, and start none after it;
    /// asked again, kill it. Returns whether this stop kills.
    pub fn stop(&self) -> bool {
        self.stopper.stop()
    }

    /// Stop, give the run going `grace` to wrap up, then kill it and wait
    /// for the worker.
    pub fn stop_within(&self, grace: Duration) {
        self.stop();
        let start = Instant::now();
        while start.elapsed() < grace && !self.finished() {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !self.finished() {
            self.stopper.kill();
        }
        if let Some(w) = self.worker.lock().ok().and_then(|mut w| w.take()) {
            let _ = w.join();
        }
    }

    /// Whether every job has ended: the worker is done.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.worker
            .lock()
            .map_or(true, |w| w.as_ref().is_none_or(|h| h.is_finished()))
    }

    /// Each job's state now.
    #[must_use]
    pub fn states(&self) -> Vec<State> {
        self.shared
            .lock()
            .map(|s| s.states.clone())
            .unwrap_or_default()
    }

    /// Where the queue stands, in a line: `runs: 1 of 2 done`, with
    /// failures and stops when there are any.
    #[must_use]
    pub fn status(&self) -> String {
        status(&self.states(), self.finished())
    }
}

/// Where runs in `states` stand, in a line.
#[must_use]
pub fn status(states: &[State], finished: bool) -> String {
    let count = |f: fn(&State) -> bool| states.iter().filter(|s| f(s)).count();
    let done = count(|s| *s == State::Done);
    let failed = count(|s| matches!(s, State::Failed(_)));
    let stopped = count(|s| *s == State::Stopped);
    let mut line = format!(
        "runs {}: {done} of {} done",
        if finished { "finished" } else { "going" },
        states.len()
    );
    if failed > 0 {
        line += &format!(", {failed} failed");
    }
    if stopped > 0 {
        line += &format!(", {stopped} stopped");
    }
    line
}

fn run_all(
    js: &[Job],
    program: &Path,
    env: &legume_tui::ChildEnv,
    s: &Mutex<Shared>,
    st: &Stopper,
) {
    for (i, job) in js.iter().enumerate() {
        let before = i
            .checked_sub(1)
            .and_then(|k| s.lock().ok().map(|s| s.states[k].clone()));
        let state = if st.is_stopped() {
            State::Stopped
        } else if before.as_ref().is_some_and(|b| *b != State::Done) {
            State::Failed(format!("{} did not finish", js[i - 1].command))
        } else {
            if let Ok(mut s) = s.lock() {
                s.states[i] = State::Running;
                s.started[i] = Some(Instant::now());
            }
            run_job(job, i, program, env, s, st)
        };
        if let Ok(mut s) = s.lock() {
            s.took[i] = s.started[i].map(|t| t.elapsed());
            s.states[i] = state;
        }
    }
}

fn say(s: &Mutex<Shared>, i: usize, line: String) {
    if let Ok(mut s) = s.lock() {
        if let Some(doing) = doing(&line) {
            s.doing[i] = doing;
        }
        if let Some(tap) = &s.tap {
            tap(&line);
        }
        let log = &mut s.logs[i];
        if log.len() >= KEEP {
            log.pop_front();
        }
        log.push_back(line);
    }
}

/// What an `info` line of cocoa's says it is doing: the line without its
/// head, when it reads as a step (`Fitting the stage-1 baseline...`).
fn doing(line: &str) -> Option<String> {
    let text = line.rsplit_once("] ").map_or(line, |(_, t)| t).trim();
    let step = text.ends_with("...") || text.starts_with("Phase") || text.starts_with("Stage");
    step.then(|| text.trim_end_matches('.').to_string())
}

/// `f` with the script of `job`, and its command line: it runs from its
/// folder and refuses to run once the first output is there.
fn with_script<T>(job: &Job, f: impl FnOnce(&Script, &[String]) -> T) -> T {
    let made_by = format!("cocoa {}", job.command);
    let name = job
        .script()
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let guard = job.outputs.first().map(|o| relative(o, &job.dir));
    let s = Script {
        program: "cocoa",
        version: env!("CARGO_PKG_VERSION"),
        made_by: &made_by,
        name: &name,
        guard: guard.as_deref(),
        cd: true,
        env: &[("RUST_LOG", LOG_LEVEL)],
        head: if job.argv.get(1).is_some_and(|w| w == crate::runs::BATCH) {
            2
        } else {
            1
        },
    };
    f(&s, &job.argv)
}

fn run_job(
    job: &Job,
    i: usize,
    program: &Path,
    env: &legume_tui::ChildEnv,
    s: &Mutex<Shared>,
    stopper: &Stopper,
) -> State {
    if let Some(found) = job.outputs.iter().find(|p| p.exists()) {
        return State::Failed(format!("{} exists", found.display()));
    }
    if let Err(e) = std::fs::create_dir_all(&job.dir) {
        return State::Failed(format!("cannot make {}: {e}", job.dir.display()));
    }
    let path = job.script();
    if let Err(e) = with_script(job, |sc, argv| script::write(&path, sc, argv)) {
        return State::Failed(format!("cannot write the script: {e}"));
    }
    say(
        s,
        i,
        format!("── cocoa {} · {}", job.command, path.display()),
    );
    let mut command = std::process::Command::new(program);
    env.apply(&mut command, legume_tui::Group::Own);
    command.args(&job.argv).current_dir(&job.dir);
    // The log level the script sets, so the run is the one it records.
    if std::env::var_os("RUST_LOG").is_none() {
        command.env("RUST_LOG", LOG_LEVEL);
    }
    let each = |said| match said {
        Said::Line(line) => say(s, i, line),
        Said::Progress(p) => {
            if let Ok(mut s) = s.lock() {
                s.progress[i] = Some(p);
            }
        }
        Said::Cleared => {
            if let Ok(mut s) = s.lock() {
                s.progress[i] = None;
            }
        }
        _ => {}
    };
    let ran = run_one(command, stopper, each);
    if let Ok(mut s) = s.lock() {
        s.progress[i] = None;
    }
    match ran {
        Ok(()) => State::Done,
        Err(Failed::Stopped) => State::Stopped,
        Err(Failed::Start(why) | Failed::Exit(why)) => State::Failed(why),
        Err(other) => State::Failed(format!("{other:?}")),
    }
}

#[cfg(test)]
#[path = "tests/queue.rs"]
mod tests;
