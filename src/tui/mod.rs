//! The views of `cocoa simulate` and `cocoa diff`: set a run up, check the
//! exact command, run it with its log on screen, and read the result back.
//!
//! `cocoa simulate` picks a model and the strength of each edge of its DAG,
//! and by default queues a diff on what it simulates, scored against the
//! simulation's causal genes. `cocoa diff` picks the data and the files
//! that say who each cell is from and each individual's exposure. Both
//! check the command line with cocoa's own parser (as `cocoa mcp` does),
//! write each run's `{out}.cmd.sh` beside it, and run cocoa as a child, so
//! the run on screen is the one the script repeats.
//!
//! The views run in a detachable session (legume-tui): Ctrl+Alt+D leaves
//! the runs going, and `cocoa attach` comes back to them. With `--watch`
//! the view follows an agent's `cocoa mcp`: its calls and background jobs
//! on the Agent tab, and the run it works on in Results.

mod agent;
mod annotation;
mod draw;
mod figure;
mod gene_figure;
mod gene_view;
mod genome_view;
mod inputs;
mod keys;
mod logo;
mod queue;
mod results;
mod sim;

pub use sim::Model;

use crate::runs::{self, BATCH};
use inputs::{Inputs, Role};
use legume_tui::form::Form;
use legume_tui::save::Field;
use legume_tui::Round;
use queue::{Job, Queue, State};
use ratatui::crossterm::event::{Event, KeyEventKind};
use results::Results;
use sim::Sim;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// The tool cocoa's views belong to: their sessions are `cocoa-diff-1`,
/// `cocoa-simulate-2`, and `cocoa attach` finds them.
pub const TOOL: legume_tui::Tool = legume_tui::Tool::new("cocoa");

/// The ids of the view's own flags, which a run never takes: `cocoa mcp`
/// keeps them from agents.
pub const VIEW_IDS: &[&str] = &[
    "batch_process",
    "job",
    "watch",
    "results",
    "gene",
    "gtf",
    "no_detach",
    "session",
];

/// How long a run has to wrap up when the view quits under it.
const STOP_GRACE: Duration = Duration::from_secs(10);

/// The view's flags on `cocoa diff`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct ViewArgs {
    #[arg(
        long,
        help = "Run straight through, without the view",
        long_help = "Run straight through, without the view, even with --results or --watch.\n\
                     A complete command line, or one without a terminal (a pipe, a job\n\
                     scheduler), runs straight through anyway."
    )]
    pub batch_process: bool,

    /// Run as `cocoa mcp`'s session job: the run starts at once, its log
    /// goes to the job, and the view ends with it.
    #[arg(long, hide = true)]
    pub job: bool,

    #[arg(
        long,
        value_name = "PID",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "",
        help = "Follow an agent's `cocoa mcp`: its calls, and its latest run in Results",
        long_help = "Follow an agent's `cocoa mcp` (the newest, or --watch=PID): its calls and\n\
                     background jobs on the Agent tab, where s stops one, and the run it\n\
                     works on in Results."
    )]
    pub watch: Option<String>,

    #[arg(
        long,
        value_name = "PREFIX",
        help = "Open the view on the results of a finished diff"
    )]
    pub results: Option<String>,

    #[arg(
        long,
        value_name = "NAME",
        requires = "results",
        help = "Open the results on this gene's page (a symbol or the data's name)"
    )]
    pub gene: Option<String>,

    #[arg(
        long,
        value_name = "FILE",
        help = "Annotation (GTF/GFF) that places genes on the genome view",
        long_help = "Annotation (GTF/GFF) that places genes on the Results tab's genome view.\n\
                     Without it, the one picked last; g in the view asks faba to download one\n\
                     (`faba reference`) into its reference cache, outside any project."
    )]
    pub gtf: Option<PathBuf>,

    #[command(flatten)]
    pub session: legume_tui::cli::SessionArgs,
}

/// `cocoa simulate`'s arguments.
#[derive(clap::Args, Debug, Clone)]
pub struct SimulateArgs {
    #[arg(
        long,
        value_enum,
        default_value = "collider",
        help = "The model it starts on"
    )]
    pub model: Model,

    #[arg(help = "Folder the outputs go under [default: ./sim]")]
    pub dir: Option<PathBuf>,

    #[command(flatten)]
    pub session: legume_tui::cli::SessionArgs,
}

/// Whether there is a terminal to show a view in.
#[must_use]
pub fn terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// The screens, in tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Model,
    Inputs,
    Flags,
    Run,
    Results,
    Agent,
}

impl Page {
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Page::Model => "Model",
            Page::Inputs => "Inputs",
            Page::Flags => "Flags",
            Page::Run => "Run",
            Page::Results => "Results",
            Page::Agent => "Agent",
        }
    }
}

/// Which command's form a flag row is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Of {
    Sim,
    Diff,
}

/// What a line being typed will become.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    SimOut,
    DiffOut,
    /// A row of the Model tab.
    Model(usize),
    /// A flag, by form and index.
    Flag(Of, usize),
    /// The flags containing it.
    Find,
}

/// The Flags tab's cursor and filter.
#[derive(Default)]
pub struct Flags {
    pub at: usize,
    pub find: String,
    pub advanced: bool,
}

/// A row of the Flags tab: a form's heading, or one of its flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlagRow {
    Heading(String),
    Flag(Of, usize),
}

/// The view.
pub struct App {
    pub cli: clap::Command,
    pub here: PathBuf,
    program: PathBuf,
    pub page: Page,
    /// The simulation, in `cocoa simulate`.
    pub sim: Option<Sim>,
    pub inputs: Inputs,
    pub diff: Form,
    pub flags: Flags,
    /// The output prefixes as typed; empty for the next free one.
    pub sim_out: String,
    pub diff_out: String,
    /// Where a simulation's outputs go when its prefix is not typed.
    sim_dir: PathBuf,
    pub editing: Option<(Target, Field)>,
    /// The runs the start would queue, or why it cannot: worked out after
    /// each key, as clap checks every command line.
    pub planned: Result<Vec<Job>, String>,
    pub preview: bool,
    pub preview_scroll: usize,
    pub queue: Option<Queue>,
    /// The job the Run tab shows, and how far its log is scrolled up.
    pub run_at: usize,
    pub log_scroll: usize,
    /// The queue's end has been told (the pop-up shown and closed).
    pub told: bool,
    pub ended: bool,
    pub results: Option<Results>,
    pub results_error: Option<String>,
    /// A run's results being read, off the screen's thread.
    pub opening: Option<(PathBuf, std::sync::mpsc::Receiver<anyhow::Result<Results>>)>,
    pub show_heat: bool,
    pub agent: Option<agent::Agent>,
    /// The genome view of Results.
    pub gv: genome_view::GenomeView,
    /// A gene's page, over the table or the genome view it was opened from.
    pub gene: Option<gene_view::GeneView>,
    /// The gene finder (`/`), narrowed as a name is typed.
    pub finder: Option<legume_tui::browse::FindList<String>>,
    /// The annotation given with `--gtf`.
    gtf: Option<PathBuf>,
    /// How pictures reach this terminal, once the screen is up.
    pub picker: Option<ratatui_image::picker::Picker>,
    pub checks: Vec<String>,
    checking: Option<Receiver<Vec<String>>>,
    checked_for: Option<(Vec<PathBuf>, Option<PathBuf>)>,
    pub message: Option<String>,
    pub help: bool,
    /// q pressed once while runs go: asking.
    pub asking_quit: bool,
    pub quit: bool,
}

impl App {
    fn new(
        cli: clap::Command,
        here: PathBuf,
        sim: Option<Sim>,
        sim_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let diff_cmd = runs::subcommand(&cli, "diff")?;
        let diff = Form::new(diff_cmd, |a| {
            let id = a.get_id().as_str();
            a.get_long().is_some() && !inputs::OWNED.contains(&id) && !VIEW_IDS.contains(&id)
        });
        let page = if sim.is_some() {
            Page::Model
        } else {
            Page::Inputs
        };
        let mut app = App {
            program: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cocoa")),
            inputs: Inputs::new(here.clone()),
            cli,
            here,
            page,
            sim,
            diff,
            flags: Flags::default(),
            sim_out: String::new(),
            diff_out: String::new(),
            sim_dir,
            editing: None,
            planned: Err(String::new()),
            preview: false,
            preview_scroll: 0,
            queue: None,
            run_at: 0,
            log_scroll: 0,
            told: false,
            ended: false,
            results: None,
            results_error: None,
            opening: None,
            show_heat: true,
            agent: None,
            gv: genome_view::GenomeView::default(),
            gene: None,
            finder: None,
            gtf: None,
            picker: None,
            checks: Vec::new(),
            checking: None,
            checked_for: None,
            message: None,
            help: false,
            asking_quit: false,
            quit: false,
        };
        app.replan();
        Ok(app)
    }

    /// The tabs there are now: Run once a queue started, Results once a
    /// diff can be read, Agent while one is followed.
    #[must_use]
    pub fn pages(&self) -> Vec<Page> {
        let mut p = vec![if self.sim.is_some() {
            Page::Model
        } else {
            Page::Inputs
        }];
        p.push(Page::Flags);
        if self.queue.is_some() {
            p.push(Page::Run);
        }
        if self.results.is_some() || self.results_error.is_some() || self.opening.is_some() {
            p.push(Page::Results);
        }
        if self.agent.is_some() {
            p.push(Page::Agent);
        }
        p
    }

    #[must_use]
    pub fn running(&self) -> bool {
        self.queue.as_ref().is_some_and(|q| !q.finished())
    }

    /// The Flags tab's rows: each command's heading, then its flags that
    /// match the filter (the advanced ones only when asked).
    #[must_use]
    pub fn flag_rows(&self) -> Vec<FlagRow> {
        let mut rows = Vec::new();
        let find = self.flags.find.to_lowercase();
        let mut add = |of: Of, title: String, form: &Form| {
            let shown: Vec<usize> = form
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| self.flags.advanced || !f.advanced)
                .filter(|(_, f)| find.is_empty() || f.long.contains(&find))
                .map(|(i, _)| i)
                .collect();
            if !shown.is_empty() {
                rows.push(FlagRow::Heading(title));
                rows.extend(shown.into_iter().map(|i| FlagRow::Flag(of, i)));
            }
        };
        if let Some(s) = &self.sim {
            add(Of::Sim, format!("cocoa {}", s.model.command()), s.form());
        }
        if self.sim.as_ref().is_none_or(|s| s.then_diff) {
            add(Of::Diff, "cocoa diff".into(), &self.diff);
        }
        rows
    }

    pub fn form(&self, of: Of) -> &Form {
        match (of, &self.sim) {
            (Of::Sim, Some(s)) => s.form(),
            _ => &self.diff,
        }
    }

    pub fn form_mut(&mut self, of: Of) -> &mut Form {
        match (of, &mut self.sim) {
            (Of::Sim, Some(s)) => s.form_mut(),
            _ => &mut self.diff,
        }
    }

    /// A prefix as typed, absolute.
    fn typed_out(&self, text: &str) -> PathBuf {
        let t = text.trim();
        if t.starts_with('~') {
            legume_tui::browse::typed_path(t)
        } else {
            legume_tui::browse::normalize(&self.here.join(t))
        }
    }

    /// A job for the command line `argv` names (the command first, no
    /// output; its paths named from a folder, or whole for none), into
    /// `out` as typed, else the next free `{dir}/{stem}`; refused as `cocoa
    /// mcp` refuses a run: when clap does, or a run is there. It runs in
    /// its output's folder, its paths named from there.
    fn job(
        &self,
        argv: impl Fn(Option<&Path>) -> Vec<String>,
        typed: &str,
        dir: &Path,
        stem: &str,
    ) -> Result<Job, String> {
        let whole = argv(None);
        let out = if typed.trim().is_empty() {
            PathBuf::from(runs::free_prefix(&self.cli, &whole, dir, stem))
        } else {
            self.typed_out(typed)
        };
        let out_s = out.to_string_lossy().into_owned();
        let outputs = runs::plan(&self.cli, &whole, &out_s).map_err(|e| e.to_string())?;
        if let Some(found) = runs::taken(&out, &outputs) {
            return Err(format!(
                "{} exists: a run is never written over; name another output",
                legume_tui::browse::tilde(&found)
            ));
        }
        let dir = out
            .parent()
            .map_or_else(|| self.here.clone(), Path::to_path_buf);
        let mut job = Job {
            command: whole[0].clone(),
            argv: argv(Some(&dir)),
            dir,
            out,
            outputs,
        };
        let name = job.out_name();
        job.argv.extend(["--output".to_string(), name]);
        Ok(job)
    }

    /// The simulation's run.
    fn sim_job(&self, sim: &Sim) -> Result<Job, String> {
        let mut argv = vec![sim.model.command().to_string()];
        argv.extend(sim.form().argv());
        self.job(
            |_| argv.clone(),
            &self.sim_out,
            &self.sim_dir,
            sim.model.name(),
        )
    }

    /// The diff of the inputs picked, or of the simulation `on`'s outputs.
    fn diff_job(&self, on: Option<(&Sim, &Job)>) -> Result<Job, String> {
        let (picked, dir, stem): (Vec<(PathBuf, Role)>, PathBuf, String) = match on {
            Some((sim, j)) => {
                let picked = sim_inputs(sim.model, j);
                let stem = format!(
                    "{}-diff",
                    j.out
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                );
                (picked, j.dir.clone(), stem)
            }
            None => {
                let missing = self.inputs.missing();
                if !missing.is_empty() {
                    return Err(format!("pick the {} on the Inputs tab", missing.join(", ")));
                }
                let data = self.inputs.files(Role::Data)[0];
                let dir = data
                    .parent()
                    .map_or_else(|| self.here.clone(), Path::to_path_buf);
                (
                    self.inputs.picked.clone(),
                    dir,
                    format!("{}-diff", data_stem(data)),
                )
            }
        };
        let argv = |from: Option<&Path>| {
            let mut argv = vec!["diff".to_string(), BATCH.to_string()];
            argv.extend(inputs::argv(&picked, from));
            argv.extend(self.flags_from(from));
            argv
        };
        self.job(argv, &self.diff_out, &dir, &stem)
    }

    /// The diff flags changed on the form, a file in one given as the run
    /// started in `from` finds it (typed, it is from where cocoa started).
    fn flags_from(&self, from: Option<&Path>) -> Vec<String> {
        let Some(from) = from else {
            return self.diff.argv();
        };
        let is_file = |id: &str| {
            runs::subcommand(&self.cli, "diff").is_ok_and(|c| {
                c.get_arguments()
                    .any(|a| a.get_id() == id && a.get_value_hint() == clap::ValueHint::FilePath)
            })
        };
        let mut form = self.diff.clone();
        for f in form
            .fields
            .iter_mut()
            .filter(|f| !f.is_default() && is_file(&f.id))
        {
            f.value = f
                .value
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|v| !v.is_empty())
                .map(|v| queue::relative(&legume_tui::browse::normalize(&self.here.join(v)), from))
                .collect::<Vec<_>>()
                .join(",");
        }
        form.argv()
    }

    /// The runs a start queues now, or why it cannot.
    fn jobs(&self) -> Result<Vec<Job>, String> {
        match &self.sim {
            Some(sim) => {
                let s = self.sim_job(sim)?;
                let mut jobs = vec![s];
                if sim.then_diff {
                    let d = self.diff_job(Some((sim, &jobs[0])))?;
                    jobs.push(d);
                }
                Ok(jobs)
            }
            None => Ok(vec![self.diff_job(None)?]),
        }
    }

    /// Work the runs out again, and start checking the inputs if they
    /// changed.
    pub fn replan(&mut self) {
        self.planned = self.jobs();
        let indv: Vec<PathBuf> = self
            .inputs
            .files(Role::Individuals)
            .into_iter()
            .cloned()
            .collect();
        let exposure = self
            .inputs
            .files(Role::Exposure)
            .first()
            .map(|p| (*p).clone());
        let wanted = (indv, exposure);
        if self.sim.is_none() && self.checked_for.as_ref() != Some(&wanted) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (i, e) = wanted.clone();
            std::thread::spawn(move || {
                let _ = tx.send(inputs::checks(&i, e.as_ref()));
            });
            self.checking = Some(rx);
            self.checked_for = Some(wanted);
        }
    }

    /// The truth to score a diff against: the simulation's causal genes,
    /// or the file picked as truth.
    fn truth_for(&self) -> Option<PathBuf> {
        if let Some(t) = self.inputs.truth() {
            return Some(t.clone());
        }
        Some(sim_truth(self.sim_run()?)).filter(|p| p.is_file())
    }

    /// The simulation the queue ran, if it ran one.
    fn sim_run(&self) -> Option<&Job> {
        self.queue
            .as_ref()?
            .jobs
            .iter()
            .find(|j| j.command.starts_with("simulate"))
    }

    /// Read the diff at `prefix` into Results.
    pub fn open_results(&mut self, prefix: &Path, truth: Option<PathBuf>) {
        let truth = self.truth_of(prefix, truth);
        let (tx, rx) = std::sync::mpsc::channel();
        let p = prefix.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(Results::read(&p, truth.as_deref()));
        });
        self.opening = Some((prefix.to_path_buf(), rx));
    }

    /// The truth to score `prefix` against: `truth`, else the inputs', else
    /// one beside a simulation's outputs.
    fn truth_of(&self, prefix: &Path, truth: Option<PathBuf>) -> Option<PathBuf> {
        truth.or_else(|| {
            let t = self.inputs.truth().cloned();
            t.or_else(|| guess_truth(prefix))
        })
    }

    /// The results of `prefix` read, or why they could not be.
    fn take_results(&mut self, prefix: &Path, read: anyhow::Result<Results>) {
        self.opening = None;
        match read {
            Ok(r) => {
                self.gv.opened(&r, self.gtf.clone());
                // Another run: its genes are not the page's.
                self.gene = None;
                self.results = Some(r);
                self.results_error = None;
            }
            Err(e) => {
                self.results = None;
                self.results_error = Some(format!("{}: {e:#}", legume_tui::browse::tilde(prefix)));
            }
        }
    }

    /// Start the runs planned, each log line told to `tap`.
    pub fn start(&mut self, session: &legume_tui::Session, tap: Option<queue::Tap>) {
        let jobs = match &self.planned {
            Ok(j) => j.clone(),
            Err(why) => {
                self.message = Some(why.clone());
                return;
            }
        };
        self.preview = false;
        self.queue = Some(Queue::start(
            jobs,
            self.program.clone(),
            session.child_env(),
            tap,
        ));
        self.run_at = 0;
        self.log_scroll = 0;
        self.told = false;
        self.ended = false;
        self.page = Page::Run;
    }

    /// `diff on this`: the finished simulation's outputs as the inputs of a
    /// diff, set up on the Inputs tab.
    pub fn diff_on_sim(&mut self) {
        let Some(j) = self.sim_run().cloned() else {
            return;
        };
        let model = self.sim.as_ref().map_or(Model::Collider, |s| s.model);
        let mut inputs = Inputs::new(j.dir.clone());
        for (f, role) in sim_inputs(model, &j) {
            inputs.set(f, Some(role));
        }
        inputs.set(sim_truth(&j), Some(Role::Truth));
        inputs.browser.refresh();
        self.inputs = inputs;
        self.sim = None;
        self.diff_out.clear();
        self.page = Page::Inputs;
        self.replan();
    }

    /// Take in what changed on its own: the checks, the queue's end, the
    /// agent's run. Returns whether anything did.
    fn tick(&mut self) -> bool {
        let mut changed = false;
        if let Some((prefix, rx)) = &self.opening {
            if let Ok(read) = rx.try_recv() {
                let prefix = prefix.clone();
                self.take_results(&prefix, read);
                changed = true;
            }
        }
        if let Some(rx) = &self.checking {
            if let Ok(c) = rx.try_recv() {
                self.checks = c;
                self.checking = None;
                changed = true;
            }
        }
        if !self.ended && self.queue.as_ref().is_some_and(Queue::finished) {
            self.ended = true;
            changed = true;
            let q = self.queue.as_ref().expect("a queue ended");
            let states = q.states();
            let diff = q
                .jobs
                .iter()
                .zip(&states)
                .rev()
                .find(|(j, s)| j.command == "diff" && **s == State::Done)
                .map(|(j, _)| j.clone());
            if let Some(j) = diff {
                let truth = self.truth_for();
                self.open_results(&j.out, truth);
            }
            // Planned again: the outputs are taken now.
            self.replan();
        }
        let placed = self.gv.tick(self.results.as_ref());
        changed |= placed;
        if let Some(g) = &mut self.gene {
            // The locus follows the annotation.
            if placed {
                g.redraw();
            }
            changed |= g.tick();
        }
        if let Some(a) = &mut self.agent {
            changed |= a.pane.poll();
            if let Some(out) = a.next_run(results::is_diff) {
                self.open_results(&out, None);
                changed = true;
            }
        }
        changed
    }

    /// One line for `cocoa sessions` and the resume picker.
    fn status(&self) -> String {
        match &self.queue {
            Some(q) => q.status(),
            None if self.sim.is_some() => "setting up a simulation".into(),
            None => "setting up a diff".into(),
        }
    }

    /// What the view leaves as it ends: each run and where it went.
    fn summary(&self) -> Vec<String> {
        let Some(q) = &self.queue else {
            return Vec::new();
        };
        let mut lines = vec![format!("cocoa: {}", q.status())];
        for (j, s) in q.jobs.iter().zip(q.states()) {
            let why = match &s {
                State::Failed(w) => format!(": {w}"),
                _ => String::new(),
            };
            lines.push(format!(
                "  {} cocoa {} → {}{why}",
                s.mark(),
                j.command,
                queue::relative(&j.out, &self.here)
            ));
        }
        lines
    }
}

/// A data file's name without its format: `cells` of `cells.zarr.zip`.
fn data_stem(p: &Path) -> String {
    legume_tui::save::stem_of(&draw::name(p), &["zarr.zip", "zarr", "h5ad", "h5"])
}

/// A simulation's truth beside a diff's inputs, when the diff's prefix is
/// `{sim}-diff` (or the next free `{sim}-diff2`): `{sim}.causal.gz`.
fn guess_truth(prefix: &Path) -> Option<PathBuf> {
    let name = prefix.file_name()?.to_string_lossy();
    let sim = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .strip_suffix("-diff")?;
    let t = prefix.with_file_name(format!("{sim}.causal.gz"));
    t.is_file().then_some(t)
}

/// `cocoa diff`: the view, prefilled from the command line `m` (parsed
/// with its required arguments let go).
pub fn diff(m: &clap::ArgMatches, view: &ViewArgs) -> anyhow::Result<()> {
    let target = view.results.is_some() || view.watch.is_some();
    let session = TOOL.session("diff", &view.session.options(target))?;
    let cli = runs::cocoa_cli();
    let here = std::env::current_dir()?;
    let mut app = App::new(cli, here.clone(), None, here.join("sim"))?;
    app.gtf = view
        .gtf
        .as_ref()
        .map(|g| legume_tui::browse::normalize(&here.join(g)));
    app.inputs.prefill(m, &here);
    app.diff.prefill(m);
    if let Some(out) = legume_tui::form::given_words(m, "output").and_then(|w| w.into_iter().next())
    {
        app.diff_out = out;
    }
    app.inputs.browser.refresh();
    app.replan();
    if let Some(prefix) = &view.results {
        let prefix = app.typed_out(prefix);
        // Before the screen is up: read here, for the gene asked for.
        let truth = app.truth_of(&prefix, None);
        app.take_results(&prefix, Results::read(&prefix, truth.as_deref()));
        app.page = Page::Results;
        if let (Some(name), Some(r)) = (&view.gene, &app.results) {
            let g = gene_view::GeneView::find(r, name)
                .ok_or_else(|| anyhow::anyhow!("no gene {name} in {}", prefix.display()))?;
            app.gene = Some(gene_view::GeneView::open(r, g));
        }
    }
    if let Some(which) = &view.watch {
        app.agent = Some(agent::Agent::find(which)?);
        app.page = Page::Agent;
    }
    if view.job {
        return job(&session, app);
    }
    run(&session, &mut app, false)
}

/// The inputs of a diff on simulation `j` of `model`: its counts, then the
/// files beside them.
fn sim_inputs(model: Model, j: &Job) -> Vec<(PathBuf, Role)> {
    let roles = [Role::Individuals, Role::Exposure, Role::Topics];
    let beside = model.beside().iter().zip(roles);
    std::iter::once((j.outputs[0].clone(), Role::Data))
        .chain(beside.map(|(b, role)| (runs::suffixed(&j.out, &format!(".{b}")), role)))
        .collect()
}

/// The causal genes simulation `j` planted.
fn sim_truth(j: &Job) -> PathBuf {
    runs::suffixed(&j.out, ".causal.gz")
}

/// The diff as `cocoa mcp`'s session job: started at once, its log lines
/// told to the job, the view ending with the run, and the job told how.
fn job(session: &legume_tui::Session, mut app: App) -> anyhow::Result<()> {
    let reporter = legume_mcp::job::Reporter::from_env().map(std::sync::Arc::new);
    let tap = reporter
        .clone()
        .map(|r| -> queue::Tap { Box::new(move |line| r.line(line)) });
    app.start(session, tap);
    // A run that cannot start ends the job now, saying why.
    let result = match (&app.queue, &app.planned) {
        (None, Err(why)) => Err(anyhow::anyhow!("{why}")),
        _ => run(session, &mut app, true),
    };
    if let Some(r) = &reporter {
        let state = app.queue.as_ref().and_then(|q| q.states().pop());
        r.finish(job_end(&result, state, &app.diff_out));
    }
    result
}

/// How a session job ended: the view's error, else its run's last state.
fn job_end(result: &anyhow::Result<()>, state: Option<State>, out: &str) -> legume_mcp::job::End {
    use legume_mcp::job::End;
    match (result, state) {
        (Err(e), _) => End::Failed(format!("{e:#}")),
        (Ok(()), Some(State::Done)) => End::Done(format!("cocoa diff to {out}")),
        (Ok(()), Some(State::Failed(why))) => End::Failed(why),
        (Ok(()), _) => End::Cancelled,
    }
}

/// `cocoa simulate`: the view on `model`, its outputs under `dir`.
pub fn simulate(args: &SimulateArgs) -> anyhow::Result<()> {
    anyhow::ensure!(
        terminal(),
        "cocoa simulate is a view: without a terminal, run simulate-one or simulate-collider"
    );
    let session = TOOL.session("simulate", &args.session.options(args.dir.is_some()))?;
    let cli = runs::cocoa_cli();
    let here = std::env::current_dir()?;
    let dir =
        legume_tui::browse::normalize(&here.join(args.dir.clone().unwrap_or_else(|| "sim".into())));
    let sim = Sim::new(&cli, args.model)?;
    let mut app = App::new(cli, here, Some(sim), dir)?;
    run(&session, &mut app, false)
}

/// Run the view to its end in `session`, `app` left as it ended; with
/// `with_runs`, it ends as its runs do (a session job's).
fn run(session: &legume_tui::Session, app: &mut App, with_runs: bool) -> anyhow::Result<()> {
    use legume_tui::graphics::{Fallback, Graphics};
    let quiet = legume_tui::logs::quiet();
    let mut terminal = session.terminal()?;
    let result = (|| -> anyhow::Result<()> {
        // The terminal is fresh: its answer about pictures is this one's.
        session.take_repaint();
        app.picker = Some(legume_tui::graphics::pick(
            session,
            Graphics::Auto,
            Fallback::Probe,
        ));
        let mut dirty = true;
        while !app.quit {
            dirty |= app.tick();
            if with_runs && app.queue.as_ref().is_some_and(Queue::finished) {
                break;
            }
            let busy = app.running()
                || app.checking.is_some()
                || app.opening.is_some()
                || app.gv.busy()
                || app.gene.as_ref().is_some_and(gene_view::GeneView::busy);
            // Detached, nothing is drawn; the runs are followed all the same.
            if dirty || busy {
                session.draw(&mut terminal, |f| app.draw(f))?;
                dirty = false;
            }
            session.status(&app.status());
            // An agent followed is polled often, and drawn when it moved.
            let wanted = if busy || app.agent.is_some() {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(60)
            };
            match session.poll(&mut terminal, wanted)? {
                Round::Event(Event::Key(k)) if k.kind != KeyEventKind::Release => {
                    app.key(k, session);
                    dirty = true;
                }
                // Another terminal may be attached now: ask it about
                // pictures, and draw the figure for it.
                Round::Repainted => {
                    app.picker = Some(legume_tui::graphics::pick(
                        session,
                        Graphics::Auto,
                        Fallback::Probe,
                    ));
                    app.gv.redraw();
                    if let Some(g) = &mut app.gene {
                        g.redraw();
                    }
                    dirty = true;
                }
                Round::Event(_) => dirty = true,
                // `cocoa kill` quits as Ctrl+C does: the runs are stopped
                // and wrapped up below.
                Round::Stopped => app.quit = true,
                _ => {}
            }
        }
        Ok(())
    })();
    if app.running() {
        app.message = Some("stopping the runs…".into());
        let _ = session.draw(&mut terminal, |f| app.draw(f));
    }
    if let Some(q) = &app.queue {
        q.stop_within(STOP_GRACE);
    }
    if let Err(e) = &result {
        let _ = session.hold_error(&mut terminal, e);
    }
    session.restore();
    drop(quiet);
    result?;
    let summary = app.summary().join("\n");
    if !summary.is_empty() {
        if session.is_hosted() {
            session.leave(&summary);
        } else {
            println!("{summary}");
        }
    }
    Ok(())
}

/// `cocoa mcp watch`: legume-mcp's screen of an agent's `cocoa mcp`, where
/// o opens a diff's results in `cocoa diff --results`.
pub fn watch(pid: Option<u32>) -> anyhow::Result<()> {
    anyhow::ensure!(terminal(), "cocoa mcp watch is a view: it needs a terminal");
    let dir = match pid {
        None => None,
        Some(pid) => Some(
            legume_mcp::watch::find(crate::mcp::SERVER, Some(pid))
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no running `cocoa mcp` with pid {pid}"))?
                .dir,
        ),
    };
    legume_mcp::watch::view::run(&TOOL, dir, true, |call| {
        let out = agent::call_run(call).filter(|o| results::is_diff(o))?;
        let mut cocoa = legume_mcp::own_command(["diff", "--results"]).ok()?;
        cocoa.arg(out);
        Some(cocoa)
    })
}

#[cfg(test)]
#[path = "tests/app.rs"]
mod tests;
