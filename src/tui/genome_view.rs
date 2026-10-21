//! The Results tab's genome view: the figure of [`super::figure`] drawn on
//! screen, the genes placed by the annotation picked (asked of faba, or a
//! GTF on disk), and the genes whose individuals the insets show.

use super::annotation::{self, Genome};
use super::figure::{self, Figure, Inset, INSETS};
use super::results::Results;
use legume_tui::browse::FindList;
use legume_tui::look::Look;
use legume_tui::save::Field;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

type Done<T> = Receiver<Result<T, String>>;

/// The annotation picker, step by step.
pub enum Picking {
    /// faba's sources, and a GTF on disk last.
    Sources {
        list: Vec<(String, String)>,
        at: usize,
    },
    /// A source's releases or species, being listed.
    Listing {
        source: String,
        rx: Done<Vec<String>>,
    },
    Items {
        source: String,
        list: FindList<String>,
    },
    /// A GTF on disk, its path typed.
    Path(Field),
}

/// What the view holds between frames.
#[derive(Default)]
pub struct GenomeView {
    /// The figure shown in place of the gene table.
    pub on: bool,
    pub genome: Option<Genome>,
    /// What it is doing, or why it cannot.
    pub note: Option<String>,
    placing: Option<Done<Genome>>,
    pub picking: Option<Picking>,
    fetch: Option<annotation::Fetch>,
    /// Genes picked for the insets; none: the top called.
    pub pinned: Vec<String>,
    pub insets: Vec<Inset>,
    pub levels: Vec<String>,
    reading: Option<figure::Reading>,
    /// The figure as last drawn, and the area it was drawn for.
    shown: Option<(Rect, Protocol)>,
    /// How many screens wide the genome is (1: all of it on one), and
    /// where the screen starts along it, as a fraction of it.
    zoom: f32,
    from: f32,
}

/// The widest the genome gets: this many screens.
const ZOOM_MAX: f32 = 64.0;

impl GenomeView {
    /// Whether something goes on that the screen should follow.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.placing.is_some()
            || self.reading.is_some()
            || self.fetch.is_some()
            || matches!(self.picking, Some(Picking::Listing { .. }))
    }

    /// A diff opened: its genes placed with `gtf` (given, else the one
    /// picked last), and its insets read.
    pub fn opened(&mut self, r: &Results, gtf: Option<PathBuf>) {
        // Nothing of the last run's lands on this one: its genes pinned, nor
        // its genome or insets still on their way.
        self.genome = None;
        self.placing = None;
        self.reading = None;
        self.pinned.clear();
        self.insets.clear();
        self.levels.clear();
        self.shown = None;
        if let Some(gtf) = gtf.or_else(annotation::last) {
            self.place(gtf, r);
        }
        self.read_insets(r);
    }

    /// Place `r`'s genes with `gtf`, on a thread.
    pub fn place(&mut self, gtf: PathBuf, r: &Results) {
        let names: Vec<String> = r.genes.iter().map(|g| g.name.clone()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        self.note = Some(format!(
            "placing genes with {}",
            legume_tui::browse::tilde(&gtf)
        ));
        std::thread::spawn(move || {
            let placed = annotation::genes(&gtf)
                .map(|g| Genome::place(&gtf, &g, &names))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(placed);
        });
        self.placing = Some(rx);
    }

    /// The genes the insets show: those pinned, else the top called.
    fn inset_genes(&self, r: &Results) -> Vec<String> {
        if !self.pinned.is_empty() {
            return self.pinned.clone();
        }
        r.called()
            .into_iter()
            .take(INSETS)
            .map(|g| r.genes[g].name.clone())
            .collect()
    }

    /// Read the insets' individuals from `r`'s deltas, on a thread.
    pub fn read_insets(&mut self, r: &Results) {
        let wanted: Vec<(String, String, f32)> = self
            .inset_genes(r)
            .into_iter()
            .filter_map(|g| {
                let topic = r.strongest_topic(&g)?;
                let p = r
                    .genes
                    .iter()
                    .find(|x| x.name == g)
                    .map_or(f32::NAN, |x| x.p[r.level]);
                Some((g, topic, p))
            })
            .collect();
        if wanted.is_empty() {
            self.insets.clear();
            self.shown = None;
            self.reading = None;
            return;
        }
        // Each level's log effect, for the individuals at it.
        let mut psi: HashMap<(String, String, usize), f32> = HashMap::new();
        for (g, t, _) in &wanted {
            for l in 0..r.levels.len() {
                psi.insert((g.clone(), t.clone(), l + 1), r.effect_at(g, t, l));
            }
        }
        self.reading = Some(figure::Reading::start(
            r.prefix.clone(),
            wanted,
            move |g, t, l| {
                psi.get(&(g.to_string(), t.to_string(), l))
                    .copied()
                    .unwrap_or(f32::NAN)
            },
        ));
    }

    /// `i` on gene `name`: shown on an inset, or no longer.
    pub fn pin(&mut self, name: &str, r: &Results) {
        if self.pinned.is_empty() {
            self.pinned = self.inset_genes(r);
        }
        match self.pinned.iter().position(|p| p == name) {
            Some(k) => {
                self.pinned.remove(k);
            }
            None => {
                self.pinned.push(name.to_string());
                if self.pinned.len() > INSETS {
                    self.pinned.remove(0);
                }
            }
        }
        self.read_insets(r);
    }

    /// Take in what the threads have; whether anything changed.
    pub fn tick(&mut self, r: Option<&Results>) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.placing {
            if let Ok(placed) = rx.try_recv() {
                self.placing = None;
                changed = true;
                match placed {
                    Ok(g) => {
                        self.note = Some(format!(
                            "{} of {} genes placed with {}",
                            g.found(),
                            g.at.len(),
                            super::draw::name(&g.file)
                        ));
                        self.genome = Some(g);
                    }
                    Err(e) => self.note = Some(format!("the annotation: {e}")),
                }
                self.shown = None;
            }
        }
        if let Some(rx) = &self.reading {
            if let Ok(read) = rx.try_recv() {
                self.reading = None;
                changed = true;
                match read {
                    Ok((insets, levels)) => {
                        self.insets = insets;
                        self.levels = levels;
                    }
                    Err(e) => self.note = Some(format!("the individuals: {e}")),
                }
                self.shown = None;
            }
        }
        if let Some(f) = &self.fetch {
            match f.done.try_recv() {
                Ok(Ok(gtf)) => {
                    self.fetch = None;
                    annotation::remember(&gtf);
                    if let Some(r) = r {
                        self.place(gtf, r);
                    }
                    changed = true;
                }
                Ok(Err(e)) => {
                    self.fetch = None;
                    self.note = Some(e);
                    changed = true;
                }
                Err(_) => {
                    let said = f.said.lock().map(|s| s.clone()).unwrap_or_default();
                    let note = format!("faba: {} · {said}", f.what);
                    changed |= self.note.as_ref() != Some(&note);
                    self.note = Some(note);
                }
            }
        }
        if let Some(Picking::Listing { source, rx }) = &self.picking {
            if let Ok(listed) = rx.try_recv() {
                changed = true;
                self.picking = match listed {
                    Ok(items) => {
                        let mut list = FindList::default();
                        list.set(items, 0);
                        Some(Picking::Items {
                            source: source.clone(),
                            list,
                        })
                    }
                    Err(e) => {
                        self.note = Some(e);
                        None
                    }
                };
            }
        }
        changed
    }

    /// `g`: open the annotation picker on faba's sources.
    pub fn pick_annotation(&mut self) {
        match annotation::sources() {
            Ok(list) => self.picking = Some(Picking::Sources { list, at: 0 }),
            // Without faba, a GTF on disk is still there to pick.
            Err(e) => {
                self.note = Some(format!("{e:#}"));
                self.picking = Some(Picking::Path(Field::new(String::new())));
            }
        }
    }

    /// A key while the picker is open; it always takes it.
    pub fn picker_key(&mut self, k: KeyEvent, r: Option<&Results>) {
        let Some(picking) = self.picking.take() else {
            return;
        };
        self.picking = match picking {
            Picking::Sources { list, at } => match k.code {
                KeyCode::Esc => None,
                KeyCode::Up => Some(Picking::Sources {
                    at: at.saturating_sub(1),
                    list,
                }),
                KeyCode::Down => Some(Picking::Sources {
                    at: (at + 1).min(list.len()),
                    list,
                }),
                KeyCode::Enter if at == list.len() => {
                    Some(Picking::Path(Field::new(String::new())))
                }
                KeyCode::Enter => {
                    let source = list[at].0.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    let s = source.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(annotation::items(&s).map_err(|e| format!("{e:#}")));
                    });
                    Some(Picking::Listing { source, rx })
                }
                _ => Some(Picking::Sources { list, at }),
            },
            Picking::Listing { source, rx } => match k.code {
                KeyCode::Esc => None,
                _ => Some(Picking::Listing { source, rx }),
            },
            Picking::Items { source, mut list } => match k.code {
                KeyCode::Esc if list.find.text.is_empty() => None,
                KeyCode::Enter => {
                    if let Some(item) = list.highlighted() {
                        self.fetch = Some(annotation::Fetch::start(&source, item));
                    }
                    None
                }
                _ => {
                    list.key(&k);
                    Some(Picking::Items { source, list })
                }
            },
            Picking::Path(mut field) => match k.code {
                KeyCode::Esc => None,
                KeyCode::Enter => {
                    let gtf = legume_tui::browse::typed_path(field.text.trim());
                    if gtf.is_file() {
                        annotation::remember(&gtf);
                        if let Some(r) = r {
                            self.place(gtf, r);
                        }
                        None
                    } else {
                        self.note = Some(format!("{} is not a file", gtf.display()));
                        Some(Picking::Path(field))
                    }
                }
                _ => {
                    field.key(k);
                    Some(Picking::Path(field))
                }
            },
        };
    }

    /// The picker as the lines of a pop-up.
    #[must_use]
    pub fn picker_lines(&self, look: &Look, rows: usize) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::styled(" Place genes on the genome: an annotation", look.bold),
            Line::styled(
                " faba downloads it to its reference cache, outside any project",
                look.hint,
            ),
            Line::raw(""),
        ];
        let row = |on: bool, text: String| {
            let style = if on { look.selected } else { look.page };
            Line::from(vec![
                Span::styled(if on { "▸ " } else { "  " }, look.bold),
                Span::styled(text, style),
            ])
        };
        match &self.picking {
            Some(Picking::Sources { list, at }) => {
                for (k, (n, what)) in list.iter().enumerate() {
                    lines.push(row(k == *at, format!("{n:<9} {what}")));
                }
                lines.push(row(*at == list.len(), "a GTF on disk…".into()));
                lines.push(Line::raw(""));
                lines.push(Line::styled(" ↑↓ pick  Enter next  Esc closes", look.hint));
            }
            Some(Picking::Listing { source, .. }) => {
                lines.push(Line::styled(
                    format!(" asking faba what {source} has…"),
                    look.hint,
                ));
            }
            Some(Picking::Items { source, list }) => {
                lines.push(Line::styled(
                    format!(" {source} {}", list.find.tag()),
                    look.hint,
                ));
                let shown: Vec<Line> = list
                    .shown()
                    .enumerate()
                    .map(|(k, s)| row(k == list.at, s.clone()))
                    .collect();
                lines.extend(legume_tui::browse::window(
                    shown,
                    list.at,
                    rows.saturating_sub(7).max(3),
                ));
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    " type to find  ↑↓ pick  Enter downloads (kept for next time)  Esc back",
                    look.hint,
                ));
            }
            Some(Picking::Path(field)) => {
                let mut spans = vec![Span::styled(" GTF or GFF: ", look.bold)];
                spans.extend(field.lines(80, true, look.page, look.selected));
                lines.push(Line::from(spans));
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    " Enter places the genes  Esc closes",
                    look.hint,
                ));
            }
            None => {}
        }
        lines
    }

    /// The share of the genome on screen.
    fn span(&self) -> f32 {
        1.0 / self.zoom.max(1.0)
    }

    /// Where the screen is along the genome, in words: `chr 3–5`.
    #[must_use]
    pub fn window(&self) -> String {
        let span = self.span();
        if span >= 1.0 {
            return "whole genome".into();
        }
        let Some(g) = &self.genome else {
            return format!(
                "{:.0}–{:.0}% of the genes",
                self.from * 100.0,
                (self.from + span) * 100.0
            );
        };
        let len = g.len().max(1) as f32;
        let at = |t: f32| {
            g.chroms
                .iter()
                .rev()
                .find(|(_, o, _)| (*o as f32 / len) <= t)
                .map_or_else(String::new, |(n, _, _)| n.clone())
        };
        let (a, b) = (at(self.from), at(self.from + span * 0.999));
        let zoom = format!("{:.0}× wide", self.zoom);
        if a == b {
            format!("chr {a} · {zoom}")
        } else {
            format!("chr {a}–{b} · {zoom}")
        }
    }

    /// The keys that move along the genome: ←/→ a quarter screen, +/- zoom
    /// about the middle, [ / ] a chromosome, Home/End its ends. Whether it
    /// took the key.
    pub fn nav_key(&mut self, k: KeyEvent) -> bool {
        let span = self.span();
        let mid = self.from + span / 2.0;
        match k.code {
            KeyCode::Left => self.from -= span / 4.0,
            KeyCode::Right => self.from += span / 4.0,
            KeyCode::Home => self.from = 0.0,
            KeyCode::End => self.from = 1.0,
            KeyCode::Char('+' | '=') => {
                self.zoom = (self.zoom.max(1.0) * 2.0).min(ZOOM_MAX);
                self.from = mid - self.span() / 2.0;
            }
            KeyCode::Char('-') => {
                self.zoom = (self.zoom / 2.0).max(1.0);
                self.from = mid - self.span() / 2.0;
            }
            KeyCode::Char(c @ ('[' | ']')) => {
                let Some(g) = &self.genome else { return false };
                let len = g.len().max(1) as f32;
                let starts: Vec<f32> = g.chroms.iter().map(|(_, o, _)| *o as f32 / len).collect();
                let next = if c == ']' {
                    starts.iter().copied().find(|s| *s > self.from + 1e-6)
                } else {
                    starts.iter().copied().rev().find(|s| *s < self.from - 1e-6)
                };
                if let Some(s) = next {
                    // A chromosome a screen: zoomed in far enough to see it.
                    self.zoom = self.zoom.max(4.0);
                    self.from = s;
                }
            }
            _ => return false,
        }
        self.from = self.from.clamp(0.0, 1.0 - self.span());
        self.shown = None;
        true
    }

    fn figure<'a>(&'a self, r: &'a Results, whole: bool) -> Figure<'a> {
        Figure {
            results: r,
            genome: self.genome.as_ref(),
            insets: &self.insets,
            levels: &self.levels,
            from: if whole { 0.0 } else { self.from },
            span: if whole { 1.0 } else { self.span() },
        }
    }

    /// The figure in `area` through `picker`, drawn again only when it
    /// changed or the area did.
    pub fn draw(&mut self, f: &mut ratatui::Frame, area: Rect, r: &Results, picker: &Picker) {
        if area.width < 20 || area.height < 8 {
            return;
        }
        let stale = self.shown.as_ref().is_none_or(|(a, _)| *a != area);
        if stale {
            let drawn = figure::on_screen(area, picker, |frame, font, bold| {
                self.figure(r, false).draw(frame, font, bold);
            });
            match drawn {
                Ok(p) => self.shown = Some((area, p)),
                Err(e) => {
                    self.note = Some(format!("the figure: {e}"));
                    return;
                }
            }
        }
        if let Some((_, p)) = &self.shown {
            f.render_widget(ratatui_image::Image::new(p), area);
        }
    }

    /// Draw again at the next frame: a new terminal, a new level.
    pub fn redraw(&mut self) {
        self.shown = None;
    }

    /// The figure as a PDF page at `path`, `page` wide.
    pub fn save(
        &self,
        r: &Results,
        path: &Path,
        page: legume_tui::export::PageSize,
    ) -> anyhow::Result<()> {
        figure::save_pdf(path, page, 0.45, |frame, font, bold| {
            self.figure(r, true).draw(frame, font, bold);
        })
    }
}

#[cfg(test)]
#[path = "tests/genome_view.rs"]
mod tests;
