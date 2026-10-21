//! `cocoa simulate`'s setup: which model, its sizes and the strength of
//! each edge of its DAG, as rows over the model's own flags (the Flags tab
//! has the rest of them), and where the outputs go.

use legume_tui::form::{Field, Form};

/// The simulators, by their DAGs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Model {
    /// One cell type, W → X → Y with W → Y.
    One,
    /// Many cell types, with X → A ← U making cell type a collider.
    #[default]
    Collider,
}

impl Model {
    /// The command that simulates it.
    #[must_use]
    pub fn command(self) -> &'static str {
        match self {
            Model::One => "simulate-one",
            Model::Collider => "simulate-collider",
        }
    }

    #[must_use]
    pub fn other(self) -> Model {
        match self {
            Model::One => Model::Collider,
            Model::Collider => Model::One,
        }
    }

    /// In a word, for the header and the output's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Model::One => "one",
            Model::Collider => "collider",
        }
    }

    /// The DAG, as `simulate-one --help` and `simulate-collider --help`
    /// draw it.
    #[must_use]
    pub fn dag(self) -> &'static [&'static str] {
        match self {
            Model::One => &["W ──→ X ──→ Y", "└──────────↗"],
            Model::Collider => &[
                "┌──→ A ←───┐",
                "│          │",
                "X ──→ Y ←─ U",
                "↑     ↑",
                "└─ V ─┘",
            ],
        }
    }

    /// The rows of the Model tab: sizes first, then each edge's PVE.
    #[must_use]
    pub fn rows(self) -> &'static [Row] {
        match self {
            Model::One => &ONE,
            Model::Collider => &COLLIDER,
        }
    }

    /// Files beside the counts that a diff reads: individuals, exposures,
    /// cell types (as topics), and the truth.
    #[must_use]
    pub fn beside(self) -> &'static [&'static str] {
        match self {
            Model::One => &["samples.gz", "exposures.gz"],
            Model::Collider => &["samples.gz", "exposures.gz", "celltypes.gz"],
        }
    }
}

/// A row of the Model tab: a flag of the model's command, what it is
/// called there, and the value it starts at when clap has none.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub long: &'static str,
    pub label: &'static str,
    /// An edge's PVE, drawn as a bar and stepped by [`PVE_STEP`].
    pub edge: bool,
    pub start: Option<&'static str>,
}

const fn size(long: &'static str, label: &'static str, start: Option<&'static str>) -> Row {
    Row {
        long,
        label,
        edge: false,
        start,
    }
}

const fn edge(long: &'static str, label: &'static str) -> Row {
    Row {
        long,
        label,
        edge: true,
        start: None,
    }
}

const ONE: [Row; 10] = [
    size("n-genes", "genes", Some("2000")),
    size("n-cells", "cells", Some("10000")),
    size("n-causal-genes", "causal genes", Some("200")),
    size("n-exposure", "exposure levels", None),
    size("n-samples-per-exposure", "individuals per level", None),
    size("effect-size", "effect size", None),
    size("rseed", "seed", None),
    edge("pve-covar-exposure", "W → X"),
    edge("pve-exposure-gene", "X → Y"),
    edge("pve-covar-gene", "W → Y"),
];

const COLLIDER: [Row; 14] = [
    size("n-genes", "genes", Some("2000")),
    size("n-cells", "cells", Some("10000")),
    size("n-causal-genes", "causal genes", Some("200")),
    size("n-cell-types", "cell types", Some("5")),
    size("n-exposure", "exposure levels", None),
    size("n-samples-per-exposure", "individuals per level", None),
    size("effect-size", "effect size", None),
    size("rseed", "seed", None),
    edge("pve-covar-exposure", "V → X"),
    edge("pve-exposure-celltype", "X → A"),
    edge("pve-cell-covar-celltype", "U → A"),
    edge("pve-exposure-gene", "X → Y"),
    edge("pve-covar-gene", "V → Y"),
    edge("pve-cell-covar-gene", "U → Y"),
];

/// How far ←/→ moves an edge's PVE.
pub const PVE_STEP: f32 = 0.05;

/// A PVE `v` moved by `d` steps, held in [0, 1], as the form writes it.
#[must_use]
pub fn stepped(v: &str, d: f32) -> Option<String> {
    let v: f32 = v.trim().parse().ok()?;
    let v = ((v + d * PVE_STEP) / PVE_STEP).round() * PVE_STEP;
    Some(format!("{:.2}", v.clamp(0.0, 1.0)))
}

/// A size `v` moved by `d`: the next round number up or down (×2, ÷2 for
/// counts; ±1 for small ones), as text.
#[must_use]
pub fn resized(v: &str, d: isize) -> Option<String> {
    if let Ok(n) = v.trim().parse::<u64>() {
        let n = match (d > 0, n < 10) {
            (true, true) => n + 1,
            (false, true) => n.saturating_sub(1).max(1),
            (true, false) => n * 2,
            (false, false) => (n / 2).max(1),
        };
        return Some(n.to_string());
    }
    let x: f64 = v.trim().parse().ok()?;
    Some(format!("{}", x + d as f64))
}

/// The two models' forms, the one shown, and whether a diff follows.
pub struct Sim {
    pub model: Model,
    pub one: Form,
    pub collider: Form,
    /// Queue `cocoa diff` on the simulation after it.
    pub then_diff: bool,
    /// The Model tab's row.
    pub at: usize,
}

impl Sim {
    /// The forms of `app`'s two simulators, without `--output`; their sizes
    /// start where [`Row::start`] says.
    pub fn new(app: &clap::Command, model: Model) -> anyhow::Result<Self> {
        let form = |m: Model| -> anyhow::Result<Form> {
            let sub = crate::runs::subcommand(app, m.command())?;
            let mut f = Form::new(sub, |a| {
                a.get_long().is_some_and(|l| l != "output")
                    && !crate::tui::VIEW_IDS.contains(&a.get_id().as_str())
            });
            for r in m.rows() {
                if let (Some(start), Some(field)) = (r.start, f.get_mut(r.long)) {
                    field.value = start.to_string();
                }
            }
            Ok(f)
        };
        Ok(Sim {
            model,
            one: form(Model::One)?,
            collider: form(Model::Collider)?,
            then_diff: true,
            at: 0,
        })
    }

    #[must_use]
    pub fn form(&self) -> &Form {
        match self.model {
            Model::One => &self.one,
            Model::Collider => &self.collider,
        }
    }

    pub fn form_mut(&mut self) -> &mut Form {
        match self.model {
            Model::One => &mut self.one,
            Model::Collider => &mut self.collider,
        }
    }

    /// The row under the cursor, and its field.
    #[must_use]
    pub fn row(&self) -> Option<(&Row, &Field)> {
        let r = self.model.rows().get(self.at)?;
        Some((r, self.form().get(r.long)?))
    }

    /// The value of `long`, as a number, when it is one.
    #[must_use]
    pub fn number(&self, long: &str) -> Option<f64> {
        self.form().get(long)?.value.trim().parse().ok()
    }

    /// The Model tab's row moved by `d`.
    pub fn step(&mut self, d: isize) {
        let last = self.model.rows().len().saturating_sub(1);
        self.at = legume_tui::browse::moved(self.at, d, last);
    }

    /// ←/→ on the row: an edge's PVE by a step, a size to the next round
    /// number.
    pub fn nudge(&mut self, d: isize) {
        let Some(r) = self.model.rows().get(self.at).copied() else {
            return;
        };
        let Some(f) = self.form_mut().get_mut(r.long) else {
            return;
        };
        let moved = if r.edge {
            stepped(&f.value, d as f32)
        } else {
            resized(&f.value, d)
        };
        if let Some(v) = moved {
            f.value = v;
        }
    }

    /// The other model, keeping the row where the other has it.
    pub fn switch(&mut self) {
        let long = self.model.rows().get(self.at).map(|r| r.long);
        self.model = self.model.other();
        self.at = long
            .and_then(|l| self.model.rows().iter().position(|r| r.long == l))
            .unwrap_or(0);
    }

    /// Individuals in all: exposure levels × individuals per level.
    #[must_use]
    pub fn individuals(&self) -> Option<f64> {
        Some(self.number("n-exposure")? * self.number("n-samples-per-exposure")?)
    }
}

#[cfg(test)]
#[path = "tests/sim.rs"]
mod tests;
