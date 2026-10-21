//! The Results tab's gene page: one gene's [`GeneFigure`], opened from the
//! gene table (Enter), from a name typed into the finder (`/`), or with
//! `--gene`; n and N (↓ and ↑) step to the next and previous gene of the
//! table's order, + and - narrow and widen its locus.

use super::annotation::Genome;
use super::figure::{self, Inset};
use super::gene_figure::GeneFigure;
use super::results::Results;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use std::collections::HashMap;

/// Half the locus shown at first, and its bounds, in bases.
const FLANK: u64 = 1_000_000;
const FLANK_MIN: u64 = 50_000;
const FLANK_MAX: u64 = 64_000_000;

/// What a key did on the page.
#[derive(Debug, PartialEq, Eq)]
pub enum Took {
    Consumed,
    /// Esc: back to where it was opened from.
    Close,
    Ignored,
}

pub struct GeneView {
    /// The gene, by its index in the results.
    pub gene: usize,
    pub facets: Vec<Inset>,
    pub levels: Vec<String>,
    reading: Option<figure::Reading>,
    pub flank: u64,
    pub note: Option<String>,
    shown: Option<(Rect, Protocol)>,
}

impl GeneView {
    /// The page of gene `gene`, its individuals being read.
    #[must_use]
    pub fn open(r: &Results, gene: usize) -> Self {
        let mut v = GeneView {
            gene,
            facets: Vec::new(),
            levels: Vec::new(),
            reading: None,
            flank: FLANK,
            note: None,
            shown: None,
        };
        v.read(r);
        v
    }

    /// The gene of `r` named `name`: its whole name, or its symbol.
    #[must_use]
    pub fn find(r: &Results, name: &str) -> Option<usize> {
        let n = name.to_ascii_uppercase();
        r.genes
            .iter()
            .position(|g| g.name.to_ascii_uppercase() == n)
            .or_else(|| {
                r.genes
                    .iter()
                    .position(|g| figure::shown_name(&g.name).to_ascii_uppercase() == n)
            })
    }

    #[must_use]
    pub fn busy(&self) -> bool {
        self.reading.is_some()
    }

    /// Read the individuals again: another contrast level shifts them.
    pub fn reread(&mut self, r: &Results) {
        self.read(r);
    }

    /// Read the gene's individuals in every topic, on a thread.
    fn read(&mut self, r: &Results) {
        let name = r.genes[self.gene].name.clone();
        let wanted: Vec<(String, String, f32)> = r
            .topics()
            .iter()
            .map(|t| (name.clone(), t.clone(), f32::NAN))
            .collect();
        let mut psi: HashMap<(String, usize), f32> = HashMap::new();
        for t in r.topics() {
            for l in 0..r.levels.len() {
                psi.insert((t.clone(), l + 1), r.effect_at(&name, t, l));
            }
        }
        let reading = figure::Reading::start(r.prefix.clone(), wanted, move |_, t, l| {
            psi.get(&(t.to_string(), l)).copied().unwrap_or(f32::NAN)
        });
        self.note = Some(format!(
            "reading {}'s individuals",
            figure::shown_name(&name)
        ));
        self.reading = Some(reading);
        self.shown = None;
    }

    /// Go to gene `gene`.
    pub fn go(&mut self, r: &Results, gene: usize) {
        if gene != self.gene {
            self.gene = gene;
            self.facets.clear();
            self.read(r);
        }
    }

    /// Take in the individuals once read; whether anything changed.
    pub fn tick(&mut self) -> bool {
        let Some(rx) = &self.reading else {
            return false;
        };
        let Ok(read) = rx.try_recv() else {
            return false;
        };
        self.reading = None;
        match read {
            Ok((facets, levels)) => {
                self.facets = facets;
                self.levels = levels;
                self.note = None;
            }
            Err(e) => self.note = Some(format!("the individuals: {e}")),
        }
        self.shown = None;
        true
    }

    /// A key on the page.
    pub fn key(&mut self, k: KeyEvent, r: &Results) -> Took {
        let at = r.order.iter().position(|&g| g == self.gene);
        let step = |d: isize| {
            at.map(|a| {
                a.saturating_add_signed(d)
                    .min(r.order.len().saturating_sub(1))
            })
            .and_then(|a| r.order.get(a).copied())
        };
        match k.code {
            KeyCode::Esc | KeyCode::Backspace => return Took::Close,
            KeyCode::Char('n') | KeyCode::Down => {
                if let Some(g) = step(1) {
                    self.go(r, g);
                }
            }
            KeyCode::Char('N') | KeyCode::Up => {
                if let Some(g) = step(-1) {
                    self.go(r, g);
                }
            }
            KeyCode::Char('+' | '=') => self.flank = (self.flank / 2).max(FLANK_MIN),
            KeyCode::Char('-') => self.flank = (self.flank * 2).min(FLANK_MAX),
            _ => return Took::Ignored,
        }
        self.shown = None;
        Took::Consumed
    }

    /// Draw again at the next frame.
    pub fn redraw(&mut self) {
        self.shown = None;
    }

    fn figure<'a>(&'a self, r: &'a Results, genome: Option<&'a Genome>) -> GeneFigure<'a> {
        GeneFigure {
            results: r,
            genome,
            gene: self.gene,
            facets: &self.facets,
            levels: &self.levels,
            flank: self.flank,
        }
    }

    /// The page in `area` through `picker`, drawn again only when it
    /// changed or the area did.
    pub fn draw(
        &mut self,
        f: &mut ratatui::Frame,
        area: Rect,
        r: &Results,
        genome: Option<&Genome>,
        picker: &Picker,
    ) {
        if area.width < 20 || area.height < 8 {
            return;
        }
        if self.shown.as_ref().is_none_or(|(a, _)| *a != area) {
            let drawn = figure::on_screen(area, picker, |frame, font, bold| {
                self.figure(r, genome).draw(frame, font, bold);
            });
            match drawn {
                Ok(p) => self.shown = Some((area, p)),
                Err(e) => {
                    self.note = Some(format!("the page: {e}"));
                    return;
                }
            }
        }
        if let Some((_, p)) = &self.shown {
            f.render_widget(ratatui_image::Image::new(p), area);
        }
    }

    /// The page as a PDF at `path`, `page` wide.
    pub fn save(
        &self,
        r: &Results,
        genome: Option<&Genome>,
        path: &std::path::Path,
        page: legume_tui::export::PageSize,
    ) -> anyhow::Result<()> {
        figure::save_pdf(path, page, 0.7, |frame, font, bold| {
            self.figure(r, genome).draw(frame, font, bold);
        })
    }
}

#[cfg(test)]
#[path = "tests/gene_view.rs"]
mod tests;
