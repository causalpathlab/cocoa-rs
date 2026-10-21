mod collapse_cocoa_data;
mod common;
mod input;
#[cfg(test)]
mod main_tests;
mod mcp;
mod randomly_partition_data;
mod run_collapse;
mod run_diff;
mod run_sim_collider;
mod run_sim_one_type;
mod runs;
mod stat;
mod tui;

use crate::run_collapse::*;
use crate::run_diff::*;
use crate::run_sim_collider::*;
use crate::run_sim_one_type::*;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "CoCoA — Counterfactual Confounder Adjustment for single-cell DE",
    long_about = "CoCoA implements counterfactual confounder adjustment.\n\
                  It targets single-cell pseudobulk differential expression,\n\
                  after Park & Kellis, 2021.\n\
                  \n\
                  It pairs individual-level matching with topic-aware pseudobulk collapsing.\n\
                  That breaks collider bias.\n\
                  The bias arises when cell type is a downstream effect of both exposure and an unmeasured confounder.\n\
                  \n\
                  Subcommands:\n  \
                  \x20 collapse           pseudobulk SC counts per (individual, topic)\n  \
                  \x20 diff               differential expression, confounder-adjusted (a view in a terminal)\n  \
                  \x20 simulate           a view: pick a DAG and its strengths, simulate, then diff\n  \
                  \x20 simulate-one       one cell type, with W → X → Y\n  \
                  \x20 simulate-collider  many cell types, with an X → A ← U collider\n  \
                  \x20 mcp                serve these to an agent (Model Context Protocol)\n  \
                  \x20 attach, sessions   come back to a detached view, list them"
)]
struct Cli {
    #[arg(short = 'v', long, global = true, help = "Increase output verbosity")]
    verbose: bool,

    #[arg(
        long = "n-threads",
        visible_aliases = ["threads", "num-threads"],
        global = true,
        value_name = "N",
        help = "Limit the number of CPU threads (rayon global pool)"
    )]
    n_threads: Option<usize>,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(
        about = "Differential expression analysis with pseudobulk",
        long_about = "Differential expression on pseudobulk data, confounder-adjusted.\n\
                      \n\
                      Pipeline:\n  \
                      (1) residualize soft topic weights vs exposure (collider; soft -r needed),\n  \
                      (2) label-free pseudobulks across individuals, refined by likelihood,\n  \
                      (3) stage 1: rank-1 baseline mu(p) * omega(i) per gene and topic,\n  \
                      (4) stage 2: doubly robust g-estimation of the exposure effect,\n  \
                      \x20   a Gamma GLM on omega with the propensity as covariate.\n\
                      Writes {out}.effect, .delta, .contrast, .stage1 and, with\n\
                      --n-permutations, .perm (conditional-permutation z, p).\n\
                      \n\
                      A complete command line runs straight through. In a terminal, one\n\
                      set up only in part (or with --results or --watch) opens a view with\n\
                      what it gives: pick the files and their roles, change flags, preview\n\
                      the exact command, run it with its log on screen (its {out}.cmd.sh\n\
                      beside it), and read the results back.\n\
                      \n\
                      References:\n  \
                      Park & Kellis (2021) Genome Biol: CoCoA-diff framework\n  \
                      Dukes & Vansteelandt (2018) Am J Epidemiol: g-estimation of risk ratios\n  \
                      Hartwig et al. (2023) Eur J Epidemiol: residual collider stratification",
        after_long_help = session_help("the run goes on")
    )]
    Diff {
        #[command(flatten)]
        args: Box<DiffArgs>,
        #[command(flatten)]
        view: tui::ViewArgs,
    },

    #[command(
        about = "A view: pick a model and its edges' strengths, simulate, then diff on it",
        long_about = "A view of the two simulators: pick the model (one cell type, or a\n\
                      collider across cell types), its sizes and the PVE of each edge of its\n\
                      DAG, and every other flag; preview the exact commands, and run the\n\
                      simulation, by default with a diff on it after. The diff's results are\n\
                      scored against the simulation's causal genes. Each run's {out}.cmd.sh\n\
                      is written beside it. simulate-one and simulate-collider run\n\
                      straight through.",
        after_long_help = session_help("the runs go on")
    )]
    Simulate(tui::SimulateArgs),

    #[command(
        about = "Collapse single-cell counts into pseudobulk per (individual, topic)",
        alias = "pseudobulk"
    )]
    Collapse(CollapseArgs),

    #[command(
        about = "Simulate single-cell counts under one cell type with a W-confounded exposure",
        long_about = "Simulate single-cell counts under a single cell type with a confounded exposure.\n\
                      \n\
                      Causal DAG:\n\
                      \n    \
                      W ──→ X ──→ Y\n    \
                      └──────────↗\n\
                      \n  \
                      Edges: W→X, W→Y, X→Y (causal, causal genes only)\n\
                      \n  \
                      W is a confounder.\n  \
                      It affects both exposure assignment X and gene expression Y.\n  \
                      That creates a spurious X–Y association, even for non-causal genes.\n\
                      \n\
                      Generative model:\n\
                      \n  \
                      Phase 1 — individual level (i = 1..N):\n    \
                      W_i  ~ N(0, I)                        [--n-covariates dims]\n    \
                      X_i  ~ Cat(softmax(√pve_wx · W·α + √(1-pve_wx) · ε))\n                                           \
                      [--n-exposure categories]\n\
                      \n  \
                      Phase 2 — individual-level gene expression (gene g = 1..G):\n    \
                      log μ_ig = √pve_xy · β_g·X̃_i         (causal, causal genes only)\n             \
                      + √pve_wy · W_i·γ_g          (confounding)\n             \
                      + √pve_res · ε_g             (noise)\n\
                      \n  \
                      Phase 3 — cell-level counts (cell j in individual i):\n    \
                      Y_ijg ~ Poisson(ρ_j · exp(log μ_ig))\n    \
                      ρ_j   ~ Gamma(shape, rate)            [--gamma-hyperparam]\n\
                      \n\
                      PVE parameters control signal strength at each DAG edge."
    )]
    SimulateOne(SimOneTypeArgs),

    #[command(
        about = "Simulate single-cell counts with collider bias (X → A ← U) across cell types",
        long_about = "Simulate single-cell counts under a collider bias model with multiple cell types.\n\
                      \n\
                      Causal DAG:\n\
                      \n    \
                      ┌──→ A ←───┐\n    \
                      │          │\n    \
                      X ──→ Y ←─ U\n    \
                      ↑     ↑\n    \
                      └─ V ─┘\n\
                      \n  \
                      Edges: V→X, V→Y, X→Y (causal), X→A, U→A, U→Y\n\
                      \n  \
                      A is a collider: X → A ← U.\n  \
                      Conditioning on cell type A opens the path X → A ← U → Y.\n  \
                      That induces a non-causal X–Y association, through U.\n\
                      \n\
                      Generative model:\n\
                      \n  \
                      Phase 1 — individual level (i = 1..N):\n    \
                      V_i  ~ N(0, I)                        [--n-covariates dims]\n    \
                      X_i  ~ Cat(softmax(√pve_vx · V·α + √(1-pve_vx) · ε))\n                                           \
                      [--n-exposure categories]\n\
                      \n  \
                      Phase 2 — cell level (j = 1..n_j for individual i):\n    \
                      U_j  ~ N(0, I)                        [--n-cell-covariates dims]\n    \
                      A_ij ~ Cat(softmax(√pve_ua · U·δ + √pve_xa · X·η + √pve_res · ε))\n           \
                      where A is the collider variable [--n-cell-types categories]\n\
                      \n  \
                      Phase 3 — gene expression (gene g = 1..G):\n    \
                      log μ_ijg = √pve_xy · β_g·X̃_i        (causal, causal genes only)\n              \
                      + √pve_vy · V_i·γ_g         (individual confounding)\n              \
                      + √pve_uy · U_j·ξ_g         (cell confounding)\n              \
                      + √pve_res · ε_g            (noise)\n    \
                      Y_ijg ~ Poisson(ρ_j · exp(log μ))\n    \
                      ρ_j   ~ Gamma(shape, rate)            [--gamma-hyperparam]\n\
                      \n\
                      PVE parameters control signal strength at each DAG edge.\n\
                      \n\
                      References:\n  \
                      Cole et al. (2010) Int J Epidemiol — collider bias via DAGs\n  \
                      Davey Smith & Munafò (2019) Int J Epidemiol — selection bias severity\n  \
                      Akimova et al. (2021) Sci Rep — continuous conditioning on a collider\n  \
                      Hartwig et al. (2023) Eur J Epidemiol — residual collider stratification"
    )]
    SimulateCollider(SimColliderArgs),

    #[command(
        about = "Serve cocoa's commands to an agent over the Model Context Protocol (stdio)",
        long_about = "Serve cocoa's commands to an agent over the Model Context Protocol.\n\
                      Speaks JSON-RPC on stdin/stdout; logs go to stderr.\n\
                      Records of each run are kept in the working directory,\n\
                      or in LEGUME_MCP_PROJECT when set (LEGUME_MCP_RECORDS=off keeps none).\n\
                      \n\
                      `cocoa mcp watch` follows a running server from a terminal: its calls\n\
                      and background jobs, which s stops; o opens a diff's results.\n\
                      `cocoa diff --watch` follows it from the diff's view."
    )]
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },

    #[command(
        about = "Come back to a detached `cocoa diff` or `cocoa simulate`",
        long_about = "Come back to a detached view. A view leaves its runs going when it is\n\
                      detached (a dropped connection, a closed laptop, or the detach key:\n\
                      ctrl+alt+d unless LEGUME_TUI_DETACH_KEY sets another). A session is\n\
                      found by its label or its id; with one live the name is optional, with\n\
                      several a picker opens. --no-detach on a view starts it plain."
    )]
    Attach(legume_tui::cli::AttachArgs),
    #[command(about = "List the live `cocoa diff` and `cocoa simulate` sessions and their runs")]
    Sessions,
    #[command(
        about = "Stop a `cocoa diff` or `cocoa simulate` session",
        long_about = "Stop a session as q does, without asking: its runs are stopped, given\n\
                      ten seconds to wrap up, then killed. --force kills it at once."
    )]
    Kill(legume_tui::cli::KillArgs),
    #[command(about = "Give a `cocoa diff` or `cocoa simulate` session a label")]
    Rename(legume_tui::cli::RenameArgs),
}

#[derive(Subcommand, Debug)]
enum McpAction {
    #[command(
        about = "Follow a running `cocoa mcp`: its calls and background jobs",
        long_about = "Follow a running `cocoa mcp` from a terminal (the only one running, or\n\
                      a picker of them): its calls as they come, their progress, and the\n\
                      background jobs, which s stops; n leaves the agent a note, and o opens\n\
                      a diff's results in `cocoa diff --results`."
    )]
    Watch {
        #[arg(long, help = "The server's pid, when several run")]
        pid: Option<u32>,
    },
}

/// The session paragraph of a view's long help: what goes on while it is
/// detached, and how to return.
fn session_help(what_goes_on: &str) -> String {
    legume_tui::cli::session_help(
        &tui::TOOL,
        legume_tui::cli::SessionHelp::new()
            .what_goes_on(what_goes_on)
            .more("`?` in the view shows the keys."),
    )
}

/// cocoa's command line with `diff`'s required arguments let go: the view
/// takes a diff set up only in part.
fn relaxed() -> clap::Command {
    Cli::command().mut_subcommand("diff", |d| d.mut_args(|a| a.required(false)))
}

/// `cocoa diff` as its view takes it.
struct ViewCall {
    /// The diff's own matches, required arguments relaxed.
    diff: clap::ArgMatches,
    view: tui::ViewArgs,
    verbose: bool,
    n_threads: Option<usize>,
}

/// The command line `cocoa diff` opens its view on, when it does: never
/// without a terminal or with --batch-process; otherwise when the command
/// line lacks a required argument, or asks for the view's own screens
/// (--results, --watch). A complete one runs straight through, as scripts
/// expect. A command line clap refuses otherwise is left to its own parse;
/// one that asks for a screen without a terminal is refused.
fn view_matches(argv: &[String], terminal: bool) -> Result<Option<ViewCall>, String> {
    if !argv.iter().skip(1).any(|w| w == "diff") {
        return Ok(None);
    }
    let Ok(loose) = relaxed().try_get_matches_from(argv) else {
        return Ok(None);
    };
    let Some((_, m)) = loose.subcommand().filter(|(name, _)| *name == "diff") else {
        return Ok(None);
    };
    let Ok(view) = tui::ViewArgs::from_arg_matches(m) else {
        return Ok(None);
    };
    let asked = view.job || view.results.is_some() || view.watch.is_some();
    if view.batch_process {
        return Ok(None);
    }
    if !terminal && asked {
        return Err("cocoa diff --results, --watch and --job open the view: \
                    they need a terminal"
            .into());
    }
    if !terminal {
        return Ok(None);
    }
    let partial = Cli::try_parse_from(argv)
        .is_err_and(|e| e.kind() == clap::error::ErrorKind::MissingRequiredArgument);
    Ok((asked || partial).then(|| ViewCall {
        diff: m.clone(),
        view,
        verbose: loose.get_flag("verbose"),
        n_threads: loose.get_one::<usize>("n_threads").copied(),
    }))
}

fn main() -> anyhow::Result<()> {
    // A view's session host runs this same program: first, before anything.
    legume_tui::serve_if_host();
    let argv: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if let Some(v) = view_matches(&argv, tui::terminal()).map_err(anyhow::Error::msg)? {
        init_logging(v.verbose, v.n_threads)?;
        return tui::diff(&v.diff, &v.view);
    }
    // Every other command, and a diff run straight through, as clap
    // requires it.
    let cli = Cli::parse();
    init_logging(cli.verbose, cli.n_threads)?;

    match &cli.commands {
        Commands::Diff { args, .. } => {
            run_cocoa_diff(DiffArgs::clone(args))?;
        }
        Commands::Simulate(args) => tui::simulate(args)?,
        Commands::Collapse(args) => {
            run_collapse(args.clone())?;
        }
        Commands::SimulateOne(args) => {
            run_sim_one_type_data(args.clone())?;
        }
        Commands::SimulateCollider(args) => {
            run_sim_collider_data(args.clone())?;
        }
        Commands::Mcp { action: None } => {
            mcp::serve()?;
        }
        Commands::Mcp {
            action: Some(McpAction::Watch { pid }),
        } => tui::watch(*pid)?,
        Commands::Attach(a) => legume_tui::cli::attach(&tui::TOOL, a)?,
        Commands::Sessions => legume_tui::cli::sessions(&tui::TOOL)?,
        Commands::Kill(k) => legume_tui::cli::kill(&tui::TOOL, k)?,
        Commands::Rename(r) => legume_tui::cli::rename(&tui::TOOL, r)?,
    }

    Ok(())
}

/// Logs to stderr at the level `-v` asks for, and the threads
/// `--n-threads` allows.
fn init_logging(verbose: bool, n_threads: Option<usize>) -> anyhow::Result<()> {
    let default_filter = if verbose {
        legume_numeric::matrix::common_io::VERBOSE_LOG_FILTER
    } else {
        legume_numeric::matrix::common_io::QUIET_LOG_FILTER
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_filter))
        .init();

    if let Some(n) = n_threads {
        if n == 0 {
            anyhow::bail!("--n-threads must be >= 1");
        }
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }
    Ok(())
}
