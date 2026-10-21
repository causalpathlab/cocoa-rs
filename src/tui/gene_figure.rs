//! One gene's page, as faba's pileup is one gene's: its effect in each cell
//! type (topic) beside its average, its neighbours on the genome with their
//! average effects, and its individuals in each cell type, sorted, the
//! reference level hollow and the others filled, sized by count.

use super::annotation::Genome;
use super::figure::{fill, individuals, p_text, shown_name, Dots, Inset, THEME};
use super::results::Results;
use legume_tui::map::colour::{to_linear, Lin};
use legume_tui::map::labels::{place, Label};
use legume_tui::map::offsets::Offsets;
use legume_tui::map::raster::{ring, stamp, Frame, Shape};
use legume_tui::map::text::{self, Font};

/// Neighbours named on the locus at most.
const NAMED: usize = 8;

/// What the page shows.
pub struct GeneFigure<'a> {
    pub results: &'a Results,
    pub genome: Option<&'a Genome>,
    /// The gene, by its index in the results.
    pub gene: usize,
    /// Its individuals in each topic, in the run's topic order.
    pub facets: &'a [Inset],
    /// The exposure levels' names, the reference first.
    pub levels: &'a [String],
    /// Half the locus shown, in bases.
    pub flank: u64,
}

/// `v` in a few significant digits.
fn num(v: f32) -> String {
    if v.is_finite() {
        format!("{v:.3}")
    } else {
        "NA".into()
    }
}

/// Where the gene is: its chromosome (name, start along the genome,
/// length) and its place on it.
fn locus(genome: &Genome, gene: usize) -> Option<((String, u64, u64), u64)> {
    let at = (*genome.at.get(gene)?)?;
    let c = genome
        .chroms
        .iter()
        .rev()
        .find(|(_, off, _)| *off <= at)?
        .clone();
    let on = at - c.1;
    Some((c, on))
}

impl GeneFigure<'_> {
    /// The gene's line over the page: its names, place and numbers.
    #[must_use]
    pub fn headline(&self) -> String {
        let r = self.results;
        let g = &r.genes[self.gene];
        let l = r.level;
        let mut parts = vec![shown_name(&g.name).to_string()];
        if shown_name(&g.name) != g.name {
            parts.push(g.name.split('_').next().unwrap_or("").to_string());
        }
        if let Some(((chrom, _, _), on)) = self.genome.and_then(|gn| locus(gn, self.gene)) {
            parts.push(format!("chr{chrom}:{on}"));
        }
        parts.push(format!("contrast {}", num(g.contrast[l])));
        if g.z[l].is_finite() {
            parts.push(format!("z {}", num(g.z[l])));
            parts.push(format!("p {}", p_text(g.p[l])));
        }
        if r.truth.as_ref().is_some_and(|t| t.genes.contains(&g.name)) {
            parts.push("causal".into());
        }
        if !g.flag.is_empty() {
            parts.push(format!("left out: {}", g.flag));
        }
        parts.join(" · ")
    }

    pub fn draw(&self, frame: &mut Frame, font: Font, bold: Font) {
        let lh = font.line_height() as f32;
        let (w, h) = (frame.w as f32, frame.h as f32);
        let bg = frame.background();
        text::draw(
            frame,
            bold,
            &self.headline(),
            lh as i32,
            (lh * 0.4) as i32,
            to_linear(THEME.text),
            bg,
        );
        let split = (h * 0.4).max(lh * 10.0).min(h * 0.6);
        let top = lh * 1.8;
        let mid = w * 0.38;
        self.draw_topics(frame, [0.0, top, mid, split], font, bold);
        self.draw_locus(frame, [mid, top, w, split], font, bold);
        self.draw_facets(frame, [0.0, split, w, h], font, bold);
    }

    /// The gene's log effect in each topic, a lollipop each, under its
    /// average.
    fn draw_topics(&self, frame: &mut Frame, area: [f32; 4], font: Font, bold: Font) {
        let r = self.results;
        let lh = font.line_height() as f32;
        let adv = font.advance() as f32;
        let bg = frame.background();
        let (ink, hint) = (to_linear(THEME.text), to_linear(THEME.hint));
        let g = &r.genes[self.gene];
        let topics = r.topics();
        let row = r.topic_effects(&g.name).cloned().unwrap_or_default();
        let avg = g.contrast[r.level];
        text::draw(
            frame,
            bold,
            "effect by cell type",
            (area[0] + lh) as i32,
            area[1] as i32,
            ink,
            bg,
        );
        let name_w = topics
            .iter()
            .map(|t| font.width(t))
            .chain([font.width("average")])
            .max()
            .unwrap_or(0) as f32;
        let (x0, x1) = (area[0] + lh + name_w + adv, area[2] - lh * 1.5);
        let (y0, y1) = (area[1] + lh * 1.6, area[3] - lh * 1.8);
        let n = topics.len() + 1;
        if x1 - x0 < lh * 3.0 || y1 - y0 < lh * n as f32 * 0.6 {
            return;
        }
        let lim = row
            .iter()
            .chain([&avg])
            .filter(|v| v.is_finite())
            .fold(0f32, |m, v| m.max(v.abs()))
            .max(0.05)
            * 1.15;
        let xs = |v: f32| x0 + (v + lim) / (2.0 * lim) * (x1 - x0);
        let step = (y1 - y0) / n as f32;
        let yc = |k: usize| y0 + step * (k as f32 + 0.5);
        fill(frame, xs(0.0), y0, xs(0.0) + 1.0, y1, hint);
        fill(frame, x0, y1, x1, y1 + 1.0, hint);
        for t in [-lim / 1.15, 0.0, lim / 1.15] {
            let s = format!("{t:.2}");
            let tw = font.width(&s) as f32;
            text::draw(
                frame,
                font,
                &s,
                (xs(t) - tw / 2.0) as i32,
                (y1 + lh * 0.2) as i32,
                hint,
                bg,
            );
        }
        let label = |frame: &mut Frame, k: usize, s: &str, c: Lin| {
            let tw = font.width(s) as f32;
            text::draw(
                frame,
                font,
                s,
                (x0 - adv - tw) as i32,
                (yc(k) - lh / 2.0) as i32,
                c,
                bg,
            );
        };
        label(frame, 0, "average", ink);
        if avg.is_finite() {
            fill(
                frame,
                xs(0.0).min(xs(avg)),
                yc(0) - 1.0,
                xs(0.0).max(xs(avg)),
                yc(0) + 1.0,
                ink,
            );
            stamp(frame, xs(avg), yc(0), lh * 0.32, Shape::Diamond, ink, 1.0);
        }
        for (k, t) in topics.iter().enumerate() {
            let c = THEME.palette.colour(k, topics.len());
            label(frame, k + 1, t, ink);
            let v = row.get(k).copied().unwrap_or(f32::NAN);
            if v.is_finite() {
                let (a, b) = (xs(0.0).min(xs(v)), xs(0.0).max(xs(v)));
                fill(frame, a, yc(k + 1) - 1.0, b, yc(k + 1) + 1.0, c);
                stamp(frame, xs(v), yc(k + 1), lh * 0.28, Shape::Circle, c, 1.0);
            } else {
                text::draw(
                    frame,
                    font,
                    "left out",
                    (xs(0.0) + adv) as i32,
                    (yc(k + 1) - lh / 2.0) as i32,
                    hint,
                    bg,
                );
            }
        }
    }

    /// The genes around it, `flank` each way, at their average effects.
    fn draw_locus(&self, frame: &mut Frame, area: [f32; 4], font: Font, bold: Font) {
        let r = self.results;
        let lh = font.line_height() as f32;
        let adv = font.advance() as f32;
        let bg = frame.background();
        let (ink, hint, muted) = (
            to_linear(THEME.text),
            to_linear(THEME.hint),
            to_linear(THEME.muted),
        );
        let Some(genome) = self.genome else {
            text::draw(
                frame,
                bold,
                "locus",
                (area[0] + lh) as i32,
                area[1] as i32,
                ink,
                bg,
            );
            let note = "g picks an annotation to place the gene among its neighbours";
            text::draw(
                frame,
                font,
                note,
                (area[0] + lh) as i32,
                (area[1] + lh * 2.0) as i32,
                hint,
                bg,
            );
            return;
        };
        let Some(((chrom, off, len), on)) = locus(genome, self.gene) else {
            text::draw(
                frame,
                bold,
                "locus",
                (area[0] + lh) as i32,
                area[1] as i32,
                ink,
                bg,
            );
            let note = "the annotation does not place this gene";
            text::draw(
                frame,
                font,
                note,
                (area[0] + lh) as i32,
                (area[1] + lh * 2.0) as i32,
                hint,
                bg,
            );
            return;
        };
        let (lo, hi) = (on.saturating_sub(self.flank), (on + self.flank).min(len));
        let mb = |b: u64| b as f32 / 1e6;
        let (x0, x1) = (area[0] + adv * 7.0, area[2] - lh);
        let (y0, y1) = (area[1] + lh * 1.6, area[3] - lh * 1.8);
        let l = r.level;
        let near: Vec<(usize, u64, f32)> = genome
            .at
            .iter()
            .enumerate()
            .filter_map(|(g, at)| {
                let at = (*at)?;
                let p = at.checked_sub(off)?;
                let v = r.genes.get(g)?.contrast[l];
                (at < off + len && p >= lo && p <= hi && v.is_finite()).then_some((g, p, v))
            })
            .collect();
        let others = near.len().saturating_sub(1);
        let title = format!(
            "chr{chrom}:{:.2}–{:.2} Mb · {others} other gene{} of the data",
            mb(lo),
            mb(hi),
            if others == 1 { "" } else { "s" }
        );
        text::draw(
            frame,
            bold,
            &title,
            (area[0] + lh) as i32,
            area[1] as i32,
            ink,
            bg,
        );
        if x1 - x0 < lh * 4.0 || y1 - y0 < lh * 3.0 {
            return;
        }
        let lim = near.iter().fold(0f32, |m, n| m.max(n.2.abs())).max(0.05) * 1.15;
        let xs = |p: u64| x0 + (p - lo) as f32 / (hi - lo).max(1) as f32 * (x1 - x0);
        let ys = |v: f32| y1 - (v + lim) / (2.0 * lim) * (y1 - y0);
        fill(frame, x0, y0, x0 + 1.0, y1, hint);
        fill(frame, x0, y1, x1, y1 + 1.0, hint);
        fill(frame, x0, ys(0.0), x1, ys(0.0) + 1.0, muted);
        for t in [-lim / 1.15, 0.0, lim / 1.15] {
            let s = format!("{t:.2}");
            let tw = font.width(&s) as f32;
            text::draw(
                frame,
                font,
                &s,
                (x0 - adv * 0.8 - tw) as i32,
                (ys(t) - lh / 2.0) as i32,
                hint,
                bg,
            );
        }
        for k in 0..=4u64 {
            let p = lo + (hi - lo) * k / 4;
            let s = format!("{:.2}", mb(p));
            let tw = font.width(&s) as f32;
            fill(frame, xs(p), y1, xs(p) + 1.0, y1 + lh * 0.3, hint);
            text::draw(
                frame,
                font,
                &s,
                (xs(p) - tw / 2.0) as i32,
                (y1 + lh * 0.35) as i32,
                hint,
                bg,
            );
        }
        let called: std::collections::HashSet<usize> = r.called().into_iter().collect();
        let topics = r.topics();
        let mut labels: Vec<Label<usize>> = Vec::new();
        let mut by_size: Vec<&(usize, u64, f32)> =
            near.iter().filter(|n| called.contains(&n.0)).collect();
        by_size.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()));
        let named: Vec<usize> = by_size.iter().take(NAMED).map(|n| n.0).collect();
        for &(g, p, v) in &near {
            let (x, y) = (xs(p), ys(v));
            let name = &r.genes[g].name;
            let colour = if called.contains(&g) {
                r.strongest_topic(name)
                    .and_then(|t| topics.iter().position(|x| *x == t))
                    .map_or(ink, |k| THEME.palette.colour(k, topics.len()))
            } else {
                muted
            };
            let big = called.contains(&g) || g == self.gene;
            stamp(
                frame,
                x,
                y,
                lh * if big { 0.26 } else { 0.16 },
                Shape::Circle,
                colour,
                1.0,
            );
            if g == self.gene || named.contains(&g) {
                if g == self.gene {
                    ring(frame, x, y, lh * 0.3, ink);
                }
                labels.push(Label {
                    text: shown_name(name).to_string(),
                    x,
                    y: y - lh * 0.9 * v.signum(),
                    ink,
                    tier: u8::from(g == self.gene),
                    priority: v.abs(),
                    font: if g == self.gene { bold } else { font },
                    key: Some(g),
                });
            }
        }
        let _ = place(frame, labels, &[], &Offsets::default());
    }

    /// Its individuals in each topic, a panel each, and the key.
    fn draw_facets(&self, frame: &mut Frame, area: [f32; 4], font: Font, bold: Font) {
        let lh = font.line_height() as f32;
        let adv = font.advance() as f32;
        let n = self.facets.len();
        if n == 0 {
            return;
        }
        let key_w = adv * 12.0;
        let cols = n.min(5);
        let rows = n.div_ceil(cols);
        let each_w = (area[2] - area[0] - key_w - lh) / cols as f32;
        let each_h = (area[3] - area[1]) / rows as f32;
        let dots = Dots::new(self.levels, self.facets, lh);
        let topics = self.results.topics();
        for (k, facet) in self.facets.iter().enumerate() {
            let (c, rr) = (k % cols, k / cols);
            let left = area[0] + lh * 0.5 + c as f32 * each_w;
            let top = area[1] + rr as f32 * each_h;
            let v = self
                .results
                .effect_at(&facet.gene, &facet.topic, self.results.level);
            let sub = format!("log effect {}", num(v));
            let title = facet.topic.clone();
            let panel = [left, top, left + each_w, top + each_h];
            individuals(frame, panel, facet, [&title, &sub], &dots, font, bold);
            // The topic's colour under its name, as on the genome.
            let k = topics.iter().position(|t| *t == facet.topic).unwrap_or(k);
            let tw = bold.width(&title) as f32;
            let x = left + (each_w + tw) / 2.0 + lh * 0.4;
            stamp(
                frame,
                x,
                top + lh * 0.8,
                lh * 0.22,
                Shape::Circle,
                THEME.palette.colour(k, topics.len()),
                1.0,
            );
        }
        dots.key(frame, area[2] - key_w, area[1] + lh * 2.6, font);
    }
}

#[cfg(test)]
#[path = "tests/gene_figure.rs"]
mod tests;
