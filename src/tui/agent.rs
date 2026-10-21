//! Following an agent's `cocoa mcp` from the view: legume-mcp's pane lists
//! its calls and background jobs (`job-N`, which s stops there), and the
//! run the agent last worked on (what `cocoa_run` and `cocoa_read`
//! publish) comes to the Results tab while the pane follows.

use legume_mcp::watch::{self, view::AgentPane, CallView};
use serde_json::Value;
use std::path::PathBuf;

pub struct Agent {
    pub pane: AgentPane,
    /// The run last taken from the agent: the same one again is not taken
    /// again.
    taken: Option<PathBuf>,
}

impl Agent {
    /// Follow the `cocoa mcp` with pid `which`, or the newest running one
    /// when it is empty.
    pub fn find(which: &str) -> anyhow::Result<Self> {
        let pid = match which.trim() {
            "" => None,
            pid => Some(
                pid.parse::<u32>()
                    .map_err(|_| anyhow::anyhow!("{pid} is not a pid"))?,
            ),
        };
        let server = watch::find(crate::mcp::SERVER, pid)
            .into_iter()
            .next()
            .ok_or_else(|| match pid {
                None => anyhow::anyhow!("no running `cocoa mcp` to follow"),
                Some(pid) => anyhow::anyhow!("no running `cocoa mcp` with pid {pid}"),
            })?;
        Ok(Agent {
            pane: AgentPane::new(&server.dir, true)?,
            taken: None,
        })
    }

    /// The server in a line, for the header.
    #[must_use]
    pub fn title(&self) -> String {
        let a = self.pane.agent();
        let pid = a
            .meta()
            .map_or_else(String::new, |m| format!(" pid {}", m.pid));
        let how = if a.ended() {
            "ended"
        } else if self.pane.following() {
            "following"
        } else {
            "paused"
        };
        format!("agent{pid} {how}")
    }

    /// The run the agent works on, when it is new, `ready`, all it writes
    /// is there (a background job's is not until it ends), and the pane
    /// follows.
    pub fn next_run(&mut self, ready: impl Fn(&std::path::Path) -> bool) -> Option<PathBuf> {
        if !self.pane.following() {
            return None;
        }
        let state = self.pane.state()?;
        let out = run_of(state)?;
        if self.taken.as_ref() == Some(&out) {
            return None;
        }
        let written = state
            .get("outputs")
            .and_then(Value::as_array)
            .is_none_or(|o| {
                o.iter()
                    .filter_map(Value::as_str)
                    .all(|f| written(std::path::Path::new(f)))
            });
        if !written || !ready(&out) {
            return None;
        }
        self.taken = Some(out.clone());
        Some(out)
    }
}

/// The output prefix a published state names.
fn run_of(state: &Value) -> Option<PathBuf> {
    state.get("out")?.as_str().map(PathBuf::from)
}

/// The output prefix a call was about: what it returned (`out`), else what
/// it was given (`out`, `prefix`).
#[must_use]
pub fn call_run(c: &CallView) -> Option<PathBuf> {
    let returned = c
        .text
        .find('{')
        .and_then(|at| serde_json::from_str::<Value>(&c.text[at..]).ok())
        .and_then(|v| run_of(&v));
    returned.or_else(|| {
        ["out", "prefix"]
            .iter()
            .find_map(|k| c.args.get(*k)?.as_str().map(PathBuf::from))
    })
}

/// Whether output `f` is all there: a parquet table once its footer is
/// (the writer puts it last), anything else once it exists.
fn written(f: &std::path::Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    if f.extension().is_none_or(|e| e != "parquet") {
        return f.exists();
    }
    let mut tail = [0u8; 4];
    std::fs::File::open(f)
        .and_then(|mut file| {
            file.seek(SeekFrom::End(-4))?;
            file.read_exact(&mut tail)
        })
        .is_ok_and(|()| &tail == b"PAR1")
}

#[cfg(test)]
#[path = "tests/agent.rs"]
mod tests;
