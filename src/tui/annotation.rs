//! Where each gene is on the genome, for the Results tab's genome view.
//!
//! cocoa keeps no annotation of its own. It asks faba, which knows where
//! references come from (GENCODE, Ensembl): `faba reference` lists the
//! sources and their releases or species, and downloads a GTF on demand
//! into the reference cache outside any project, printing its path. The
//! genes of a GTF are read once into `{gtf}.genes.tsv` beside it (id,
//! name, chromosome, start, end), and the one picked last is remembered in
//! `cocoa.annotation` there, so a later view places genes at once.

use std::collections::HashMap;
use std::fmt::Write;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `faba reference` on `args`: `$FABA`, else `faba` on the path.
fn faba_reference(args: &[&str]) -> Command {
    let faba = std::env::var_os("FABA")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("faba"), PathBuf::from);
    let mut c = Command::new(faba);
    c.arg("reference").args(args).stdin(Stdio::null());
    c
}

/// Why faba could not be run.
fn no_faba(e: std::io::Error) -> anyhow::Error {
    anyhow::anyhow!("cannot run faba ({e}): install it, or set $FABA")
}

/// What `faba reference` prints for `args`: its lines, or why it could not
/// say.
fn ask(args: &[&str]) -> anyhow::Result<Vec<String>> {
    let out = faba_reference(args).output().map_err(no_faba)?;
    anyhow::ensure!(
        out.status.success(),
        "faba reference {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect())
}

/// faba's sources: a name `faba reference` takes, and what it is.
pub fn sources() -> anyhow::Result<Vec<(String, String)>> {
    Ok(ask(&[])?
        .into_iter()
        .map(|l| match l.split_once('\t') {
            Some((n, what)) => (n.to_string(), what.to_string()),
            None => (l.clone(), l),
        })
        .collect())
}

/// A source's releases (newest first) or species.
pub fn items(source: &str) -> anyhow::Result<Vec<String>> {
    ask(&[source])
}

/// A download faba runs on a thread: its last word on stderr, and the
/// GTF's path once done.
pub struct Fetch {
    pub what: String,
    pub said: std::sync::Arc<std::sync::Mutex<String>>,
    pub done: std::sync::mpsc::Receiver<Result<PathBuf, String>>,
}

impl Fetch {
    /// `faba reference SOURCE ITEM`, its progress followed.
    #[must_use]
    pub fn start(source: &str, item: &str) -> Fetch {
        let said = std::sync::Arc::new(std::sync::Mutex::new("asking faba".to_string()));
        let (tx, done) = std::sync::mpsc::channel();
        let (s, i, heard) = (source.to_string(), item.to_string(), said.clone());
        std::thread::spawn(move || {
            let run = || -> anyhow::Result<PathBuf> {
                let mut child = faba_reference(&[&s, &i])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(no_faba)?;
                let mut last = String::new();
                if let Some(err) = child.stderr.take() {
                    for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                        if let Ok(mut h) = heard.lock() {
                            h.clone_from(&line);
                        }
                        last = line;
                    }
                }
                let mut out = String::new();
                if let Some(mut o) = child.stdout.take() {
                    std::io::Read::read_to_string(&mut o, &mut out)?;
                }
                anyhow::ensure!(child.wait()?.success(), "faba reference {s} {i}: {last}");
                let gtf = out
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("faba said no path"))?;
                Ok(PathBuf::from(gtf.trim()))
            };
            let _ = tx.send(run().map_err(|e| format!("{e:#}")));
        });
        Fetch {
            what: format!("{source} {item}"),
            said,
            done,
        }
    }
}

/// One gene of an annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct Gene {
    pub id: String,
    pub name: String,
    pub chrom: String,
    pub start: u64,
    pub end: u64,
}

/// `{gtf}.genes.tsv`.
#[must_use]
pub fn genes_file(gtf: &Path) -> PathBuf {
    crate::runs::suffixed(gtf, ".genes.tsv")
}

/// A GTF attribute's value: `gene_name "GENE1";` gives `GENE1`.
fn attribute<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    attrs.split(';').find_map(|a| {
        let (k, v) = a.trim().split_once(' ')?;
        (k == key).then(|| v.trim().trim_matches('"'))
    })
}

/// The genes of a GTF (gzipped or not): its `gene` lines.
fn read_gtf(gtf: &Path) -> anyhow::Result<Vec<Gene>> {
    let reader = legume_numeric::matrix::common_io::open_buf_reader(&gtf.to_string_lossy())?;
    let mut genes = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.splitn(9, '\t').collect();
        if f.len() < 9 || f[2] != "gene" {
            continue;
        }
        let id = attribute(f[8], "gene_id").unwrap_or("");
        genes.push(Gene {
            id: id.split('.').next().unwrap_or(id).to_string(),
            name: attribute(f[8], "gene_name").unwrap_or(id).to_string(),
            chrom: f[0].to_string(),
            start: f[3].parse().unwrap_or(0),
            end: f[4].parse().unwrap_or(0),
        });
    }
    Ok(genes)
}

/// The genes of `gtf`, from `{gtf}.genes.tsv` when it was read before, else
/// read now and kept there.
pub fn genes(gtf: &Path) -> anyhow::Result<Vec<Gene>> {
    let tsv = genes_file(gtf);
    if let Some(genes) = std::fs::read_to_string(&tsv).ok().and_then(|t| kept(&t)) {
        return Ok(genes);
    }
    let genes = read_gtf(gtf)?;
    anyhow::ensure!(!genes.is_empty(), "{}: no gene lines", gtf.display());
    // A cache: a folder that cannot take it only costs a read next time.
    // Written aside and moved in whole, so no reader sees it half done.
    let mut text = String::new();
    for g in &genes {
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}",
            g.id, g.name, g.chrom, g.start, g.end
        );
    }
    let aside = tsv.with_extension(format!("tsv.{}", std::process::id()));
    if std::fs::write(&aside, text).is_ok() && std::fs::rename(&aside, &tsv).is_err() {
        let _ = std::fs::remove_file(&aside);
    }
    Ok(genes)
}

/// The genes of a kept table; none when it holds none.
fn kept(text: &str) -> Option<Vec<Gene>> {
    let genes: Vec<Gene> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some(Gene {
                id: f.first()?.to_string(),
                name: f.get(1)?.to_string(),
                chrom: f.get(2)?.to_string(),
                start: f.get(3)?.parse().ok()?,
                end: f.get(4)?.parse().ok()?,
            })
        })
        .collect();
    (!genes.is_empty()).then_some(genes)
}

/// Where the annotation picked last is remembered: beside faba's
/// references.
fn remembered() -> Option<PathBuf> {
    let set = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = set("LEGUME_REFERENCES").or_else(|| {
        set("XDG_CACHE_HOME")
            .or_else(|| set("HOME").map(|h| h.join(".cache")))
            .map(|c| c.join("legume").join("references"))
    })?;
    Some(dir.join("cocoa.annotation"))
}

/// The annotation picked last, if it is still there.
#[must_use]
pub fn last() -> Option<PathBuf> {
    let p = PathBuf::from(std::fs::read_to_string(remembered()?).ok()?.trim());
    p.is_file().then_some(p)
}

/// Remember `gtf` as the one picked.
pub fn remember(gtf: &Path) {
    if let Some(r) = remembered() {
        let _ = r.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(r, gtf.to_string_lossy().as_bytes());
    }
}

/// A chromosome's place in the genome's order: 1..22, X, Y; `None` for
/// the mitochondrion and the scaffolds, left out.
fn chrom_rank(c: &str) -> Option<u32> {
    let c = c.strip_prefix("chr").unwrap_or(c);
    match c {
        "X" => Some(23),
        "Y" => Some(24),
        _ => c.parse::<u32>().ok().filter(|n| (1..=22).contains(n)),
    }
}

/// The genome laid end to end: each chromosome's name, where it starts and
/// how long it is (to its last gene), and each data gene's place on it.
#[derive(Clone, Debug)]
pub struct Genome {
    pub file: PathBuf,
    pub chroms: Vec<(String, u64, u64)>,
    /// Per gene of the data, in its order: its place, when found.
    pub at: Vec<Option<u64>>,
}

impl Genome {
    /// How many of the data's genes were placed.
    #[must_use]
    pub fn found(&self) -> usize {
        self.at.iter().flatten().count()
    }

    /// `genes` (the annotation's) laid out, and `names` (the data's) placed
    /// on it: by Ensembl id, by symbol, or by either part of `ID_SYMBOL`.
    #[must_use]
    pub fn place(file: &Path, genes: &[Gene], names: &[String]) -> Genome {
        let mut len: HashMap<u32, (String, u64)> = HashMap::new();
        for g in genes {
            if let Some(r) = chrom_rank(&g.chrom) {
                let e = len.entry(r).or_insert_with(|| (g.chrom.clone(), 0));
                e.1 = e.1.max(g.end);
            }
        }
        let mut ranks: Vec<u32> = len.keys().copied().collect();
        ranks.sort_unstable();
        let mut chroms = Vec::new();
        let mut offset: HashMap<String, u64> = HashMap::new();
        let mut at = 0;
        for r in ranks {
            let (name, l) = &len[&r];
            offset.insert(name.clone(), at);
            chroms.push((name.strip_prefix("chr").unwrap_or(name).to_string(), at, *l));
            at += l;
        }
        let mut by: HashMap<String, u64> = HashMap::new();
        for g in genes {
            if let Some(o) = offset.get(&g.chrom) {
                let p = o + g.start;
                by.entry(g.id.to_ascii_uppercase()).or_insert(p);
                by.entry(g.name.to_ascii_uppercase()).or_insert(p);
            }
        }
        let find = |n: &str| -> Option<u64> {
            let n = n.to_ascii_uppercase();
            let bare = n.split('.').next().unwrap_or(&n).to_string();
            by.get(&n)
                .or_else(|| by.get(&bare))
                .or_else(|| n.split(['_', '|']).find_map(|p| by.get(p)))
                .copied()
        };
        let at: Vec<Option<u64>> = names.iter().map(|n| find(n)).collect();
        Genome {
            file: file.to_path_buf(),
            chroms,
            at,
        }
    }

    /// The genome's length, end to end.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.chroms.last().map_or(0, |(_, o, l)| o + l)
    }
}

#[cfg(test)]
#[path = "tests/annotation.rs"]
mod tests;
