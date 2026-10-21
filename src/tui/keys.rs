//! The view's keys: Tab and Shift+Tab between the tabs, Ctrl+R to preview
//! the runs (and start them from the preview), Ctrl+C to quit, `?` for all
//! of them, and each tab's own.

use super::{App, FlagRow, Of, Page, Target};
use legume_mcp::watch::view::Handled;
use legume_tui::browse::Nav;
use legume_tui::form::Kind;
use legume_tui::save::Field;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The apply key: Ctrl+R, a control byte every terminal sends as it is.
pub fn is_apply(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('r' | 'R'))
}

fn ctrl(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char(c)
}

impl App {
    pub fn key(&mut self, k: KeyEvent, session: &legume_tui::Session) {
        if ctrl(&k, 'c') {
            self.quit = true;
            return;
        }
        let asked = std::mem::take(&mut self.asking_quit);
        self.message = None;
        if self.editing.is_some() {
            self.edit_key(k);
            return;
        }
        if self.help {
            self.help = !matches!(k.code, KeyCode::Esc | KeyCode::Char('q' | '?'));
            return;
        }
        if self.gv.picking.is_some() {
            self.gv.picker_key(k, self.results.as_ref());
            return;
        }
        if self.finder.is_some() {
            self.finder_key(k);
            return;
        }
        if self.preview {
            match k.code {
                KeyCode::Enter => self.start(session, None),
                _ if is_apply(&k) => self.start(session, None),
                KeyCode::Esc | KeyCode::Char('q') => self.preview = false,
                KeyCode::Down => self.preview_scroll += 1,
                KeyCode::Up => self.preview_scroll = self.preview_scroll.saturating_sub(1),
                _ => {}
            }
            return;
        }
        if self.ended && !self.told {
            self.told = true;
            if k.code == KeyCode::Char('d') && self.can_diff_on_sim() {
                self.diff_on_sim();
            } else if self.results.is_some() {
                self.page = Page::Results;
            }
            return;
        }
        // The agent's pane takes every key while a note is typed.
        if self.page == Page::Agent && self.agent_key(k) {
            return;
        }
        if is_apply(&k) {
            if self.running() {
                self.message = Some("runs are going: s on the Run tab stops them".into());
            } else {
                self.replan();
                self.preview = true;
                self.preview_scroll = 0;
            }
            return;
        }
        match k.code {
            KeyCode::Tab => return self.turn(1),
            KeyCode::BackTab => return self.turn(-1),
            _ => {}
        }
        // The file browser types: its letters narrow the listing.
        let typing = self.page == Page::Inputs;
        if !typing {
            match k.code {
                KeyCode::Char('q') if self.running() && !asked => {
                    self.asking_quit = true;
                    self.message = Some("runs are going: q again stops them and quits".into());
                    return;
                }
                KeyCode::Char('q') => {
                    self.quit = true;
                    return;
                }
                KeyCode::Char('?') => {
                    self.help = true;
                    return;
                }
                KeyCode::Char('W') => {
                    self.follow_agent();
                    return;
                }
                _ => {}
            }
        }
        // Only the tabs that set a run up change what it would be.
        let sets_up = matches!(self.page, Page::Model | Page::Inputs | Page::Flags);
        match self.page {
            Page::Model => self.model_key(k),
            Page::Inputs => self.inputs_key(k),
            Page::Flags => self.flags_key(k),
            Page::Run => self.run_key(k),
            Page::Results => self.results_key(k),
            Page::Agent => {}
        }
        if sets_up {
            self.replan();
        }
    }

    /// The next (or previous) tab there is.
    fn turn(&mut self, d: isize) {
        let pages = self.pages();
        let at = pages.iter().position(|p| *p == self.page).unwrap_or(0) as isize;
        let n = pages.len() as isize;
        self.page = pages[(at + d).rem_euclid(n) as usize];
    }

    /// `W`: follow the newest `cocoa mcp`, or say there is none.
    fn follow_agent(&mut self) {
        match super::agent::Agent::find("") {
            Ok(a) => {
                self.agent = Some(a);
                self.page = Page::Agent;
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    pub fn can_diff_on_sim(&self) -> bool {
        let Some(q) = &self.queue else { return false };
        let states = q.states();
        q.jobs.len() == 1
            && q.jobs[0].command.starts_with("simulate")
            && states.first() == Some(&super::State::Done)
    }

    fn edit(&mut self, target: Target, text: String) {
        self.editing = Some((target, Field::new(text)));
    }

    fn edit_key(&mut self, k: KeyEvent) {
        let Some((target, mut field)) = self.editing.take() else {
            return;
        };
        match k.code {
            KeyCode::Esc => {
                if target == Target::Find {
                    self.flags.find.clear();
                }
            }
            KeyCode::Enter => self.take(&target, &field.text),
            _ => {
                field.key(k);
                if target == Target::Find {
                    self.flags.find = field.text.clone();
                    self.flags.at = 0;
                }
                self.editing = Some((target, field));
            }
        }
        self.replan();
    }

    /// Typed `text` taken as `target`.
    fn take(&mut self, target: &Target, text: &str) {
        match target {
            Target::SimOut => self.sim_out = text.trim().to_string(),
            Target::DiffOut => self.diff_out = text.trim().to_string(),
            Target::Find => self.flags.find = text.to_string(),
            Target::Model(i) => {
                if let Some(sim) = &mut self.sim {
                    let long = sim.model.rows()[*i].long;
                    if let Some(f) = sim.form_mut().get_mut(long) {
                        f.set_text(text);
                    }
                }
            }
            Target::Flag(of, i) => {
                if let Some(f) = self.form_mut(*of).fields.get_mut(*i) {
                    f.set_text(text);
                }
            }
        }
    }

    fn model_key(&mut self, k: KeyEvent) {
        let Some(sim) = &mut self.sim else { return };
        match k.code {
            KeyCode::Up => sim.step(-1),
            KeyCode::Down => sim.step(1),
            KeyCode::Left => sim.nudge(-1),
            KeyCode::Right => sim.nudge(1),
            KeyCode::Char('m') => sim.switch(),
            KeyCode::Char('d') => sim.then_diff = !sim.then_diff,
            KeyCode::Char('o') => {
                let text = self.sim_out.clone();
                self.edit(Target::SimOut, text);
            }
            KeyCode::Enter => {
                if let Some((_, f)) = sim.row() {
                    let (at, text) = (sim.at, f.value.clone());
                    self.edit(Target::Model(at), text);
                }
            }
            KeyCode::Backspace => {
                let r = sim.model.rows()[sim.at];
                if let Some(f) = sim.form_mut().get_mut(r.long) {
                    match r.start {
                        Some(s) => f.value = s.to_string(),
                        None => f.reset(),
                    }
                }
            }
            _ => {}
        }
    }

    fn inputs_key(&mut self, k: KeyEvent) {
        if ctrl(&k, 'o') {
            let text = self.diff_out.clone();
            self.edit(Target::DiffOut, text);
            return;
        }
        let b = &mut self.inputs.browser;
        match k.code {
            KeyCode::Char(' ') => {
                if let Some(e) = b.highlighted().filter(|e| !e.dir) {
                    let p = e.path.clone();
                    self.inputs.cycle(p);
                }
            }
            KeyCode::Delete => {
                if let Some(e) = b.highlighted() {
                    let p = e.path.clone();
                    self.inputs.set(p, None);
                }
            }
            _ => match b.key(k) {
                Nav::Picked(p) => self.inputs.cycle(p),
                Nav::Moved => {}
                Nav::Ignored if k.code == KeyCode::Esc => {
                    self.message = Some("Ctrl+C quits; Tab goes to the other tabs".into());
                }
                Nav::Ignored => {}
            },
        }
    }

    fn flags_key(&mut self, k: KeyEvent) {
        let rows = self.flag_rows();
        let flags: Vec<(Of, usize)> = rows
            .iter()
            .filter_map(|r| match r {
                FlagRow::Flag(of, i) => Some((*of, *i)),
                FlagRow::Heading(_) => None,
            })
            .collect();
        let last = flags.len().saturating_sub(1);
        let at = self.flags.at.min(last);
        let current = flags.get(at).copied();
        match k.code {
            KeyCode::Up => self.flags.at = at.saturating_sub(1),
            KeyCode::Down => self.flags.at = (at + 1).min(last),
            KeyCode::PageUp => self.flags.at = at.saturating_sub(10),
            KeyCode::PageDown => self.flags.at = (at + 10).min(last),
            KeyCode::Char('/') => {
                let text = self.flags.find.clone();
                self.edit(Target::Find, text);
            }
            KeyCode::Char('a') => self.flags.advanced = !self.flags.advanced,
            KeyCode::Esc if !self.flags.find.is_empty() => self.flags.find.clear(),
            _ => {
                let Some((of, i)) = current else { return };
                let Some(f) = self.form_mut(of).fields.get_mut(i) else {
                    return;
                };
                match (k.code, &f.kind) {
                    (KeyCode::Backspace, _) => f.reset(),
                    (KeyCode::Enter | KeyCode::Char(' '), Kind::Flag { .. }) => f.toggle(),
                    (KeyCode::Left, Kind::Choice(_)) => f.cycle(-1),
                    (KeyCode::Right | KeyCode::Enter | KeyCode::Char(' '), Kind::Choice(_)) => {
                        f.cycle(1);
                    }
                    (KeyCode::Enter, Kind::Text) => {
                        let text = f.value.clone();
                        self.edit(Target::Flag(of, i), text);
                    }
                    _ => {}
                }
            }
        }
    }

    fn run_key(&mut self, k: KeyEvent) {
        let Some(q) = &self.queue else { return };
        let n = q.jobs.len();
        match k.code {
            KeyCode::Char('s') if self.running() => {
                let kills = q.stop();
                self.message = Some(if kills {
                    "killing the run".into()
                } else {
                    "stopping the run: s again kills it".into()
                });
            }
            KeyCode::Char('d') if self.can_diff_on_sim() => self.diff_on_sim(),
            KeyCode::Left | KeyCode::Char('[') => {
                self.run_at = self.run_at.saturating_sub(1);
                self.log_scroll = 0;
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.run_at = (self.run_at + 1).min(n.saturating_sub(1));
                self.log_scroll = 0;
            }
            KeyCode::Up => self.log_scroll += 1,
            KeyCode::Down => self.log_scroll = self.log_scroll.saturating_sub(1),
            KeyCode::PageUp => self.log_scroll += 10,
            KeyCode::PageDown => self.log_scroll = self.log_scroll.saturating_sub(10),
            KeyCode::End => self.log_scroll = 0,
            _ => {}
        }
    }

    /// `/`: the finder, over every gene of the results.
    fn open_finder(&mut self) {
        let Some(r) = &self.results else { return };
        let mut names: Vec<String> = r
            .genes
            .iter()
            .map(|g| super::figure::shown_name(&g.name).to_string())
            .collect();
        names.sort();
        names.dedup();
        let mut list = legume_tui::browse::FindList::default();
        list.set(names, 0);
        self.finder = Some(list);
    }

    fn finder_key(&mut self, k: KeyEvent) {
        let Some(list) = &mut self.finder else { return };
        match k.code {
            KeyCode::Esc if list.find.text.is_empty() => self.finder = None,
            KeyCode::Enter => {
                let name = list.highlighted().cloned();
                self.finder = None;
                let (Some(name), Some(r)) = (name, &self.results) else {
                    return;
                };
                if let Some(g) = super::gene_view::GeneView::find(r, &name) {
                    match &mut self.gene {
                        Some(page) => page.go(r, g),
                        None => self.gene = Some(super::gene_view::GeneView::open(r, g)),
                    }
                }
            }
            _ => {
                list.key(&k);
            }
        }
    }

    /// A key on a gene's page: its own first, then the Results keys that
    /// mean the same there.
    fn gene_key(&mut self, k: KeyEvent) {
        use super::gene_view::Took;
        let took = match (&mut self.gene, &self.results) {
            (Some(page), Some(r)) => page.key(k, r),
            _ => return,
        };
        match took {
            Took::Consumed => return,
            Took::Close => {
                self.gene = None;
                return;
            }
            Took::Ignored => {}
        }
        match k.code {
            KeyCode::Char('l') => {
                if let (Some(page), Some(r)) = (&mut self.gene, &mut self.results) {
                    r.next_level();
                    page.reread(r);
                    self.gv.read_insets(r);
                    self.gv.redraw();
                }
            }
            KeyCode::Char('/') => self.open_finder(),
            KeyCode::Char('g') => self.gv.pick_annotation(),
            KeyCode::Char('p') => self.save_gene(),
            _ => {}
        }
    }

    /// `p` on a gene's page: the page as a PDF beside the run.
    fn save_gene(&mut self) {
        let (Some(page), Some(r)) = (&self.gene, &self.results) else {
            return;
        };
        let name = super::figure::shown_name(&r.genes[page.gene].name).to_string();
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = Self::free_pdf(&r.prefix, &format!("gene.{safe}"));
        let size = legume_tui::export::PageSize {
            width_in: 10.0,
            dpi: 300,
        };
        self.message = Some(match page.save(r, self.gv.genome.as_ref(), &path, size) {
            Ok(()) => format!("saved {}", legume_tui::browse::tilde(&path)),
            Err(e) => format!("the page was not saved: {e:#}"),
        });
    }

    fn results_key(&mut self, k: KeyEvent) {
        if self.gene.is_some() {
            return self.gene_key(k);
        }
        if k.code == KeyCode::Char('/') {
            return self.open_finder();
        }
        if self.gv.on && self.gv.nav_key(k) {
            return;
        }
        if !self.gv.on && k.code == KeyCode::Enter {
            if let Some(r) = &self.results {
                if let Some(&g) = r.order.get(r.at) {
                    self.gene = Some(super::gene_view::GeneView::open(r, g));
                }
            }
            return;
        }
        let Some(r) = &mut self.results else { return };
        let last = r.order.len().saturating_sub(1);
        match k.code {
            KeyCode::Up => r.at = r.at.saturating_sub(1),
            KeyCode::Down => r.at = (r.at + 1).min(last),
            KeyCode::PageUp => r.at = r.at.saturating_sub(10),
            KeyCode::PageDown => r.at = (r.at + 10).min(last),
            KeyCode::Home => r.at = 0,
            KeyCode::Char('s') => r.next_sort(),
            KeyCode::Char('l') => {
                r.next_level();
                self.gv.read_insets(r);
                self.gv.redraw();
            }
            KeyCode::Char('h') => self.show_heat = !self.show_heat,
            KeyCode::Char('+' | '=') => r.per_topic_by(1),
            KeyCode::Char('-') => r.per_topic_by(-1),
            KeyCode::Char('v') => {
                self.gv.on = !self.gv.on;
                self.gv.redraw();
            }
            KeyCode::Char('g') => self.gv.pick_annotation(),
            KeyCode::Char('i') => {
                if let Some(&g) = r.order.get(r.at) {
                    let name = r.genes[g].name.clone();
                    self.gv.pin(&name, r);
                    self.message = Some(format!("insets: {}", self.gv.pinned.join(", ")));
                }
            }
            KeyCode::Char('p') if self.gv.on => self.save_genome(),
            KeyCode::Char('p') => self.save_heat(),
            _ => {}
        }
    }

    /// A free name beside the run for a figure: `{prefix}.{what}.pdf`, else
    /// `{prefix}.{what}-2.pdf`, ... as every legume tool numbers them.
    fn free_pdf(prefix: &std::path::Path, what: &str) -> std::path::PathBuf {
        use legume_tui::save::{free_stem, Numbering};
        let stem = format!("{}.{what}", prefix.to_string_lossy());
        std::path::PathBuf::from(free_stem(&stem, &[".pdf"], Numbering::Suffix) + ".pdf")
    }

    /// `p` on the genome view: the figure as a PDF beside the run.
    fn save_genome(&mut self) {
        let Some(r) = &self.results else { return };
        let path = Self::free_pdf(&r.prefix, "genome");
        // Wide: the whole genome on one page, as the papers set it.
        let page = legume_tui::export::PageSize {
            width_in: 14.0,
            dpi: 300,
        };
        self.message = Some(match self.gv.save(r, &path, page) {
            Ok(()) => format!("saved {}", legume_tui::browse::tilde(&path)),
            Err(e) => format!("the figure was not saved: {e:#}"),
        });
    }

    /// `p`: the heatmap as a PDF beside the run, never over a file there.
    fn save_heat(&mut self) {
        let Some(r) = &self.results else { return };
        let path = Self::free_pdf(&r.prefix, "heatmap");
        let page = legume_tui::export::PageSize {
            width_in: 7.0,
            dpi: 300,
        };
        self.message = Some(match r.save_heat(&path, page) {
            Ok(()) => format!("saved {}", legume_tui::browse::tilde(&path)),
            Err(e) => format!("the heatmap was not saved: {e:#}"),
        });
    }

    /// A key for the agent's pane; whether it took it.
    fn agent_key(&mut self, k: KeyEvent) -> bool {
        let Some(a) = &mut self.agent else {
            return false;
        };
        // The host's own keys go by, unless a note is typed.
        if !a.pane.typing() && (matches!(k.code, KeyCode::Tab | KeyCode::BackTab) || is_apply(&k)) {
            return false;
        }
        match a.pane.key(k) {
            Handled::Consumed => true,
            Handled::Ignored => false,
            Handled::Open(call) => {
                match super::agent::call_run(&call).filter(|o| super::results::is_diff(o)) {
                    Some(out) => {
                        self.open_results(&out, None);
                        self.page = Page::Results;
                    }
                    None => self.message = Some("that call wrote no diff to open".into()),
                }
                true
            }
        }
    }
}
