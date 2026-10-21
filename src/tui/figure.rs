//! The genome figure of a diff, as the CoCoA-diff papers draw one: every
//! gene's average exposure effect along the chromosomes (grey), the genes
//! called drawn again in each cell type (topic) at its own log effect, in
//! the topic's colour and ringed where it is strongest, the top ones named;
//! and under it, for a few genes, each individual's effect, sorted, the
//! reference level hollow and the others filled, sized by the individual's
//! count, with its spread and the gene's p-value.
//!
//! Drawn on a legume-tui frame, so the same pixels go on screen (through
//! the terminal's image protocol, or block characters) and on a PDF page,
//! its text kept as text.

use super::annotation::Genome;
use super::results::Results;
use legume_tui::map::colour::{to_linear, Lin, Srgb};
use legume_tui::map::labels::{place, Label, Rect};
use legume_tui::map::offsets::Offsets;
use legume_tui::map::raster::{ring, stamp, Frame, Shape};
use legume_tui::map::text::{self, Font};
use legume_tui::map::theme::Theme;
use std::collections::HashMap;

/// The page the figure is drawn on, on screen and in the PDF.
pub const THEME: Theme = Theme::PAPER;
/// A gene's p-value over its inset.
const PVALUE: Srgb = [196, 32, 160];
/// The individuals at an exposed level.
const EXPOSED: Srgb = [122, 64, 160];
/// Genes named on the genome panel at most.
const NAMED: usize = 24;
/// Called genes drawn in their topics' colours, the best in sight first:
/// as many on a zoomed-in screen as on the whole genome.
const COLOURED: usize = 100;

/// A gene's name as a figure prints it: the symbol of `ENSG…_SYMBOL`.
#[must_use]
pub fn shown_name(name: &str) -> &str {
    match name.split_once('_') {
        Some((id, symbol)) if id.starts_with("ENS") && !symbol.is_empty() => symbol,
        _ => name,
    }
}
/// Insets drawn at most.
pub const INSETS: usize = 5;

/// One individual on an inset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// `log(omega / tau(0))`: the individual's rate over the reference
    /// level's mean, its exposure's effect included.
    pub y: f32,
    /// Its spread on the log scale, `1/sqrt(count + 1)`; NaN when the run
    /// wrote no counts.
    pub sd: f32,
    pub count: f32,
    /// Its exposure level, 0 the reference.
    pub level: usize,
}

/// A gene's individuals in one topic.
#[derive(Clone, Debug, PartialEq)]
pub struct Inset {
    pub gene: String,
    pub topic: String,
    pub p: f32,
    pub points: Vec<Point>,
}

/// What the figure shows.
pub struct Figure<'a> {
    pub results: &'a Results,
    pub genome: Option<&'a Genome>,
    pub insets: &'a [Inset],
    /// The exposure levels' names, the reference first.
    pub levels: &'a [String],
    /// The part of the genome shown, as fractions of it: where it starts
    /// and how much of it (1: all of it).
    pub from: f32,
    pub span: f32,
}

/// A figure `draw` makes, rasterized to fill `area` of the screen through
/// `picker`.
pub fn on_screen(
    area: ratatui::layout::Rect,
    picker: &ratatui_image::picker::Picker,
    draw: impl FnOnce(&mut Frame, Font, Font),
) -> anyhow::Result<ratatui_image::protocol::Protocol> {
    let (cw, ch) = legume_tui::map::display::cell(picker);
    let (w, h) = (
        (f32::from(area.width) * cw) as usize,
        (f32::from(area.height) * ch) as usize,
    );
    let mut frame = Frame::blank(w, h, THEME.background);
    let (font, bold) = legume_tui::export::fonts(ch);
    draw(&mut frame, font, bold);
    legume_tui::map::display::fit_protocol(picker, frame.to_image(), area)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// A figure `draw` makes, as a PDF page at `path`, `page` wide and `tall`
/// of its width high.
pub fn save_pdf(
    path: &std::path::Path,
    page: legume_tui::export::PageSize,
    tall: f32,
    draw: impl FnOnce(&mut Frame, Font, Font),
) -> anyhow::Result<()> {
    use legume_tui::export::{write_pdf, Page};
    let (pdf, ()) = Page::draw(page.px(tall), THEME.background, page.cell_px(), draw);
    write_pdf(&[pdf], page.dpi as f32, path)?;
    Ok(())
}

/// A rectangle filled, clipped to the frame.
pub(super) fn fill(frame: &mut Frame, x0: f32, y0: f32, x1: f32, y1: f32, c: Lin) {
    let cl = |v: f32, max: usize| (v.round().max(0.0) as usize).min(max);
    let (x0, x1) = (cl(x0.min(x1), frame.w), cl(x0.max(x1), frame.w));
    let (y0, y1) = (cl(y0.min(y1), frame.h), cl(y0.max(y1), frame.h));
    if x1 > x0 && y1 > y0 {
        frame.fill([x0, y0, x1, y1], c);
    }
}

/// Steps for an axis over `[lo, hi]`: about `n` round numbers.
fn ticks(lo: f32, hi: f32, n: usize) -> Vec<f32> {
    let span = (hi - lo).abs().max(1e-6);
    let raw = span / n.max(1) as f32;
    let mag = 10f32.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let mut t = (lo / step).ceil() * step;
    let mut out = Vec::new();
    while t <= hi + step * 1e-3 {
        out.push(if t.abs() < step * 1e-3 { 0.0 } else { t });
        t += step;
    }
    out
}

fn tick_text(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A p-value as the figure prints it: `7.4e-07`.
#[must_use]
pub fn p_text(p: f32) -> String {
    if !p.is_finite() {
        return String::new();
    }
    if p >= 0.001 {
        return format!("{p:.3}");
    }
    // `7.4e-7` written `7.4e-07`, as R prints it.
    let s = format!("{p:.1e}");
    match s.split_once("e-") {
        Some((m, e)) if e.len() == 1 => format!("{m}e-0{e}"),
        _ => s,
    }
}

/// The colour of topic `k` of `n`.
fn topic_colour(k: usize, n: usize) -> Lin {
    THEME.palette.colour(k, n)
}

impl Figure<'_> {
    /// Each gene's place across, in `[0, 1]`; none where it is not placed.
    fn x_of(&self, g: usize) -> Option<f32> {
        let n = self.results.genes.len().max(1);
        match self.genome {
            Some(gen) => {
                let len = gen.len().max(1) as f32;
                gen.at.get(g).copied().flatten().map(|p| p as f32 / len)
            }
            None => Some((g as f32 + 0.5) / n as f32),
        }
    }

    /// Draw the figure on `frame`.
    pub fn draw(&self, frame: &mut Frame, font: Font, bold: Font) {
        let lh = font.line_height() as f32;
        let (w, h) = (frame.w as f32, frame.h as f32);
        let ink = to_linear(THEME.text);
        let bg = frame.background();
        let r = self.results;
        let run = r
            .prefix
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let title = match r.levels.len() {
            1 => format!("{run} · average exposure effect"),
            _ => format!("{run} · average exposure effect, {}", r.level_name()),
        };
        text::draw(frame, bold, &title, lh as i32, (lh * 0.4) as i32, ink, bg);
        let insets_h = if self.insets.is_empty() {
            0.0
        } else {
            (h * 0.36).max(lh * 9.0).min(h * 0.5)
        };
        let top = [0.0, lh * 1.6, w, h - insets_h];
        self.draw_genome(frame, top, font);
        if insets_h > 0.0 {
            self.draw_insets(frame, [0.0, h - insets_h, w, h], font, bold);
        }
    }

    /// The genome panel in `area`: chromosomes across, effects up.
    fn draw_genome(&self, frame: &mut Frame, area: [f32; 4], font: Font) {
        let r = self.results;
        let lh = font.line_height() as f32;
        let adv = font.advance() as f32;
        let bg = frame.background();
        let ink = to_linear(THEME.text);
        let hint = to_linear(THEME.hint);
        let muted = to_linear(THEME.muted);
        let topics = r.topics();
        let n_topics = topics.len();
        let key_w = topics.iter().map(|t| font.width(t)).max().unwrap_or(0) as f32 + lh * 2.0;
        let x0 = area[0] + adv * 7.0 + lh;
        let x1 = area[2] - key_w - lh * 0.5;
        let y0 = area[1] + lh * 1.4;
        let y1 = area[3] - lh * 0.8;
        if x1 - x0 < lh * 4.0 || y1 - y0 < lh * 3.0 {
            return;
        }
        let l = r.level;
        let called = r.called();
        // The values drawn: every gene's average, each called gene's topics.
        let mut lim = 0f32;
        for g in &r.genes {
            if g.contrast[l].is_finite() {
                lim = lim.max(g.contrast[l].abs());
            }
        }
        for &g in &called {
            for v in r.topic_effects(&r.genes[g].name).into_iter().flatten() {
                if v.is_finite() {
                    lim = lim.max(v.abs());
                }
            }
        }
        let lim = (lim * 1.08).max(1e-3);
        let ys = |v: f32| y1 - (v + lim) / (2.0 * lim) * (y1 - y0);
        let span = self.span.clamp(1e-4, 1.0);
        let from = self.from.clamp(0.0, 1.0 - span);
        let xs = |t: f32| x0 + (t - from) / span * (x1 - x0);
        let seen = |t: f32| t >= from && t <= from + span;

        // Chromosome bands and names; the data's order without a genome.
        match self.genome {
            Some(gen) => {
                let len = gen.len().max(1) as f32;
                for (k, (name, off, l)) in gen.chroms.iter().enumerate() {
                    let (a, b) = (xs(*off as f32 / len), xs((off + l) as f32 / len));
                    let (a, b) = (a.max(x0), b.min(x1));
                    if b <= a {
                        continue;
                    }
                    if k % 2 == 1 {
                        fill(frame, a, y0, b, y1, to_linear([236, 235, 231]));
                    }
                    let tw = font.width(name) as f32;
                    if b - a > tw * 0.9 {
                        let cx = (a + b - tw) / 2.0;
                        text::draw(
                            frame,
                            font,
                            name,
                            cx as i32,
                            (y0 - lh * 1.2) as i32,
                            ink,
                            bg,
                        );
                    }
                }
            }
            None => {
                let note = "genes in the data's order (g places them on the genome)";
                text::draw(
                    frame,
                    font,
                    note,
                    x0 as i32,
                    (y0 - lh * 1.2) as i32,
                    hint,
                    bg,
                );
            }
        }
        // The axis, its ticks and the zero line.
        fill(frame, x0 - 1.0, y0, x0, y1, hint);
        for t in ticks(-lim, lim, 4) {
            let y = ys(t);
            fill(frame, x0 - adv * 0.6, y, x0, y + 1.0, hint);
            let s = tick_text(t);
            let tw = font.width(&s) as f32;
            text::draw(
                frame,
                font,
                &s,
                (x0 - adv - tw) as i32,
                (y - lh / 2.0) as i32,
                hint,
                bg,
            );
        }
        fill(frame, x0, ys(0.0), x1, ys(0.0) + 1.5, ink);
        let ylab = "average exposure effect";
        let mid = (y0 + y1) / 2.0 + font.width(ylab) as f32 / 2.0;
        text::draw_vertical(frame, font, ylab, (area[0] as i32 + 2, mid as i32), ink, bg);

        // Every gene, grey.
        let small = (lh * 0.11).max(1.2);
        for (g, gene) in r.genes.iter().enumerate() {
            let (Some(t), v) = (self.x_of(g).filter(|t| seen(*t)), gene.contrast[l]) else {
                continue;
            };
            if v.is_finite() {
                stamp(frame, xs(t), ys(v), small, Shape::Circle, muted, 1.0);
            }
        }
        // The called genes in each topic, the strongest ringed; the top
        // ones in sight named.
        let big = (lh * 0.24).max(2.5);
        let mut labels: Vec<Label<usize>> = Vec::new();
        let mut rank = 0;
        for &g in &called {
            let Some(t) = self.x_of(g).filter(|t| seen(*t)) else {
                continue;
            };
            if rank >= COLOURED {
                break;
            }
            let name = &r.genes[g].name;
            let Some(row) = r.topic_effects(name) else {
                continue;
            };
            let x = xs(t);
            let strongest = row
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite())
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(k, _)| k);
            for (k, v) in row.iter().enumerate().filter(|(_, v)| v.is_finite()) {
                stamp(
                    frame,
                    x,
                    ys(*v),
                    big,
                    Shape::Circle,
                    topic_colour(k, n_topics),
                    1.0,
                );
            }
            if let Some(k) = strongest {
                let y = ys(row[k]);
                ring(frame, x, y, big, ink);
                if rank < NAMED {
                    labels.push(Label {
                        text: shown_name(name).to_string(),
                        x,
                        y: y - lh * 0.9 * row[k].signum(),
                        ink,
                        tier: 0,
                        priority: (NAMED - rank) as f32,
                        font,
                        key: Some(g),
                    });
                }
                rank += 1;
            }
        }
        // Where the window is along the whole genome.
        if span < 1.0 {
            let (by, bh) = (y1 + lh * 0.35, (lh * 0.18).max(2.0));
            fill(frame, x0, by, x1, by + bh, to_linear([224, 223, 219]));
            let (a, b) = (x0 + from * (x1 - x0), x0 + (from + span) * (x1 - x0));
            fill(frame, a, by, b.max(a + 2.0), by + bh, hint);
        }
        // The key: each topic's colour.
        let kx = x1 + lh * 0.8;
        let mut ky = y0;
        let reserved: Vec<Rect> = vec![[kx - lh * 0.5, y0, area[2], y0 + lh * n_topics as f32]];
        for (k, t) in topics.iter().enumerate() {
            stamp(
                frame,
                kx + lh * 0.3,
                ky + lh * 0.5,
                lh * 0.22,
                Shape::Circle,
                topic_colour(k, n_topics),
                1.0,
            );
            text::draw(frame, font, t, (kx + lh * 0.8) as i32, ky as i32, ink, bg);
            ky += lh;
        }
        let _ = place(frame, labels, &reserved, &Offsets::default());
    }

    /// The insets in `area`, side by side, and their key at the right.
    fn draw_insets(&self, frame: &mut Frame, area: [f32; 4], font: Font, bold: Font) {
        let lh = font.line_height() as f32;
        let adv = font.advance() as f32;
        let key_w = adv * 12.0;
        let n = self.insets.len().min(INSETS);
        let each = (area[2] - area[0] - key_w - lh) / n.max(1) as f32;
        let dots = Dots::new(self.levels, self.insets, lh);
        for (k, inset) in self.insets.iter().take(n).enumerate() {
            let left = area[0] + lh * 0.5 + k as f32 * each;
            let sub = format!("in {}", inset.topic);
            let panel = [left, area[1], left + each, area[3]];
            individuals(
                frame,
                panel,
                inset,
                [shown_name(&inset.gene), &sub],
                &dots,
                font,
                bold,
            );
        }
        dots.key(frame, area[2] - key_w, area[1] + lh * 2.6, font);
    }
}

/// How individuals are drawn across a figure's panels: a dot as large as
/// the square root of the count, against the largest count drawn; the
/// reference level hollow, the others filled.
pub struct Dots<'a> {
    levels: &'a [String],
    max_count: f32,
    lh: f32,
}

impl<'a> Dots<'a> {
    #[must_use]
    pub fn new(levels: &'a [String], insets: &[Inset], lh: f32) -> Self {
        let max_count = insets
            .iter()
            .flat_map(|i| i.points.iter().map(|p| p.count))
            .filter(|c| c.is_finite())
            .fold(1f32, f32::max);
        Dots {
            levels,
            max_count,
            lh,
        }
    }

    fn radius(&self, c: f32) -> f32 {
        let c = if c.is_finite() { c } else { 0.0 };
        self.lh * 0.12 + self.lh * 0.26 * (c.sqrt() / self.max_count.sqrt())
    }

    fn colour(&self, l: usize) -> Lin {
        if self.levels.len() <= 2 {
            to_linear(EXPOSED)
        } else {
            THEME.palette.colour(l, self.levels.len())
        }
    }

    /// One dot of level `l`, radius `r`, at `(x, y)`.
    fn dot(&self, frame: &mut Frame, x: f32, y: f32, r: f32, l: usize) {
        if l == 0 {
            stamp(
                frame,
                x,
                y,
                r,
                Shape::Circle,
                to_linear([255, 255, 255]),
                1.0,
            );
            ring(frame, x, y, r, to_linear([60, 60, 60]));
        } else {
            stamp(frame, x, y, r, Shape::Circle, self.colour(l), 1.0);
        }
    }

    /// The key at `(kx, ky)` down: each level, then what a dot's size says.
    pub fn key(&self, frame: &mut Frame, kx: f32, mut ky: f32, font: Font) {
        let (lh, ink, bg) = (self.lh, to_linear(THEME.text), frame.background());
        for (l, name) in self.levels.iter().enumerate() {
            self.dot(frame, kx + lh * 0.4, ky + lh * 0.5, lh * 0.25, l);
            text::draw(frame, font, name, (kx + lh) as i32, ky as i32, ink, bg);
            ky += lh;
        }
        ky += lh * 0.5;
        text::draw(frame, font, "sqrt(count)", kx as i32, ky as i32, ink, bg);
        ky += lh * 1.2;
        let top = self.max_count.sqrt().max(1.0);
        for s in [top * 0.25, top * 0.5, top] {
            let s = s.round().max(1.0);
            self.dot(frame, kx + lh * 0.4, ky + lh * 0.5, self.radius(s * s), 0);
            text::draw(
                frame,
                font,
                &format!("{s}"),
                (kx + lh * 1.2) as i32,
                ky as i32,
                ink,
                bg,
            );
            ky += lh * 1.1;
        }
    }
}

/// One gene's individuals in one topic, in `area`: the `heading`'s title
/// and subtitle over them, sorted by effect, each with its spread, the
/// p-value in a corner.
pub fn individuals(
    frame: &mut Frame,
    area: [f32; 4],
    inset: &Inset,
    [title, sub]: [&str; 2],
    dots: &Dots,
    font: Font,
    bold: Font,
) {
    let lh = font.line_height() as f32;
    let adv = font.advance() as f32;
    let bg = frame.background();
    let (ink, hint) = (to_linear(THEME.text), to_linear(THEME.hint));
    let [left, top, right, bottom] = area;
    let each = right - left;
    let head = top + lh * 0.3;
    let tw = bold.width(title) as f32;
    text::draw(
        frame,
        bold,
        title,
        (left + (each - tw) / 2.0) as i32,
        head as i32,
        ink,
        bg,
    );
    let sw = font.width(sub) as f32;
    text::draw(
        frame,
        font,
        sub,
        (left + (each - sw) / 2.0) as i32,
        (head + lh) as i32,
        hint,
        bg,
    );
    let (x0, x1) = (left + adv * 5.5 + lh, right - lh * 0.8);
    let (y0, y1) = (head + lh * 2.6, bottom - lh * 2.0);
    if x1 - x0 < lh * 3.0 || y1 - y0 < lh * 2.0 {
        return;
    }
    let yl = "exposure effect";
    let ym = (y0 + y1) / 2.0 + font.width(yl) as f32 / 2.0;
    text::draw_vertical(frame, font, yl, (left as i32, ym as i32), ink, bg);
    let mut pts = inset.points.clone();
    pts.retain(|p| p.y.is_finite());
    pts.sort_by(|a, b| a.y.total_cmp(&b.y));
    if pts.is_empty() {
        let note = "no individuals with cells here";
        text::draw(
            frame,
            font,
            note,
            x0 as i32,
            ((y0 + y1) / 2.0) as i32,
            hint,
            bg,
        );
        return;
    }
    let spread = |p: &Point| if p.sd.is_finite() { p.sd } else { 0.0 };
    let lo = pts
        .iter()
        .map(|p| p.y - spread(p))
        .fold(f32::INFINITY, f32::min);
    let hi = pts
        .iter()
        .map(|p| p.y + spread(p))
        .fold(f32::NEG_INFINITY, f32::max);
    let (lo, hi) = if lo.is_finite() && hi > lo {
        (lo, hi)
    } else {
        (-1.0, 1.0)
    };
    let pad = (hi - lo) * 0.06;
    let (lo, hi) = (lo - pad, hi + pad);
    let ys = |v: f32| y1 - (v - lo) / (hi - lo) * (y1 - y0);
    let m = pts.len().max(2) as f32 - 1.0;
    let xs = |i: usize| x0 + lh * 0.4 + i as f32 / m * (x1 - x0 - lh * 0.8);
    fill(frame, x0, y0, x0 + 1.0, y1, hint);
    fill(frame, x0, y1, x1, y1 + 1.0, hint);
    for t in ticks(lo, hi, 3) {
        let y = ys(t);
        fill(frame, x0 - adv * 0.5, y, x0, y + 1.0, hint);
        let s = tick_text(t);
        let tw = font.width(&s) as f32;
        text::draw(
            frame,
            font,
            &s,
            (x0 - adv * 0.8 - tw) as i32,
            (y - lh / 2.0) as i32,
            hint,
            bg,
        );
    }
    let xl = "individuals";
    let xw = font.width(xl) as f32;
    text::draw(
        frame,
        font,
        xl,
        ((x0 + x1 - xw) / 2.0) as i32,
        (y1 + lh * 0.4) as i32,
        ink,
        bg,
    );
    let p = p_text(inset.p);
    if !p.is_empty() {
        text::draw(
            frame,
            font,
            &p,
            (x0 + lh * 0.5) as i32,
            y0 as i32,
            to_linear(PVALUE),
            bg,
        );
    }
    for (i, pt) in pts.iter().enumerate() {
        let x = xs(i);
        if pt.sd.is_finite() {
            fill(
                frame,
                x - 0.5,
                ys(pt.y + pt.sd),
                x + 0.5,
                ys(pt.y - pt.sd),
                to_linear([150, 150, 150]),
            );
        }
    }
    for (i, pt) in pts.iter().enumerate() {
        dots.dot(frame, xs(i), ys(pt.y), dots.radius(pt.count), pt.level);
    }
}

/// The insets and the levels' names, or why they could not be read.
pub type Read = Result<(Vec<Inset>, Vec<String>), String>;

/// The insets read on a thread: what it sends once done. Dropping it (a
/// newer read in its place, or nobody waiting) stops the thread.
pub struct Reading {
    rx: std::sync::mpsc::Receiver<Read>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Reading {
    /// [`read_insets`] of `wanted` at `prefix`, on a thread.
    pub fn start(
        prefix: std::path::PathBuf,
        wanted: Vec<(String, String, f32)>,
        psi: impl Fn(&str, &str, usize) -> f32 + Send + 'static,
    ) -> Self {
        let stop = std::sync::Arc::<std::sync::atomic::AtomicBool>::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let flag = stop.clone();
        std::thread::spawn(move || {
            let read = read_insets(&prefix, &wanted, psi, &flag);
            let _ = tx.send(read.map_err(|e| format!("{e:#}")));
        });
        Reading { rx, stop }
    }

    /// The read, once it is done.
    pub fn try_recv(&self) -> Result<Read, std::sync::mpsc::TryRecvError> {
        self.rx.try_recv()
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The row groups of a delta table (its footer, `meta`) that may hold one of `genes`:
/// those whose gene column's least and greatest names bracket one. A group
/// without those statistics (a table written in one group) may hold any.
pub fn groups_holding(
    meta: &parquet::file::metadata::ParquetMetaData,
    genes: &std::collections::HashSet<&str>,
) -> Vec<usize> {
    use parquet::file::statistics::Statistics;
    let holds = |g: &parquet::file::metadata::RowGroupMetaData| match g.column(0).statistics() {
        Some(Statistics::ByteArray(s)) => match (s.min_opt(), s.max_opt()) {
            (Some(lo), Some(hi)) => genes
                .iter()
                .any(|g| (lo.data()..=hi.data()).contains(&g.as_bytes())),
            _ => true,
        },
        _ => true,
    };
    meta.row_groups()
        .iter()
        .enumerate()
        .filter(|(_, g)| holds(g))
        .map(|(k, _)| k)
        .collect()
}

/// The run's exposure levels in its order, the reference first: the
/// `tau_<level>` columns of its effect table.
fn run_levels(prefix: &std::path::Path) -> anyhow::Result<Vec<String>> {
    use legume_numeric::matrix::table::TableReader;
    let file = super::results::table(prefix, "effect");
    let t = TableReader::open(&file.to_string_lossy())?;
    let levels: Vec<String> = t
        .header()
        .iter()
        .filter_map(|h| h.strip_prefix("tau_").map(str::to_string))
        .collect();
    anyhow::ensure!(!levels.is_empty(), "{}: no tau columns", file.display());
    Ok(levels)
}

/// Each wanted gene's individuals in its topic, read from
/// `{prefix}.delta.parquet` with the log effect `psi` of each level
/// (`psi(gene, topic, level)`, 0 for the reference): the inset points, and
/// the levels' names in the run's order, the reference first. An
/// individual at a level the run does not have is left out. Once `stop` is
/// set (a newer read wanted), it ends early with an error.
pub fn read_insets(
    prefix: &std::path::Path,
    wanted: &[(String, String, f32)],
    psi: impl Fn(&str, &str, usize) -> f32,
    stop: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<(Vec<Inset>, Vec<String>)> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::RowAccessor;
    let stopped = || stop.load(std::sync::atomic::Ordering::Relaxed);
    anyhow::ensure!(!stopped(), "a newer read replaced this one");
    let file = super::results::table(prefix, "delta");
    let reader = SerializedFileReader::new(std::fs::File::open(&file)?)?;
    let schema = reader.metadata().file_metadata().schema_descr_ptr();
    let col = |n: &str| (0..schema.num_columns()).find(|&j| schema.column(j).name() == n);
    let need = |n: &str| col(n).ok_or_else(|| anyhow::anyhow!("{}: no {n} column", file.display()));
    let (gc, ic, ec) = (0, need("individual")?, need("individual_exposure")?);
    let (tc, dc) = (need("topic")?, need("delta")?);
    let cc = col("count");
    let index: HashMap<(&str, &str), usize> = wanted
        .iter()
        .enumerate()
        .map(|(k, (g, t, _))| ((g.as_str(), t.as_str()), k))
        .collect();
    let levels = run_levels(prefix)?;
    let genes: std::collections::HashSet<&str> =
        wanted.iter().map(|(g, _, _)| g.as_str()).collect();
    let mut raw: Vec<Vec<(String, f32, f32)>> = vec![Vec::new(); wanted.len()];
    for g in groups_holding(reader.metadata(), &genes) {
        for (n, row) in reader.get_row_group(g)?.get_row_iter(None)?.enumerate() {
            if n % 4096 == 0 {
                anyhow::ensure!(!stopped(), "a newer read replaced this one");
            }
            let row = row?;
            let key = (row.get_string(gc)?.as_str(), row.get_string(tc)?.as_str());
            let Some(&k) = index.get(&key) else { continue };
            // `individual_exposure` is `{individual}_{exposure}`.
            let whole = row.get_string(ec)?;
            let exposure = whole
                .strip_prefix(&format!("{}_", row.get_string(ic)?))
                .unwrap_or(whole)
                .to_string();
            let delta = row.get_float(dc).unwrap_or(f32::NAN);
            let count = cc.map_or(f32::NAN, |c| row.get_float(c).unwrap_or(f32::NAN));
            raw[k].push((exposure, delta, count));
        }
    }
    let insets = wanted
        .iter()
        .zip(raw)
        .map(|((gene, topic, p), rows)| Inset {
            gene: gene.clone(),
            topic: topic.clone(),
            p: *p,
            points: rows
                .into_iter()
                .filter(|(_, d, _)| *d > 0.0)
                .filter_map(|(e, d, c)| {
                    let level = levels.iter().position(|l| *l == e)?;
                    let shift = if level == 0 {
                        0.0
                    } else {
                        psi(gene, topic, level)
                    };
                    Some(Point {
                        y: d.ln() + shift,
                        sd: if c.is_finite() {
                            1.0 / (c + 1.0).sqrt()
                        } else {
                            f32::NAN
                        },
                        count: c,
                        level,
                    })
                })
                .collect(),
        })
        .collect();
    Ok((insets, levels))
}

#[cfg(test)]
#[path = "tests/figure.rs"]
mod tests;
