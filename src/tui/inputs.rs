//! `cocoa diff`'s inputs: files picked in the browser, each with the role
//! it plays (the counts, who each cell is from, each individual's exposure,
//! topics, covariates, control genes, and the truth of a simulation), and
//! the checks of how they fit together.

use crate::input::DELIMS;
use legume_tui::browse::Browser;
use std::path::{Path, PathBuf};

/// What a picked file is to the diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Data,
    Individuals,
    Exposure,
    Topics,
    Proportions,
    Covariates,
    Controls,
    /// A simulation's causal genes: not passed on, scored against.
    Truth,
}

impl Role {
    /// The roles in the order Space steps through them.
    pub const ALL: [Role; 8] = [
        Role::Data,
        Role::Individuals,
        Role::Exposure,
        Role::Topics,
        Role::Proportions,
        Role::Covariates,
        Role::Controls,
        Role::Truth,
    ];

    /// The box before a file in the browser.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Role::Data => "[data ]",
            Role::Individuals => "[indv ]",
            Role::Exposure => "[exp  ]",
            Role::Topics => "[topic]",
            Role::Proportions => "[prop ]",
            Role::Covariates => "[covar]",
            Role::Controls => "[ctrl ]",
            Role::Truth => "[truth]",
        }
    }

    /// The row of the inputs panel.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Role::Data => "data",
            Role::Individuals => "individuals",
            Role::Exposure => "exposure",
            Role::Topics => "topics",
            Role::Proportions => "proportions",
            Role::Covariates => "covariates",
            Role::Controls => "controls",
            Role::Truth => "truth",
        }
    }

    /// The flag `cocoa diff` takes it by; none for the data (its
    /// positionals) and the truth.
    #[must_use]
    pub fn long(self) -> Option<&'static str> {
        match self {
            Role::Data | Role::Truth => None,
            Role::Individuals => Some("indv-files"),
            Role::Exposure => Some("exposure-assignment-file"),
            Role::Topics => Some("topic-assignment-files"),
            Role::Proportions => Some("topic-proportion-files"),
            Role::Covariates => Some("covariate-file"),
            Role::Controls => Some("control-genes"),
        }
    }

    /// Whether it takes several files.
    #[must_use]
    pub fn many(self) -> bool {
        matches!(
            self,
            Role::Data | Role::Individuals | Role::Topics | Role::Proportions
        )
    }

    /// The role a file's name suggests, the first Space gives it.
    #[must_use]
    pub fn guess(name: &str) -> Role {
        let n = name.to_ascii_lowercase();
        let has = |w: &[&str]| w.iter().any(|w| n.contains(w));
        if has(&[".zarr", ".h5"]) {
            Role::Data
        } else if has(&["sample", "indv", "individual", "donor"]) {
            Role::Individuals
        } else if has(&["exposure"]) {
            Role::Exposure
        } else if has(&["celltype", "topic", "cluster", "annot"]) {
            Role::Topics
        } else if has(&["prop", "theta"]) {
            Role::Proportions
        } else if has(&["causal", "truth"]) {
            Role::Truth
        } else if has(&["covar", "conf_v"]) {
            Role::Covariates
        } else if has(&["control"]) {
            Role::Controls
        } else {
            Role::Data
        }
    }

    /// The role after this one, or none past the last.
    #[must_use]
    pub fn next(self) -> Option<Role> {
        let at = Role::ALL.iter().position(|r| *r == self)?;
        Role::ALL.get(at + 1).copied()
    }
}

/// The ids of the flags the Inputs tab sets, kept out of the Flags tab.
pub const OWNED: &[&str] = &[
    "data_files",
    "indv_files",
    "exposure_assignment_file",
    "topic_assignment_files",
    "topic_proportion_files",
    "covariate_file",
    "control_genes",
    "output",
];

/// Whether a file might be an input: counts, a list, a table.
#[must_use]
pub fn is_input(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        ".zarr",
        ".zarr.zip",
        ".h5",
        ".h5ad",
        ".gz",
        ".txt",
        ".tsv",
        ".csv",
        ".parquet",
    ]
    .iter()
    .any(|e| n.ends_with(e))
}

/// The diff's inputs as picked.
pub struct Inputs {
    pub browser: Browser,
    /// Files with a role, in the order picked.
    pub picked: Vec<(PathBuf, Role)>,
}

impl Inputs {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        let note = |p: &Path| note(p);
        Inputs {
            browser: Browser::new(dir, is_input, note).opened(),
            picked: Vec::new(),
        }
    }

    #[must_use]
    pub fn role_of(&self, p: &Path) -> Option<Role> {
        self.picked.iter().find(|(q, _)| q == p).map(|(_, r)| *r)
    }

    /// The files with `role`, in the order picked.
    #[must_use]
    pub fn files(&self, role: Role) -> Vec<&PathBuf> {
        self.picked
            .iter()
            .filter(|(_, r)| *r == role)
            .map(|(p, _)| p)
            .collect()
    }

    /// Give `p` `role`, or none; a role that takes one file leaves the
    /// file that had it.
    pub fn set(&mut self, p: PathBuf, role: Option<Role>) {
        self.picked.retain(|(q, _)| *q != p);
        let Some(role) = role else {
            return;
        };
        if !role.many() {
            self.picked.retain(|(_, r)| *r != role);
        }
        self.picked.push((p, role));
    }

    /// Space on `p`: the role its name suggests, then each role after it,
    /// then none.
    pub fn cycle(&mut self, p: PathBuf) {
        let next = match self.role_of(&p) {
            None => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                Some(Role::guess(name.as_deref().unwrap_or("")))
            }
            Some(r) => r.next(),
        };
        self.set(p, next);
    }

    /// What the run still needs: data, individuals and an exposure.
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        [Role::Data, Role::Individuals, Role::Exposure]
            .into_iter()
            .filter(|r| self.files(*r).is_empty())
            .map(Role::label)
            .collect()
    }

    /// The truth to score against, when one was picked.
    #[must_use]
    pub fn truth(&self) -> Option<&PathBuf> {
        self.files(Role::Truth).into_iter().next()
    }

    /// Prefill from what the command line gave: the data and each role's
    /// files, as `here` resolves them.
    pub fn prefill(&mut self, m: &clap::ArgMatches, here: &Path) {
        let words = |id: &str| legume_tui::form::given_words(m, id).unwrap_or_default();
        let path = |w: &str| legume_tui::browse::normalize(&here.join(w));
        for w in words("data_files") {
            self.set(path(&w), Some(Role::Data));
        }
        for role in Role::ALL {
            let Some(long) = role.long() else { continue };
            for w in words(&long.replace('-', "_")) {
                for f in w.split(',').filter(|f| !f.is_empty()) {
                    self.set(path(f), Some(role));
                }
            }
        }
    }
}

/// The command line's inputs of `picked`, as a command run in `from` names
/// them (whole for none): the data first, then each role's flag.
#[must_use]
pub fn argv(picked: &[(PathBuf, Role)], from: Option<&Path>) -> Vec<String> {
    let shown = |p: &PathBuf| match from {
        Some(dir) => super::queue::relative(p, dir),
        None => p.to_string_lossy().into_owned(),
    };
    let of = |role: Role| {
        picked
            .iter()
            .filter(move |(_, r)| *r == role)
            .map(|(p, _)| p)
    };
    let mut argv: Vec<String> = of(Role::Data).map(shown).collect();
    for role in Role::ALL {
        let Some(long) = role.long() else { continue };
        let files: Vec<String> = of(role).map(shown).collect();
        if !files.is_empty() {
            argv.push(format!("--{long}"));
            argv.push(files.join(","));
        }
    }
    argv
}

/// What the browser says beside a file: a matrix, or a list's size.
fn note(p: &Path) -> Option<String> {
    let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".zarr") || name.ends_with(".zarr.zip") || name.ends_with(".h5") {
        return Some("counts".into());
    }
    let bytes = std::fs::metadata(p).ok()?.len();
    Some(match bytes {
        b if b < 1 << 10 => format!("{b} B"),
        b if b < 1 << 20 => format!("{} KB", b >> 10),
        b => format!("{} MB", b >> 20),
    })
}

/// How the individuals and the exposures fit: one line each, `✓` or `✗`
/// first. Reads the files, so off the screen's thread.
#[must_use]
pub fn checks(indv: &[PathBuf], exposure: Option<&PathBuf>) -> Vec<String> {
    use std::collections::{BTreeMap, HashMap, HashSet};
    // Split as cocoa splits them, empty lines left out.
    let read = |p: &Path| {
        let path = p.to_string_lossy();
        legume_numeric::matrix::common_io::read_lines_of_words_delim(&path, DELIMS, -1).map(|w| {
            w.lines
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
    };
    let mut out = Vec::new();
    let mut people: HashSet<String> = HashSet::new();
    let mut cells = 0usize;
    for f in indv {
        match read(f) {
            Ok(lines) => {
                // One individual per line, or cell and individual.
                for l in &lines {
                    cells += 1;
                    people.insert(l.get(1).unwrap_or(&l[0]).to_string());
                }
            }
            Err(e) => out.push(format!("✗ {}: {e}", f.display())),
        }
    }
    if !indv.is_empty() {
        out.push(format!("✓ {cells} cells of {} individuals", people.len()));
    }
    let Some(exposure) = exposure else {
        return out;
    };
    let pairs = match read(exposure) {
        Ok(lines) => lines,
        Err(e) => {
            out.push(format!("✗ {}: {e}", exposure.display()));
            return out;
        }
    };
    let assigned: HashMap<String, String> = pairs
        .iter()
        .filter(|l| l.len() > 1)
        .map(|l| (l[0].to_string(), l[1].to_string()))
        .collect();
    let mut levels: BTreeMap<&str, usize> = BTreeMap::new();
    for (who, level) in &assigned {
        if people.is_empty() || people.contains(who) {
            *levels.entry(level.as_str()).or_default() += 1;
        }
    }
    let shown: Vec<String> = levels.iter().map(|(l, n)| format!("{l}: {n}")).collect();
    out.push(format!("✓ exposure levels {}", shown.join(", ")));
    let mut without: Vec<&String> = people
        .iter()
        .filter(|p| !assigned.contains_key(*p))
        .collect();
    without.sort();
    match without.len() {
        0 if !people.is_empty() => out.push("✓ every individual has an exposure".into()),
        0 => {}
        n => {
            let some: Vec<&str> = without.iter().take(3).map(|s| s.as_str()).collect();
            out.push(format!(
                "✗ {n} individuals have no exposure: {}",
                some.join(", ")
            ));
        }
    }
    if levels.len() < 2 {
        out.push("✗ fewer than two exposure levels: nothing to contrast".into());
    }
    out
}

#[cfg(test)]
#[path = "tests/inputs.rs"]
mod tests;
