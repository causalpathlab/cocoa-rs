//! A finished diff read back: each gene's contrast per exposure level,
//! adjusted and not, with its permutation z and p-value when there are
//! some; scored against a simulation's causal genes when its truth is
//! known; and the top genes × topics of the log effects as a heatmap, on
//! screen and as a PDF.

use legume_numeric::matrix::table::TableReader;
use legume_tui::chart::heatmap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Genes per topic the heatmap shows unless asked for another number.
pub const PER_TOPIC: usize = 5;

/// What the gene table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// |z| from the permutations, else |contrast|.
    Score,
    Contrast,
    Unadjusted,
    Name,
}

impl Sort {
    #[must_use]
    pub fn next(self) -> Sort {
        match self {
            Sort::Score => Sort::Contrast,
            Sort::Contrast => Sort::Unadjusted,
            Sort::Unadjusted => Sort::Name,
            Sort::Name => Sort::Score,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Sort::Score => "|z|",
            Sort::Contrast => "|contrast|",
            Sort::Unadjusted => "|unadjusted|",
            Sort::Name => "name",
        }
    }
}

/// One gene, a value per contrast level (NaN where left out).
#[derive(Clone, Debug)]
pub struct Gene {
    pub name: String,
    pub contrast: Vec<f32>,
    pub unadjusted: Vec<f32>,
    pub z: Vec<f32>,
    pub p: Vec<f32>,
    pub flag: String,
}

/// How well a ranking finds the causal genes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Score {
    pub auroc: Option<f64>,
    /// At q ≤ [`Q`] (Benjamini–Hochberg on the permutation p-values): how
    /// many were called, the share of causal genes among them found, and
    /// the share of them that are not causal.
    pub called: Option<(usize, f64, f64)>,
}

/// The FDR level the truth is scored at.
pub const Q: f64 = 0.1;

/// The causal genes of a simulation, by name.
#[derive(Clone, Debug)]
pub struct Truth {
    pub file: PathBuf,
    pub genes: HashSet<String>,
}

impl Truth {
    /// `{sim}.causal.gz`: a causal gene and its exposure level a line.
    pub fn read(file: &Path) -> anyhow::Result<Self> {
        let lines = legume_numeric::matrix::common_io::read_lines(&file.to_string_lossy())?;
        let genes = lines
            .iter()
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect();
        Ok(Truth {
            file: file.to_path_buf(),
            genes,
        })
    }
}

/// The heatmap: top genes × topics, the log effect of one level.
#[derive(Clone, Debug)]
pub struct Heat {
    pub genes: Vec<String>,
    pub topics: Vec<String>,
    /// Row-major, `genes × topics`; NaN where left out.
    pub v: Vec<f32>,
    /// The |value| the colours saturate at.
    pub clip: f32,
}

/// A diff read back.
pub struct Results {
    pub prefix: PathBuf,
    /// The contrast levels' column suffixes: `""` for one contrast
    /// (`contrast`), else each level (`contrast_b`).
    pub levels: Vec<String>,
    pub level: usize,
    pub genes: Vec<Gene>,
    /// Indices into `genes`, in the order shown.
    pub order: Vec<usize>,
    pub sort: Sort,
    pub at: usize,
    pub truth: Option<Truth>,
    /// Genes left out, by why.
    pub left_out: Vec<(String, usize)>,
    /// The log effects per (gene, topic), for the heatmap: topics, and
    /// each gene's row of values per level.
    topics: Vec<String>,
    effects: Effects,
    pub per_topic: usize,
    pub heat: Option<Heat>,
}

/// A column's values as numbers.
fn number(s: &str) -> f32 {
    s.parse().unwrap_or(f32::NAN)
}

/// `{prefix}.{table}.parquet`.
#[must_use]
pub fn table(prefix: &Path, table: &str) -> PathBuf {
    crate::runs::suffixed(prefix, &format!(".{table}.parquet"))
}

/// Whether a diff finished at `prefix`.
#[must_use]
pub fn is_diff(prefix: &Path) -> bool {
    table(prefix, "contrast").is_file()
}

impl Results {
    /// The diff at `prefix`, scored against `truth` when given.
    pub fn read(prefix: &Path, truth: Option<&Path>) -> anyhow::Result<Self> {
        let contrast = TableReader::open(&table(prefix, "contrast").to_string_lossy())?;
        let header: Vec<String> = contrast.header().iter().map(|h| h.to_string()).collect();
        let levels: Vec<String> = header
            .iter()
            .filter(|h| h.starts_with("contrast") && !h.ends_with("_unadjusted"))
            .map(|h| h.strip_prefix("contrast").unwrap_or("").to_string())
            .collect();
        anyhow::ensure!(!levels.is_empty(), "{}: no contrast", prefix.display());
        let col = |name: &str| header.iter().position(|h| h == name);
        let name_col = 0;
        let contrast_cols: Vec<Option<usize>> = levels
            .iter()
            .map(|l| col(&format!("contrast{l}")))
            .collect();
        let unadj_cols: Vec<Option<usize>> = levels
            .iter()
            .map(|l| col(&format!("contrast{l}_unadjusted")))
            .collect();
        let flag_col = col("flag");
        let all: Vec<usize> = (0..header.len()).collect();
        let at = |row: &[Box<str>], c: Option<usize>| c.map_or(f32::NAN, |c| number(&row[c]));
        let mut genes = Vec::new();
        for row in contrast.rows(&all)? {
            let row = row?;
            genes.push(Gene {
                name: row[name_col].to_string(),
                contrast: contrast_cols.iter().map(|c| at(&row, *c)).collect(),
                unadjusted: unadj_cols.iter().map(|c| at(&row, *c)).collect(),
                z: vec![f32::NAN; levels.len()],
                p: vec![f32::NAN; levels.len()],
                flag: flag_col.map_or_else(String::new, |c| row[c].to_string()),
            });
        }
        let perm = table(prefix, "perm");
        if perm.is_file() {
            let t = TableReader::open(&perm.to_string_lossy())?;
            let h: Vec<String> = t.header().iter().map(|h| h.to_string()).collect();
            let col = |name: String| h.iter().position(|x| *x == name);
            let zc: Vec<Option<usize>> =
                levels.iter().map(|l| col(format!("z_score{l}"))).collect();
            let pc: Vec<Option<usize>> = levels.iter().map(|l| col(format!("pvalue{l}"))).collect();
            let index: HashMap<String, usize> = genes
                .iter()
                .enumerate()
                .map(|(i, g)| (g.name.clone(), i))
                .collect();
            let all: Vec<usize> = (0..h.len()).collect();
            for row in t.rows(&all)? {
                let row = row?;
                if let Some(&i) = index.get(row[0].as_ref()) {
                    genes[i].z = zc.iter().map(|c| at(&row, *c)).collect();
                    genes[i].p = pc.iter().map(|c| at(&row, *c)).collect();
                }
            }
        }
        let mut left: HashMap<String, usize> = HashMap::new();
        for g in genes.iter().filter(|g| !g.flag.is_empty()) {
            *left.entry(g.flag.clone()).or_default() += 1;
        }
        let mut left_out: Vec<(String, usize)> = left.into_iter().collect();
        left_out.sort();
        let (topics, effects) = read_effects(prefix, &levels).unwrap_or_default();
        let truth = truth.map(Truth::read).transpose()?;
        let mut r = Results {
            prefix: prefix.to_path_buf(),
            levels,
            level: 0,
            genes,
            order: Vec::new(),
            sort: Sort::Score,
            at: 0,
            truth,
            left_out,
            topics,
            effects,
            per_topic: PER_TOPIC,
            heat: None,
        };
        r.resort();
        Ok(r)
    }

    /// Whether the permutations ran: z and p-values to show.
    #[must_use]
    pub fn has_perm(&self) -> bool {
        self.genes.iter().any(|g| g.z.iter().any(|z| z.is_finite()))
    }

    /// The topics (cell types) the effects are in, in the run's order.
    #[must_use]
    pub fn topics(&self) -> &[String] {
        &self.topics
    }

    /// Gene `name`'s log effect in each topic, at the level shown.
    #[must_use]
    pub fn topic_effects(&self, name: &str) -> Option<&Vec<f32>> {
        self.effects.get(name).and_then(|l| l.get(self.level))
    }

    /// The log effect of `name` in `topic` at contrast level `l` (0 the
    /// first non-reference level).
    #[must_use]
    pub fn effect_at(&self, name: &str, topic: &str, l: usize) -> f32 {
        let k = self.topics.iter().position(|t| t == topic);
        k.and_then(|k| Some(*self.effects.get(name)?.get(l)?.get(k)?))
            .unwrap_or(f32::NAN)
    }

    /// The genes called on the level shown, best first: at q ≤ [`Q`]
    /// (Benjamini–Hochberg on the permutation p-values) when there are
    /// some, else the 50 of largest |contrast|.
    #[must_use]
    pub fn called(&self) -> Vec<usize> {
        let l = self.level;
        if self.has_perm() {
            let mut p: Vec<(usize, f32)> = self
                .genes
                .iter()
                .enumerate()
                .filter(|(_, g)| g.p[l].is_finite())
                .map(|(i, g)| (i, g.p[l]))
                .collect();
            p.sort_by(|a, b| a.1.total_cmp(&b.1));
            let m = p.len() as f64;
            let k = p
                .iter()
                .enumerate()
                .rev()
                .find(|(i, (_, pv))| f64::from(*pv) <= Q * (*i as f64 + 1.0) / m)
                .map_or(0, |(i, _)| i + 1);
            let mut out: Vec<usize> = p[..k].iter().map(|(i, _)| *i).collect();
            out.sort_by(|&a, &b| {
                let z = |g: usize| self.genes[g].z[l].abs();
                z(b).total_cmp(&z(a))
            });
            return out;
        }
        let mut out: Vec<usize> = (0..self.genes.len())
            .filter(|&g| self.genes[g].contrast[l].is_finite())
            .collect();
        out.sort_by(|&a, &b| {
            let c = |g: usize| self.genes[g].contrast[l].abs();
            c(b).total_cmp(&c(a))
        });
        out.truncate(50);
        out
    }

    /// The strongest topic of gene `name` at the level shown.
    #[must_use]
    pub fn strongest_topic(&self, name: &str) -> Option<String> {
        let row = self.topic_effects(name)?;
        let k = row
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?
            .0;
        self.topics.get(k).cloned()
    }

    /// Whether the run wrote the contrast without its adjustment (it does
    /// with a propensity: covariates, control genes or PCs).
    #[must_use]
    pub fn has_unadjusted(&self) -> bool {
        self.genes
            .iter()
            .any(|g| g.unadjusted.iter().any(|u| u.is_finite()))
    }

    /// The level shown, as a header names it: `contrast` or `contrast_b`.
    #[must_use]
    pub fn level_name(&self) -> String {
        match self.levels[self.level].as_str() {
            "" => "the contrast".into(),
            l => format!("level {}", l.trim_start_matches('_')),
        }
    }

    /// A gene's score by `sort`; NaN when it has none, sorted last.
    fn key(&self, g: &Gene, sort: Sort) -> f32 {
        let l = self.level;
        match sort {
            Sort::Score if g.z[l].is_finite() => g.z[l].abs(),
            Sort::Score | Sort::Contrast => g.contrast[l].abs(),
            Sort::Unadjusted => g.unadjusted[l].abs(),
            Sort::Name => 0.0,
        }
    }

    /// Sort again, the heatmap made again, the cursor on top.
    pub fn resort(&mut self) {
        let mut order: Vec<usize> = (0..self.genes.len()).collect();
        if self.sort == Sort::Name {
            order.sort_by(|&a, &b| natural(&self.genes[a].name, &self.genes[b].name));
        } else {
            let keys: Vec<f32> = self.genes.iter().map(|g| self.key(g, self.sort)).collect();
            order.sort_by(|&a, &b| {
                let (x, y) = (keys[a], keys[b]);
                x.is_nan().cmp(&y.is_nan()).then(y.total_cmp(&x))
            });
        }
        self.order = order;
        self.at = 0;
        self.heat = self.make_heat();
    }

    pub fn next_level(&mut self) {
        self.level = (self.level + 1) % self.levels.len();
        self.resort();
    }

    pub fn next_sort(&mut self) {
        self.sort = self.sort.next();
        self.resort();
    }

    /// More or fewer genes per topic on the heatmap.
    pub fn per_topic_by(&mut self, d: isize) {
        self.per_topic = self.per_topic.saturating_add_signed(d).clamp(1, 50);
        self.heat = self.make_heat();
    }

    /// The truth scored on the level shown: the adjusted ranking, and the
    /// unadjusted one when the run wrote it.
    #[must_use]
    pub fn score(&self) -> Option<(Score, Option<Score>)> {
        let truth = self.truth.as_ref()?;
        let l = self.level;
        let tested: Vec<&Gene> = self
            .genes
            .iter()
            .filter(|g| g.contrast[l].is_finite())
            .collect();
        let causal = |g: &&Gene| truth.genes.contains(&g.name);
        let ranked = |key: &dyn Fn(&Gene) -> f32| -> Option<f64> {
            let scored: Vec<(f32, bool)> = tested
                .iter()
                .filter(|g| key(g).is_finite())
                .map(|g| (key(g), causal(g)))
                .collect();
            auroc(&scored)
        };
        let adjusted = Score {
            auroc: ranked(&|g: &Gene| {
                if g.z[l].is_finite() {
                    g.z[l].abs()
                } else {
                    g.contrast[l].abs()
                }
            }),
            called: {
                let p: Vec<(f64, bool)> = tested
                    .iter()
                    .filter(|g| g.p[l].is_finite())
                    .map(|g| (f64::from(g.p[l]), causal(g)))
                    .collect();
                (!p.is_empty()).then(|| called_at(&p, Q, truth.genes.len()))
            },
        };
        let unadjusted = self.has_unadjusted().then(|| Score {
            auroc: ranked(&|g: &Gene| g.unadjusted[l].abs()),
            called: None,
        });
        Some((adjusted, unadjusted))
    }

    /// The z of the genes that are not causal, binned over [-4, 4] in
    /// `bins`; and their standard deviation (1 when calibrated).
    #[must_use]
    pub fn null_z(&self, bins: usize) -> Option<(Vec<usize>, f64)> {
        let l = self.level;
        let truth = self.truth.as_ref();
        let z: Vec<f64> = self
            .genes
            .iter()
            .filter(|g| truth.is_none_or(|t| !t.genes.contains(&g.name)))
            .map(|g| f64::from(g.z[l]))
            .filter(|z| z.is_finite())
            .collect();
        if z.len() < 2 || bins == 0 {
            return None;
        }
        let mut hist = vec![0; bins];
        for v in &z {
            let b = ((v + 4.0) / 8.0 * bins as f64).floor();
            hist[(b.max(0.0) as usize).min(bins - 1)] += 1;
        }
        let mean = z.iter().sum::<f64>() / z.len() as f64;
        let var = z.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (z.len() - 1) as f64;
        Some((hist, var.sqrt()))
    }

    /// The top genes of each topic by the margin of their |log effect|
    /// there over the next topic (legume-tui's heatmap ranking), each
    /// shown with its signed log effect.
    fn make_heat(&self) -> Option<Heat> {
        let l = self.level;
        let mut names: Vec<&String> = self.effects.keys().collect();
        names.sort_by(|a, b| natural(a, b));
        let (rows, cols) = (names.len(), self.topics.len());
        if rows == 0 || cols == 0 {
            return None;
        }
        let signed: Vec<f32> = names
            .iter()
            .flat_map(|g| self.effects[*g][l].iter().copied())
            .collect();
        let size: Vec<f32> = signed.iter().map(|v| v.abs()).collect();
        let m = heatmap::Matrix {
            rows,
            cols,
            v: &size,
        };
        let mut picked: Vec<usize> = Vec::new();
        for (r, _) in heatmap::by_margin(&m, self.per_topic) {
            if !picked.contains(&r) {
                picked.push(r);
            }
        }
        if picked.is_empty() {
            return None;
        }
        let v: Vec<f32> = picked
            .iter()
            .flat_map(|&r| signed[r * cols..(r + 1) * cols].iter().copied())
            .collect();
        let mut mags: Vec<f32> = v
            .iter()
            .filter(|x| x.is_finite())
            .map(|x| x.abs())
            .collect();
        mags.sort_by(f32::total_cmp);
        let clip = mags
            .get(mags.len().saturating_sub(1) * 95 / 100)
            .map(|c| (c * 100.0).ceil() / 100.0)
            .filter(|c| *c > 0.0)
            .unwrap_or(1.0);
        Some(Heat {
            genes: picked.iter().map(|&r| names[r].clone()).collect(),
            topics: self.topics.clone(),
            v,
            clip,
        })
    }

    /// The heatmap as a PDF page at `path`, `page` wide.
    pub fn save_heat(&self, path: &Path, page: legume_tui::export::PageSize) -> anyhow::Result<()> {
        use legume_tui::chart::heatmap::{draw_heat, heat_size, HeatStyle};
        use legume_tui::export::{write_pdf, Page};
        use legume_tui::map::theme::Theme;
        let h = self
            .heat
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no heatmap to save"))?;
        let theme = Theme::PAPER;
        let title = format!(
            "{} · log effect of {} · top {} genes a topic",
            self.prefix
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            self.level_name(),
            self.per_topic
        );
        let heat = heatmap::Heat {
            title: &title,
            rows: &h.genes,
            cols: &h.topics,
            col_swatch: None,
            z: &h.v,
            clip: h.clip,
            bold_row: None,
        };
        let style = HeatStyle::for_theme(&theme);
        let (font, _) = page.fonts();
        let (w, ht) = heat_size(&heat, font, &style, page.px(1.0).0);
        let (pdf, drawn) = Page::draw(
            (w, ht),
            theme.background,
            page.cell_px(),
            |frame, font, bold| draw_heat(frame, &heat, font, bold, &theme, &style),
        );
        drawn.map_err(|why| anyhow::anyhow!("{why}"))?;
        write_pdf(&[pdf], page.dpi as f32, path)?;
        Ok(())
    }
}

/// Each gene's log effects, a row of topics per contrast level.
type Effects = HashMap<String, Vec<Vec<f32>>>;

/// `{prefix}.effect.parquet`'s log effects: the topics in order, and each
/// gene's values per level and topic.
fn read_effects(prefix: &Path, levels: &[String]) -> anyhow::Result<(Vec<String>, Effects)> {
    let t = TableReader::open(&table(prefix, "effect").to_string_lossy())?;
    let h: Vec<String> = t.header().iter().map(|h| h.to_string()).collect();
    let col = |name: &str| h.iter().position(|x| x == name);
    let topic = col("topic").ok_or_else(|| anyhow::anyhow!("no topic column"))?;
    let effect: Vec<Option<usize>> = levels
        .iter()
        .map(|l| col(&format!("log_effect{l}")))
        .collect();
    let all: Vec<usize> = (0..h.len()).collect();
    let mut topics: Vec<String> = Vec::new();
    let mut rows: Vec<(String, usize, Vec<f32>)> = Vec::new();
    for row in t.rows(&all)? {
        let row = row?;
        let k = match topics.iter().position(|x| x == row[topic].as_ref()) {
            Some(k) => k,
            None => {
                topics.push(row[topic].to_string());
                topics.len() - 1
            }
        };
        let v = effect
            .iter()
            .map(|c| c.map_or(f32::NAN, |c| number(&row[c])))
            .collect();
        rows.push((row[0].to_string(), k, v));
    }
    let mut out: Effects = HashMap::new();
    for (gene, k, v) in rows {
        let per_level = out
            .entry(gene)
            .or_insert_with(|| vec![vec![f32::NAN; topics.len()]; levels.len()]);
        for (l, x) in v.into_iter().enumerate() {
            per_level[l][k] = x;
        }
    }
    Ok((topics, out))
}

/// Names that are numbers in their order, the rest after them by text.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    }
}

/// The area under the ROC curve of `scored` (score, is causal): the chance
/// a causal gene scores above one that is not, ties counting half. None
/// without both kinds.
#[must_use]
pub fn auroc(scored: &[(f32, bool)]) -> Option<f64> {
    let mut s: Vec<(f32, bool)> = scored.to_vec();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pos = s.iter().filter(|x| x.1).count();
    let neg = s.len() - pos;
    if pos == 0 || neg == 0 {
        return None;
    }
    // Rank sum of the causal genes, ties at their mean rank.
    let mut rank_sum = 0.0;
    let mut i = 0;
    while i < s.len() {
        let mut j = i;
        while j + 1 < s.len() && s[j + 1].0 == s[i].0 {
            j += 1;
        }
        let mean_rank = (i + j) as f64 / 2.0 + 1.0;
        rank_sum += mean_rank * s[i..=j].iter().filter(|x| x.1).count() as f64;
        i = j + 1;
    }
    let pos = pos as f64;
    Some((rank_sum - pos * (pos + 1.0) / 2.0) / (pos * neg as f64))
}

/// The genes called at FDR `q` by Benjamini–Hochberg on `p` (p-value, is
/// causal): how many, the share of the `n_causal` causal genes found, and
/// the share of calls that are not causal.
#[must_use]
pub fn called_at(p: &[(f64, bool)], q: f64, n_causal: usize) -> (usize, f64, f64) {
    let mut s: Vec<(f64, bool)> = p.to_vec();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    let m = s.len() as f64;
    let k = s
        .iter()
        .enumerate()
        .rev()
        .find(|(i, (p, _))| *p <= q * (*i as f64 + 1.0) / m)
        .map_or(0, |(i, _)| i + 1);
    let hits = s[..k].iter().filter(|x| x.1).count();
    let power = if n_causal == 0 {
        0.0
    } else {
        hits as f64 / n_causal as f64
    };
    let fdr = if k == 0 {
        0.0
    } else {
        (k - hits) as f64 / k as f64
    };
    (k, power, fdr)
}

#[cfg(test)]
#[path = "tests/results.rs"]
pub(crate) mod tests;
