//! Drawing the views: the header and tabs, each tab's body, the preview,
//! the pop-up a finished queue raises, the keys, and the line being typed.

use super::inputs::Role;
use super::queue::State;
use super::{App, FlagRow, Page, Target};
use legume_tui::browse::{first_visible, tilde, window};
use legume_tui::chrome::{At, Chrome};
use legume_tui::form::{self, script, Kind};
use legume_tui::look::Look;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// cocoa's accent: cocoa brown, for what matters.
pub const ACCENT: Color = Color::Rgb(176, 112, 60);
/// Borders and rules: a muted brown.
const RULE: Color = Color::Rgb(128, 96, 70);
/// Key hints and other secondary lines: a warm grey-brown.
const HINT: Color = Color::Rgb(150, 126, 104);
/// The cursor's line: cream on dark chocolate.
const CURSOR: (Color, Color) = (Color::Rgb(242, 228, 208), Color::Rgb(92, 58, 30));

/// The styles the screens draw in: browns, over the terminal's own
/// background and text, readable on a dark or a light one.
#[must_use]
pub fn look() -> Look {
    Look {
        page: Style::default(),
        bold: Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        hint: Style::new().fg(HINT),
        selected: Style::new().fg(CURSOR.0).bg(CURSOR.1),
        border: Style::new().fg(RULE),
    }
}

/// Width of the side panel on the Model, Inputs and Results tabs.
const SIDE: u16 = 44;

fn panel(title: String, look: &Look) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(look.border)
        .title(Span::styled(title, look.bold))
}

/// `area` framed and titled; the inside, a column clear of each side.
fn framed(f: &mut Frame, area: Rect, title: String, look: &Look) -> Rect {
    let block = panel(title, look);
    let inner = block.inner(area);
    f.render_widget(block, area);
    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

/// Keys and what they do, as a footer line: each key in the accent.
fn keys(pairs: &[(&str, &str)], look: &Look) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (k, what) in pairs {
        spans.push(Span::styled((*k).to_string(), look.bold));
        spans.push(Span::raw(format!(" {what}  ")));
    }
    Line::from(spans)
}

/// A footer line wrapped to `width`, never parting a key from what it does.
fn wrap_keys(line: Line<'static>, width: usize, look: &Look) -> Vec<Line<'static>> {
    let mut groups: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for s in line.spans {
        if s.style == look.bold {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("a group").push(s);
    }
    let mut out = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for g in groups {
        let w: usize = g.iter().map(Span::width).sum();
        if used + w > width && used > 1 {
            out.push(Line::from(std::mem::take(&mut row)));
            row.push(Span::raw(" "));
            used = 1;
        }
        used += w;
        row.extend(g);
    }
    out.push(Line::from(row));
    out
}

/// A `width`-cell bar for `v` of 1.
fn bar(v: f64, width: usize, look: &Look) -> [Span<'static>; 2] {
    let full = ((v.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    [
        Span::styled("█".repeat(full), look.bold),
        Span::styled("░".repeat(width - full), look.hint),
    ]
}

fn clock(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

impl App {
    pub fn draw(&mut self, f: &mut Frame) {
        let look = look();
        let area = f.area();
        let footer = wrap_keys(self.footer(&look), usize::from(area.width), &look);
        let [top, tabs, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(footer.len() as u16),
        ])
        .areas(area);
        let (header, tab_line) = (self.header(&look), self.tabs(&look));
        let reserve = header.width().max(tab_line.width()) as u16 + 2;
        f.render_widget(header, top);
        f.render_widget(tab_line, tabs);
        let corner = Rect::new(area.x, area.y, area.width, 2.min(area.height));
        super::logo::draw_corner(f.buffer_mut(), corner, reserve);
        match self.page {
            Page::Model => self.draw_model(f, body, &look),
            Page::Inputs => self.draw_inputs(f, body, &look),
            Page::Flags => self.draw_flags(f, body, &look),
            Page::Run => self.draw_run(f, body, &look),
            Page::Results if self.gene.is_some() && self.results.is_some() => {
                self.draw_gene_page(f, body, &look);
            }
            Page::Results if self.gv.on && self.results.is_some() => {
                self.draw_genome(f, body, &look);
            }
            Page::Results => self.draw_results(f, body, &look),
            Page::Agent => {
                if let Some(a) = &mut self.agent {
                    a.pane.render(body, f.buffer_mut());
                }
            }
        }
        f.render_widget(Paragraph::new(footer), foot);
        let chrome = Chrome::TERMINAL;
        if self.preview {
            let lines = self.preview_lines(&look, usize::from(body.height).saturating_sub(2));
            chrome.popup(f, body, lines, 110, At::Middle);
        } else if self.ended && !self.told {
            chrome.popup(f, body, self.ended_lines(&look), 80, At::Middle);
        }
        if self.gv.picking.is_some() {
            let rows = usize::from(body.height).saturating_sub(4);
            chrome.popup(f, body, self.gv.picker_lines(&look, rows), 90, At::Middle);
        }
        if let Some(list) = &self.finder {
            let rows = usize::from(body.height).saturating_sub(8).max(3);
            chrome.popup(f, body, finder_lines(list, &look, rows), 60, At::Middle);
        }
        if self.help {
            chrome.popup(f, body, help_lines(&look), 72, At::Middle);
        }
        if let Some(m) = &self.message {
            chrome.toast(f, body, m);
        }
    }

    fn header(&self, look: &Look) -> Line<'static> {
        let (badge, title) = match &self.sim {
            Some(s) => (
                " simulate ",
                format!(
                    "cocoa simulate · {}{}",
                    s.model.name(),
                    if s.then_diff { ", then diff" } else { "" }
                ),
            ),
            None => {
                let missing = self.inputs.missing();
                let state = if missing.is_empty() {
                    "inputs ✓".to_string()
                } else {
                    format!("needs {}", missing.join(", "))
                };
                (" diff ", format!("cocoa diff · {state}"))
            }
        };
        let state = match (&self.queue, &self.agent) {
            (Some(q), _) => format!(" · {}", q.status()),
            (None, Some(a)) => format!(" · {}", a.title()),
            _ => String::new(),
        };
        Line::from(vec![
            Span::styled(badge, look.bold.add_modifier(Modifier::REVERSED)),
            Span::styled(format!(" {title}{state}"), look.page),
            Span::styled(format!("   {}", tilde(&self.here)), look.hint),
        ])
    }

    fn tabs(&self, look: &Look) -> Line<'static> {
        let mut spans = vec![Span::raw(" ")];
        for p in self.pages() {
            let style = if p == self.page { look.bold } else { look.hint };
            spans.push(Span::styled(format!("{}  ", p.title()), style));
        }
        let button = if self.running() {
            "[ ■ running ]".to_string()
        } else {
            "[ ▶ preview Ctrl+R ]".to_string()
        };
        spans.push(Span::styled(
            button,
            look.bold.add_modifier(Modifier::REVERSED),
        ));
        Line::from(spans)
    }

    /// `p` as the view names it: from where cocoa started when under it,
    /// else from home.
    pub fn shown(&self, p: &std::path::Path) -> String {
        match p.strip_prefix(&self.here) {
            Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
            _ => tilde(p),
        }
    }

    /// The output a planned run of `command` goes to, or why there is
    /// none.
    fn planned_out(&self, command: &str) -> Result<String, String> {
        match &self.planned {
            Ok(jobs) => jobs
                .iter()
                .find(|j| j.command.starts_with(command))
                .map(|j| self.shown(&j.out))
                .ok_or_else(|| "none".to_string()),
            Err(e) => Err(e.clone()),
        }
    }

    fn draw_model(&self, f: &mut Frame, body: Rect, look: &Look) {
        let Some(sim) = &self.sim else { return };
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE)]).areas(body);
        let inner = framed(f, left, " model ".into(), look);
        let mut lines = vec![
            Line::from(vec![
                Span::styled(format!("‹ {} ›", sim.model.name()), look.bold),
                Span::styled(format!("   m: {}", sim.model.other().name()), look.hint),
            ]),
            Line::raw(""),
        ];
        lines.extend(
            sim.model
                .dag()
                .iter()
                .map(|l| Line::styled(format!("        {l}"), look.page)),
        );
        lines.push(Line::raw(""));
        let at = sim.at;
        for (i, r) in sim.model.rows().iter().enumerate() {
            if i > 0 && r.edge && !sim.model.rows()[i - 1].edge {
                lines.push(Line::raw(""));
                lines.push(Line::styled("edge      PVE", look.hint));
            }
            let Some(field) = sim.form().get(r.long) else {
                continue;
            };
            let on = i == at;
            let style = if on { look.selected } else { look.page };
            let changed = if field.is_default() { "  " } else { "* " };
            let mut spans = vec![
                form::marker(on, look),
                Span::styled(changed, look.bold),
                Span::styled(format!("{:<22}", r.label), style),
            ];
            if r.edge {
                let v: f64 = field.value.parse().unwrap_or(0.0);
                spans.extend(bar(v, 20, look));
                spans.push(Span::styled(format!("  {}", field.value), style));
            } else {
                spans.push(Span::styled(field.shown(), style));
            }
            lines.push(Line::from(spans));
        }
        let rows = usize::from(inner.height);
        let sel = lines.len().saturating_sub(sim.model.rows().len() - at);
        f.render_widget(Paragraph::new(window(lines, sel, rows)), inner);

        let inner = framed(f, right, " sizes ".into(), look);
        let row = |k: &str, v: String| {
            Line::from(vec![
                Span::styled(format!("{k:<14}"), look.hint),
                Span::styled(v, look.page),
            ])
        };
        let n = |l: &str| sim.number(l);
        let mut lines = Vec::new();
        if let Some(i) = sim.individuals() {
            lines.push(row("individuals", format!("{i}")));
            if let Some(c) = n("n-cells") {
                lines.push(row("cells", format!("{c} (~{:.0} each)", c / i.max(1.0))));
            }
        }
        if let (Some(g), Some(c)) = (n("n-genes"), n("n-causal-genes")) {
            lines.push(row(
                "causal",
                format!("{c} of {g} genes ({:.0}%)", 100.0 * c / g.max(1.0)),
            ));
        }
        lines.push(row(
            "then diff",
            if sim.then_diff { "on (d)" } else { "off (d)" }.into(),
        ));
        lines.push(Line::raw(""));
        let typed = if self.sim_out.is_empty() {
            ""
        } else {
            " (typed)"
        };
        match self.planned_out("simulate") {
            Ok(out) => lines.push(row("output", format!("{out}{typed}"))),
            Err(why) => {
                lines.push(row("output", "(o names it)".into()));
                lines.push(Line::raw(""));
                lines.push(Line::styled(why, look.bold));
            }
        }
        if let Ok(jobs) = &self.planned {
            lines.push(Line::raw(""));
            lines.push(Line::styled("writes", look.hint));
            for j in jobs {
                for o in j.outputs.iter().take(6) {
                    let name = o
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    let truth = if name.ends_with(".causal.gz") {
                        "  ← truth"
                    } else {
                        ""
                    };
                    lines.push(Line::styled(format!(" {name}{truth}"), look.page));
                }
                if let Some(more) = j.outputs.len().checked_sub(6).filter(|&m| m > 0) {
                    lines.push(Line::styled(format!(" … and {more} more"), look.hint));
                }
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_inputs(&self, f: &mut Frame, body: Rect, look: &Look) {
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE)]).areas(body);
        let b = &self.inputs.browser;
        let title = format!(" files · {} {}", tilde(&b.cwd), b.list.find.tag());
        let inner = framed(f, left, title, look);
        let mark = |e: &legume_tui::browse::Entry| {
            self.inputs.role_of(&e.path).map(|r| r.tag().to_string())
        };
        let lines = b.lines(
            usize::from(inner.height),
            usize::from(inner.width),
            look,
            7,
            mark,
            "no inputs here",
        );
        f.render_widget(Paragraph::new(lines), inner);

        let inner = framed(f, right, " inputs ".into(), look);
        let mut lines = Vec::new();
        for role in Role::ALL {
            let files = self.inputs.files(role);
            let shown = match files.len() {
                0 => "(none)".to_string(),
                1 => name(files[0]),
                n => format!("{} +{}", name(files[0]), n - 1),
            };
            let style = if files.is_empty() {
                look.hint
            } else {
                look.page
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<13}", role.label()), look.hint),
                Span::styled(shown, style),
            ]));
        }
        let typed = if self.diff_out.is_empty() {
            ""
        } else {
            " (typed)"
        };
        let out = match self.planned_out("diff") {
            Ok(o) => format!("{o}{typed}"),
            Err(_) => "(Ctrl+O names it)".into(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<13}", "output"), look.hint),
            Span::styled(out, look.page),
        ]));
        lines.push(Line::raw(""));
        for c in &self.checks {
            let style = if c.starts_with('✗') {
                look.bold
            } else {
                look.page
            };
            lines.push(Line::styled(c.clone(), style));
        }
        if let Err(why) = &self.planned {
            if !why.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::styled(why.clone(), look.bold));
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_flags(&self, f: &mut Frame, body: Rect, look: &Look) {
        let rows = self.flag_rows();
        let changed = self.diff.changed() + self.sim.as_ref().map_or(0, |s| s.form().changed());
        let mut title = format!(" flags · {changed} changed ");
        if self.flags.advanced {
            title.push_str("· advanced ");
        }
        if !self.flags.find.is_empty() {
            title.push_str(&format!("· find {} ", self.flags.find));
        }
        let inner = framed(f, body, title, look);
        let [list, help] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(6)]).areas(inner);
        let complaint = self.planned.as_ref().err();
        let mut lines = Vec::new();
        let mut k = 0;
        let mut sel_line = 0;
        let mut sel = None;
        for r in &rows {
            match r {
                FlagRow::Heading(h) => lines.push(Line::styled(h.clone(), look.bold)),
                FlagRow::Flag(of, i) => {
                    let form = self.form(*of);
                    let field = &form.fields[*i];
                    let on = k == self.flags.at;
                    if on {
                        sel_line = lines.len();
                        sel = Some(field);
                    }
                    let shown = match field.kind {
                        Kind::Choice(_) => format!("‹ {} ›", field.shown()),
                        _ => field.shown(),
                    };
                    let said = complaint
                        .filter(|c| form::blamed(c, &form.fields) == Some(field.long.as_str()))
                        .map(String::as_str);
                    let style = if on { look.selected } else { look.page };
                    lines.push(form::row_line(field, on, 30, look, style, shown, said));
                    k += 1;
                }
            }
        }
        if lines.is_empty() {
            lines.push(Line::styled(
                format!("no flag matches \"{}\"", self.flags.find),
                look.hint,
            ));
        }
        let n = usize::from(list.height);
        f.render_widget(Paragraph::new(window(lines, sel_line, n)), list);
        let Some(field) = sel else { return };
        let block = Block::new()
            .borders(Borders::TOP)
            .border_style(look.border)
            .title(Span::styled(format!(" --{} ", field.long), look.bold));
        let text = block.inner(help);
        f.render_widget(block, help);
        let d = if field.default.is_empty() {
            "(none)"
        } else {
            &field.default
        };
        let mut lines: Vec<Line> = field
            .long_help
            .split("\n\n")
            .map(|p| Line::raw(p.split_whitespace().collect::<Vec<_>>().join(" ")))
            .collect();
        lines.push(Line::styled(format!("default {d}"), look.hint));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), text);
    }

    fn draw_run(&self, f: &mut Frame, body: Rect, look: &Look) {
        let Some(q) = &self.queue else { return };
        let Ok(s) = q.shared.lock() else { return };
        let n = q.jobs.len() as u16;
        let [list, log] =
            Layout::vertical([Constraint::Length(n + 2), Constraint::Fill(1)]).areas(body);
        let inner = framed(f, list, " queue ".into(), look);
        let mut lines = Vec::new();
        for (i, j) in q.jobs.iter().enumerate() {
            let state = &s.states[i];
            let took = match (s.took[i], s.started[i]) {
                (Some(d), _) => clock(d),
                (None, Some(t)) => clock(t.elapsed()),
                _ => String::new(),
            };
            let on = i == self.run_at;
            let style = if on { look.selected } else { look.page };
            let mut spans = vec![
                Span::styled(format!("{} ", state.mark()), look.bold),
                Span::styled(format!("{:<18}", j.command), style),
                Span::styled(format!("{:<40}", self.shown(&j.out)), style),
                Span::styled(format!(" {took}"), look.hint),
            ];
            match state {
                State::Running if !s.doing[i].is_empty() => {
                    spans.push(Span::styled(format!("  {}", s.doing[i]), look.hint));
                }
                State::Failed(why) => spans.push(Span::styled(format!("  {why}"), look.bold)),
                _ => {}
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), inner);

        let i = self.run_at.min(q.jobs.len().saturating_sub(1));
        let title = format!(" log · cocoa {} ", q.jobs[i].command);
        let inner = framed(f, log, title, look);
        let progress = s.progress[i].as_ref();
        let rows = usize::from(inner.height).saturating_sub(usize::from(progress.is_some()));
        let l = &s.logs[i];
        let scroll = self.log_scroll.min(l.len().saturating_sub(rows));
        let end = l.len() - scroll;
        let start = end.saturating_sub(rows);
        let mut lines: Vec<Line> = l
            .range(start..end)
            .map(|x| Line::styled(x.clone(), look.page))
            .collect();
        if let Some(p) = progress {
            let g = legume_tui::child::gauge(p, 28, &[], usize::from(inner.width));
            lines.push(Line::styled(g, look.bold));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }

    /// A gene's page: a line of where it is, then the page.
    fn draw_gene_page(&mut self, f: &mut Frame, body: Rect, look: &Look) {
        let [head, area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(body);
        let (Some(page), Some(r)) = (&mut self.gene, &self.results) else {
            return;
        };
        let at = r
            .order
            .iter()
            .position(|&g| g == page.gene)
            .map_or(0, |a| a + 1);
        let locus = if self.gv.genome.is_some() {
            format!(" · locus ±{:.2} Mb", page.flank as f32 / 1e6)
        } else {
            String::new()
        };
        let note = page.note.clone().unwrap_or_default();
        f.render_widget(
            Line::from(vec![
                Span::styled(
                    format!(
                        " gene {at} of {} by {}{locus} · ",
                        r.order.len(),
                        r.sort.name()
                    ),
                    look.bold,
                ),
                Span::styled(note, look.hint),
            ]),
            head,
        );
        let Some(picker) = &self.picker else { return };
        page.draw(f, area, r, self.gv.genome.as_ref(), picker);
    }

    /// The genome view: a line of what it shows, then the figure.
    fn draw_genome(&mut self, f: &mut Frame, body: Rect, look: &Look) {
        let [head, area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(body);
        let placed = match &self.gv.genome {
            Some(g) => format!("{} of {} genes placed", g.found(), g.at.len()),
            None => "genes in the data's order".into(),
        };
        let note = self.gv.note.clone().unwrap_or_else(|| placed.clone());
        f.render_widget(
            Line::from(vec![
                Span::styled(format!(" genome · {} · ", self.gv.window()), look.bold),
                Span::styled(note, look.hint),
            ]),
            head,
        );
        let (Some(r), Some(picker)) = (&self.results, &self.picker) else {
            return;
        };
        self.gv.draw(f, area, r, picker);
    }

    fn draw_results(&self, f: &mut Frame, body: Rect, look: &Look) {
        let Some(r) = &self.results else {
            let inner = framed(f, body, " results ".into(), look);
            let why = match &self.opening {
                Some((p, _)) => format!("reading {}…", legume_tui::browse::tilde(p)),
                None => self.results_error.clone().unwrap_or_default(),
            };
            f.render_widget(Paragraph::new(why).wrap(Wrap { trim: false }), inner);
            return;
        };
        let heat_h = match (&r.heat, self.show_heat) {
            (Some(h), true) => (h.genes.len() as u16 + 4).min(body.height / 2),
            _ => 0,
        };
        let [top, heat] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(heat_h)]).areas(body);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE)]).areas(top);
        let title = format!(
            " genes · {} · by {} · {} ",
            r.level_name(),
            r.sort.name(),
            self.shown(&r.prefix)
        );
        let inner = framed(f, left, title, look);
        let l = r.level;
        let perm = r.has_perm();
        let unadj = r.has_unadjusted();
        let mut lines = vec![Line::styled(
            format!(
                "  {:<14}{:>10}{}{}  flag",
                "gene",
                "contrast",
                if unadj { "  unadjusted" } else { "" },
                if perm { "         z    pvalue" } else { "" }
            ),
            look.hint,
        )];
        let rows = usize::from(inner.height).saturating_sub(1);
        let first = first_visible(r.at, r.order.len(), rows);
        let num = |x: f32, w: usize| {
            if x.is_finite() {
                format!("{x:>w$.3}")
            } else {
                format!("{:>w$}", "NA")
            }
        };
        let truth = r.truth.as_ref();
        for (k, &g) in r.order.iter().enumerate().skip(first).take(rows) {
            let gene = &r.genes[g];
            let on = k == r.at;
            let causal = truth.is_some_and(|t| t.genes.contains(&gene.name));
            let mut text = format!(
                "{}{:<14}{}",
                if causal { "● " } else { "  " },
                legume_tui::chrome::short(&gene.name, 13),
                num(gene.contrast[l], 10),
            );
            if unadj {
                text.push_str(&num(gene.unadjusted[l], 12));
            }
            if perm {
                let p = gene.p[l];
                let p = if p.is_finite() && p < 0.001 {
                    "    <0.001".to_string()
                } else {
                    num(p, 10)
                };
                text.push_str(&format!("{}{p}", num(gene.z[l], 10)));
            }
            text.push_str(&format!("  {}", gene.flag));
            let style = if on { look.selected } else { look.page };
            lines.push(Line::styled(text, style));
        }
        f.render_widget(Paragraph::new(lines), inner);

        let side_title = match &r.truth {
            Some(t) => format!(" vs truth · {} ", name(&t.file)),
            None => " summary ".into(),
        };
        let inner = framed(f, right, side_title, look);
        let mut lines = Vec::new();
        let f3 = |x: Option<f64>| x.map_or_else(|| "–".to_string(), |x| format!("{x:.2}"));
        if let Some((adj, un)) = r.score() {
            let (head, un) = match un {
                Some(u) => ("unadjusted", f3(u.auroc)),
                None => ("", String::new()),
            };
            lines.push(Line::styled(
                format!("{:<14}{:>10}{head:>12}", "", "adjusted"),
                look.hint,
            ));
            lines.push(Line::raw(format!(
                "{:<14}{:>10}{un:>12}",
                "AUROC",
                f3(adj.auroc)
            )));
            if let Some((k, power, fdr)) = adj.called {
                lines.push(Line::raw(format!(
                    "{:<14}{:>10}",
                    format!("called q≤{}", super::results::Q),
                    k
                )));
                lines.push(Line::raw(format!("{:<14}{:>10.2}", "power", power)));
                lines.push(Line::raw(format!("{:<14}{:>10.2}", "FDR", fdr)));
            } else {
                lines.push(Line::styled("no permutations: no p-values", look.hint));
                lines.push(Line::styled("(--n-permutations on Flags)", look.hint));
            }
            lines.push(Line::styled("● marks a causal gene", look.hint));
            lines.push(Line::raw(""));
        }
        if let Some((hist, sd)) = r.null_z(24) {
            let top = hist.iter().copied().max().unwrap_or(1).max(1);
            const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
            let spark: String = hist
                .iter()
                .map(|&n| BLOCKS[(n * 7).div_ceil(top).min(7)])
                .collect();
            let which = if r.truth.is_some() {
                "null genes"
            } else {
                "all genes"
            };
            lines.push(Line::styled(format!("z of {which}, -4 to 4"), look.hint));
            lines.push(Line::styled(spark, look.bold));
            lines.push(Line::raw(format!("sd {sd:.2} (1 when calibrated)")));
            lines.push(Line::raw(""));
        }
        if !r.left_out.is_empty() {
            lines.push(Line::styled("left out", look.hint));
            for (why, n) in &r.left_out {
                lines.push(Line::raw(format!(" {why} {n}")));
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

        if heat_h > 0 {
            if let Some(h) = &r.heat {
                let title = format!(
                    " log effect · top {} genes a topic · ±{:.2} ",
                    r.per_topic, h.clip
                );
                let inner = framed(f, heat, title, look);
                f.render_widget(Paragraph::new(heat_lines(h, inner.width, look)), inner);
            }
        }
    }

    fn preview_lines(&self, look: &Look, room: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        match &self.planned {
            Err(why) => {
                lines.push(Line::styled(" cannot start yet:", look.bold));
                lines.push(Line::styled(format!(" {why}"), look.page));
            }
            Ok(jobs) => {
                lines.push(Line::styled(" no problems", look.hint));
                for j in jobs {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled(
                        format!(" ── {}", self.shown(&j.script())),
                        look.bold,
                    ));
                    let head = if j.argv.get(1).is_some_and(|w| w == crate::runs::BATCH) {
                        2
                    } else {
                        1
                    };
                    for (k, l) in script::command_lines("cocoa", &j.argv, head)
                        .into_iter()
                        .enumerate()
                    {
                        let indent = if k == 0 { " " } else { "     " };
                        lines.push(Line::styled(format!("{indent}{l}"), look.page));
                    }
                    lines.push(Line::styled(
                        format!("   runs in {}", self.shown(&j.dir)),
                        look.hint,
                    ));
                }
            }
        }
        let body: Vec<Line> = lines
            .into_iter()
            .skip(self.preview_scroll)
            .take(room.saturating_sub(2))
            .collect();
        let mut out = body;
        out.push(Line::raw(""));
        out.push(if self.planned.is_ok() {
            keys(
                &[("Enter", "starts"), ("Esc", "back"), ("↑↓", "scroll")],
                look,
            )
        } else {
            keys(&[("Esc", "back")], look)
        });
        out
    }

    fn ended_lines(&self, look: &Look) -> Vec<Line<'static>> {
        let Some(q) = &self.queue else {
            return Vec::new();
        };
        let mut lines = super::logo::heading(&q.status(), "", look.bold, look.hint);
        lines.push(Line::raw(""));
        let s = q.shared.lock();
        for (i, j) in q.jobs.iter().enumerate() {
            let (state, took) = s
                .as_ref()
                .ok()
                .and_then(|s| Some((s.states.get(i)?.clone(), *s.took.get(i)?)))
                .unwrap_or((State::Stopped, None));
            let why = match &state {
                State::Failed(w) => format!("  {w}"),
                _ => String::new(),
            };
            lines.push(Line::raw(format!(
                " {} cocoa {}  {}  {}{why}",
                state.mark(),
                j.command,
                self.shown(&j.out),
                took.map(clock).unwrap_or_default()
            )));
        }
        drop(s);
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            " each run's script is beside it: {out}.cmd.sh",
            look.hint,
        ));
        let mut k: Vec<(&str, &str)> = Vec::new();
        if self.can_diff_on_sim() {
            k.push(("d", "diff on this"));
        }
        if self.results.is_some() {
            k.push(("any key", "the results"));
        } else {
            k.push(("any key", "closes"));
        }
        lines.push(keys(&k, look));
        lines
    }

    fn footer(&self, look: &Look) -> Line<'static> {
        if let Some((target, field)) = &self.editing {
            let what = match target {
                Target::SimOut => {
                    "simulation's output prefix (empty: the next free one)".to_string()
                }
                Target::DiffOut => "diff's output prefix (empty: the next free one)".to_string(),
                Target::Model(i) => self
                    .sim
                    .as_ref()
                    .map_or_else(String::new, |s| format!("--{}", s.model.rows()[*i].long)),
                Target::Flag(of, i) => format!("--{}", self.form(*of).fields[*i].long),
                Target::Find => "flags containing".to_string(),
            };
            let mut spans = vec![Span::styled(format!(" {what}: "), look.bold)];
            spans.extend(field.lines(200, true, look.page, look.selected));
            spans.push(Span::styled("   Enter takes  Esc drops", look.hint));
            return Line::from(spans);
        }
        let common: &[(&str, &str)] = &[("Tab", "tabs"), ("Ctrl+R", "preview"), ("?", "keys")];
        let own: Vec<(&str, &str)> = match self.page {
            Page::Model => vec![
                ("↑↓", "row"),
                ("←→", "step"),
                ("Enter", "type"),
                ("m", "model"),
                ("d", "then diff"),
                ("o", "output"),
            ],
            Page::Inputs => vec![
                ("type", "find, / ~ go to a path"),
                ("Space", "role"),
                ("Del", "none"),
                ("Enter/→", "open"),
                ("←", "up"),
                ("Ctrl+O", "output"),
                ("Ctrl+C", "quits"),
            ],
            Page::Flags => vec![
                ("↑↓", "flag"),
                ("Enter", "change"),
                ("←→", "choice"),
                ("Backspace", "default"),
                ("/", "find"),
                ("a", "advanced"),
            ],
            Page::Run => {
                let mut k = vec![("[ ]", "job"), ("↑↓", "scroll"), ("End", "follow")];
                if self.running() {
                    k.push(("s", "stop"));
                }
                if self.can_diff_on_sim() {
                    k.push(("d", "diff on this"));
                }
                k.push(("Ctrl+Alt+D", "detach, runs go on"));
                k
            }
            Page::Results if self.gene.is_some() => vec![
                ("n N", "next, previous gene"),
                ("/", "find a gene"),
                ("+ -", "locus"),
                ("l", "level"),
                ("g", "annotation"),
                ("p", "save PDF"),
                ("Esc", "back"),
            ],
            Page::Results if self.gv.on => vec![
                ("/", "find a gene"),
                ("←→", "scroll"),
                ("+ -", "zoom"),
                ("[ ]", "chromosome"),
                ("Home End", "ends"),
                ("v", "table"),
                ("g", "annotation"),
                ("l", "level"),
                ("p", "save PDF (whole genome)"),
            ],
            Page::Results => vec![
                ("↑↓", "gene"),
                ("Enter", "its page"),
                ("/", "find"),
                ("s", "sort"),
                ("l", "level"),
                ("h", "heatmap"),
                ("+ -", "genes a topic"),
                ("v", "genome"),
                ("i", "inset"),
                ("g", "annotation"),
                ("p", "save PDF"),
            ],
            Page::Agent => vec![("o", "open its results"), ("Tab", "tabs")],
        };
        let mut all = own;
        if self.page != Page::Agent {
            all.extend_from_slice(common);
        }
        if !matches!(self.page, Page::Inputs | Page::Agent) {
            all.push(("q", "quit"));
        }
        keys(&all, look)
    }
}

/// The file name of `p`, else `p` whole.
pub(super) fn name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The heatmap as text: each gene's row of cells coloured on legume-tui's
/// diverging scale, a topic a column.
fn heat_lines(h: &super::results::Heat, width: u16, look: &Look) -> Vec<Line<'static>> {
    use legume_tui::chart::heatmap::cell_colour;
    const NAME: usize = 12;
    let cols = h.topics.len();
    let cell = ((usize::from(width).saturating_sub(NAME + 1)) / cols.max(1)).clamp(1, 4);
    let mut head = vec![Span::raw(" ".repeat(NAME + 1))];
    for t in &h.topics {
        head.push(Span::styled(
            format!("{:<cell$}", legume_tui::chrome::short(t, cell)),
            look.hint,
        ));
    }
    let mut lines = vec![Line::from(head)];
    for (r, g) in h.genes.iter().enumerate() {
        let mut spans = vec![Span::styled(
            format!("{:<NAME$} ", legume_tui::chrome::short(g, NAME)),
            look.page,
        )];
        for c in 0..cols {
            let v = h.v[r * cols + c];
            let [red, green, blue] = cell_colour(v, h.clip, [96, 96, 96]);
            spans.push(Span::styled(
                " ".repeat(cell),
                Style::new().bg(Color::Rgb(red, green, blue)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// The gene finder as the lines of a pop-up.
fn finder_lines(
    list: &legume_tui::browse::FindList<String>,
    look: &Look,
    rows: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(" find a gene: ", look.bold),
            Span::styled(format!("{}▏", list.find.text), look.page),
        ]),
        Line::raw(""),
    ];
    if list.is_empty() {
        lines.push(Line::styled(format!(" {}", list.empty_note("")), look.hint));
    } else {
        let shown: Vec<Line<'static>> = list
            .shown()
            .enumerate()
            .map(|(k, s)| {
                let on = k == list.at;
                Line::from(vec![
                    Span::styled(if on { " ▸ " } else { "   " }, look.bold),
                    Span::styled(s.clone(), if on { look.selected } else { look.page }),
                ])
            })
            .collect();
        lines.extend(window(shown, list.at, rows));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " type a name  ↑↓ pick  Enter its page  Esc closes",
        look.hint,
    ));
    lines
}

fn help_lines(look: &Look) -> Vec<Line<'static>> {
    let k = |key: &str, what: &str| {
        Line::from(vec![
            Span::styled(format!(" {key:<12}"), look.bold),
            Span::raw(what.to_string()),
        ])
    };
    let mut lines = super::logo::heading(
        &format!("cocoa {}", env!("CARGO_PKG_VERSION")),
        "counterfactual confounder adjustment",
        look.bold,
        look.hint,
    );
    lines.push(Line::raw(""));
    lines.extend([
        Line::styled(" every tab", look.hint),
        k("Tab", "the next tab (Shift+Tab back)"),
        k(
            "Ctrl+R",
            "preview the exact commands; Enter there starts them",
        ),
        k("W", "follow an agent's `cocoa mcp`"),
        k(
            "Ctrl+Alt+D",
            "detach: the runs go on; `cocoa attach` returns",
        ),
        k("Ctrl+C", "quit, stopping the runs"),
        Line::raw(""),
        Line::styled(" Model", look.hint),
        k(
            "←→",
            "an edge's PVE by 0.05, a size to the next round number",
        ),
        k("m d o", "the other model, a diff after it, the output"),
        Line::styled(" Inputs", look.hint),
        k("Space", "the file's role: its name's guess, then the next"),
        k("Ctrl+O", "name the output"),
        Line::styled(" Results", look.hint),
        k("s l h", "sort, contrast level, heatmap"),
        k("+ - p", "genes a topic on the heatmap, save it as a PDF"),
        k("v g", "the genome view, the annotation that places genes"),
        k(
            "Enter /",
            "a gene's page: the highlighted one, or one typed",
        ),
        k("n N + -", "on a page: next, previous gene; its locus"),
        Line::raw(""),
        k("Esc", "closes this"),
    ]);
    lines
}
