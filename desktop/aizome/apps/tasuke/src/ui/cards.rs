//! A transcript's steps: thinking, one line per call (opening to what it
//! printed), runs of looking folded into one line, the approval card, a
//! screenshot's thumbnail, notices and errors.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::Cursor;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};

use crate::agent::{Decision, Kind, Level};
use crate::app::{ConvId, Conversation, Msg, Tasuke};
use crate::approval::Gate;
use crate::diff;
use crate::fold::Run;
use crate::thumb::{self, Thumb};
use crate::transcript::{Body, Reasoning, Tool, ToolState};
use crate::widgets::prose::prose;

/// Where a step's details start, under its glyph.
const INDENT: f64 = 22.0;
/// The most lines of output shown: the head and the tail.
const HEAD: usize = 40;
const TAIL: usize = 20;

fn chevron(open: bool) -> Element<Msg> {
    icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }).size(13.0).role(Role::Faint).into()
}

/// A line that opens and closes with a click.
fn opener(line: impl Into<Element<Msg>>, conv: ConvId, key: u64) -> Element<Msg> {
    mouse_area(line).on_press(Msg::Toggle(conv, key)).cursor(Cursor::Pointer).width(Fill).into()
}

pub fn reasoning(c: &Conversation, key: u64, r: &Reasoning) -> Element<Msg> {
    let open = c.open.contains(&key);
    let words = match r.seconds() {
        Some(s) => format!("Thought for {s} s"),
        None => "Thinking…".to_owned(),
    };
    // Its first words, while it's closed: what it was about.
    let gist = r.text.split_whitespace().take(24).collect::<Vec<_>>().join(" ");
    let (glyph, role) = if r.ended.is_none() { (Icon::Ellipsis, Role::Accent) } else { (Icon::Ellipsis, Role::Text3) };
    let line = row![
        icon(glyph).size(14.0).role(role),
        text(words).size(12.5).role(Role::Text3),
        text(if open { String::new() } else { gist }).size(12.0).italic().role(Role::Faint).width(Fill),
        chevron(open),
    ]
    .spacing(8.0)
    .padding([3.0, 0.0])
    .width(Fill);
    let head = opener(line, c.id, key);
    if !open {
        return head;
    }
    let body = container(
        prose([(Span::new(r.text.trim().to_owned()).italic(), None)])
            .font(FontSpec::ui(13.0))
            .role(Role::Text2)
            .line_height(1.45),
    )
    .edges(Edge::Line2, Sides::LEFT)
    .padding([2.0, 0.0, 4.0, 12.0])
    .width(Fill);
    column![head, container(body).padding([2.0, 0.0, 4.0, 6.0]).width(Fill)].width(Fill).into()
}

/// A policy's reason as the end of a sentence: "It deletes files" reads
/// "it deletes files" after "Why it asks:" (but "SSH keys" stays).
fn clause(reason: &str) -> String {
    let r = reason.trim().trim_end_matches('.');
    let mut chars = r.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_lowercase() => {
            a.to_lowercase().chain(r[a.len_utf8()..].chars()).collect()
        }
        _ => r.to_owned(),
    }
}

/// A call's state as a glyph, in a colour, never by colour alone.
fn state_glyph(s: &ToolState) -> (Icon, Role) {
    match s {
        ToolState::Waiting => (Icon::Hourglass, Role::Text3),
        ToolState::Running => (Icon::CircleDot, Role::Accent),
        ToolState::Done { .. } => (Icon::CircleCheck, Role::Success),
        ToolState::Failed { .. } | ToolState::TimedOut => (Icon::CircleX, Role::Danger),
        ToolState::Denied => (Icon::CircleMinus, Role::Text3),
        ToolState::Refused { .. } => (Icon::Shield, Role::Warning),
        ToolState::Stopped => (Icon::Square, Role::Text3),
    }
}

/// What the line says after the target, when it's worth saying.
fn state_note(s: &ToolState) -> String {
    match s {
        ToolState::Done { .. } => String::new(),
        ToolState::Failed { exit: Some(code) } => format!("exit {code}"),
        ToolState::Refused { .. } => "never run".into(),
        other => other.words(),
    }
}

/// A call's output, its middle left out when long.
fn output_text(t: &Tool) -> String {
    let Some(o) = &t.output else { return String::new() };
    let lines: Vec<&str> = o.text.lines().collect();
    let mut s = if lines.len() > HEAD + TAIL {
        let left = lines.len() - HEAD - TAIL;
        format!("{}\n… {left} more lines …\n{}", lines[..HEAD].join("\n"), lines[lines.len() - TAIL..].join("\n"))
    } else {
        o.text.trim_end().to_owned()
    };
    if o.truncated {
        s.push_str("\n… (the agent kept only the start and end of this output)");
    }
    s
}

/// What a call printed, and why it was run, under its line.
fn details(t: &Tool) -> Element<Msg> {
    let mut parts: Vec<Element<Msg>> = Vec::new();
    if let Some(p) = &t.described.purpose {
        parts.push(text(format!("Why: {p}")).size(12.0).role(Role::Text3).wrap().line_height(1.4).into());
    }
    if t.call.name == "invalid"
        && let Some(e) = t.call.args.get("error").and_then(serde_json::Value::as_str)
    {
        parts.push(
            text(format!("The model asked for this in a way Tasuke couldn't read: {e}"))
                .size(12.0)
                .role(Role::Text3)
                .wrap()
                .line_height(1.4)
                .into(),
        );
    }
    if t.described.code && t.described.target.contains('\n') {
        parts.push(code_box(&t.described.target));
    }
    if let ToolState::Refused { reason } = &t.state {
        parts.push(
            text(format!("Tasuke never runs this: {reason}."))
                .size(12.0)
                .role(Role::Warning)
                .wrap()
                .line_height(1.4)
                .into(),
        );
    }
    let out = output_text(t);
    if !out.is_empty() {
        parts.push(output_box(&out));
    } else if matches!(t.state, ToolState::Done { .. }) {
        parts.push(text("It printed nothing.").size(12.0).role(Role::Text3).into());
    }
    container(column_of(parts).spacing(6.0).width(Fill)).padding([2.0, 0.0, 6.0, INDENT]).width(Fill).into()
}

fn code_box(s: &str) -> Element<Msg> {
    container(prose([(Span::new(s.to_owned()), None)]).font(FontSpec::mono(12.0)).role(Role::Text2).line_height(1.4))
        .padding([8.0, 10.0])
        .fill(Tone::Sunk)
        .border(Edge::Line)
        .width(Fill)
        .into()
}

/// One call as one line: its state, what it did, to what.
fn line(t: &Tool) -> Element<Msg> {
    // A look that exited non-zero still answered: quietly, not in red.
    let answered = t.looks && matches!(t.state, ToolState::Failed { exit: Some(_) });
    let (glyph, role) = if answered { (Icon::Circle, Role::Text3) } else { state_glyph(&t.state) };
    let d = &t.described;
    let verb = if t.state.is_open() { d.doing } else { d.done };
    let target_line = d.target.lines().next().unwrap_or("").to_owned();
    let target: Element<Msg> = if d.code {
        text(target_line).mono().size(12.0).role(Role::Text2).width(Fill).into()
    } else {
        text(target_line).size(12.5).role(Role::Text2).width(Fill).into()
    };
    let note = state_note(&t.state);
    row![
        icon(glyph).size(14.0).role(role),
        text(verb).size(12.5).weight(600.0).role(Role::Text2),
        target,
        text(note).size(11.5).role(if t.state.is_failure() && !answered { Role::Danger } else { Role::Text3 }),
    ]
    .spacing(8.0)
    .padding([3.0, 0.0])
    .width(Fill)
    .into()
}

pub fn tool(app: &Tasuke, c: &Conversation, key: u64, t: &Tool) -> Element<Msg> {
    if let Some(g) = &t.gate {
        return approval(c, key, t, g);
    }
    if t.call.name == "screenshot" {
        return screenshot(app, c, key, t);
    }
    step(c, key, t)
}

/// A line that opens to its details.
fn step(c: &Conversation, key: u64, t: &Tool) -> Element<Msg> {
    let open = c.open.contains(&key);
    let head = opener(row![line(t), chevron(open)].spacing(6.0).width(Fill), c.id, key);
    if open { column![head, details(t)].width(Fill).into() } else { head }
}

/// A run of looking: "Looked at 6 things", opening to each.
pub fn looked(_app: &Tasuke, c: &Conversation, run: &Run) -> Element<Msg> {
    let items = &c.transcript.items;
    let h = run.headline(items);
    let open = c.open.contains(&run.key);
    let (glyph, role) = if h.open {
        (Icon::CircleDot, Role::Accent)
    } else if h.failed > 0 {
        (Icon::TriangleAlert, Role::Warning)
    } else {
        (Icon::CircleCheck, Role::Success)
    };
    let mut words = h.words();
    if h.failed > 0 && !h.open {
        words.push_str(&format!(" · {} didn't finish", h.failed));
    }
    let line = row![
        icon(glyph).size(14.0).role(role),
        text(words).size(12.5).weight(600.0).role(Role::Text2),
        text(h.summary()).mono().size(11.5).role(Role::Text3).width(Fill),
        chevron(open),
    ]
    .spacing(8.0)
    .padding([3.0, 0.0])
    .width(Fill);
    let head = opener(line, c.id, run.key);
    if !open {
        return head;
    }
    let inner: Vec<Element<Msg>> = run
        .items
        .iter()
        .map(|&i| {
            let item = &items[i];
            let el = match &item.body {
                Body::Tool(t) => step(c, item.id, t),
                Body::Reasoning(r) => reasoning(c, item.id, r),
                _ => space(0.0),
            };
            el.key(item.id)
        })
        .collect();
    column![
        head,
        container(column_of(inner).spacing(2.0).width(Fill))
            .edges(Edge::Line2, Sides::LEFT)
            .padding([2.0, 0.0, 4.0, 14.0])
            .width(Fill),
    ]
    .spacing(2.0)
    .width(Fill)
    .into()
}

/// A diff, its lines coloured by what they do.
fn diff_box(d: &str) -> Element<Msg> {
    let mut spans = Vec::new();
    for (i, l) in d.lines().enumerate() {
        if i > 0 {
            spans.push((Span::new("\n"), None));
        }
        let role = match diff::kind(l) {
            diff::Line::Added => Role::Success,
            diff::Line::Removed => Role::Danger,
            diff::Line::Note => Role::Cyan,
            diff::Line::Same => Role::Text2,
        };
        spans.push((Span::new(l.to_owned()).role(role), None));
    }
    container(prose(spans).font(FontSpec::mono(12.0)).line_height(1.4))
        .padding([8.0, 10.0])
        .fill(Tone::Sunk)
        .border(Edge::Line)
        .width(Fill)
        .into()
}

/// The approval card: the exact command (or change), why the model wants
/// it, why it asks, and Run or Don't run, neither the default. Answered,
/// it stays, saying what was decided and what came of it.
fn approval(c: &Conversation, key: u64, t: &Tool, g: &Gate) -> Element<Msg> {
    let d = &t.described;
    let root = matches!(d.kind, Kind::RootCommand)
        || matches!(g, Gate::Pending { root: true, .. } | Gate::Sent { root: true });
    let pending = g.is_pending();
    let question = match (&d.kind, root) {
        (Kind::FileChange { .. }, _) => "Change this file?",
        (_, true) => "Run this as administrator?",
        (Kind::Desktop, _) => "Use the desktop?",
        _ => "Run this command?",
    };
    let (glyph, role) = if pending { (Icon::Shield, Role::Warning) } else { (Icon::ShieldCheck, Role::Text3) };
    let mut parts: Vec<Element<Msg>> =
        vec![row![icon(glyph).size(16.0).role(role), text(question).size(14.0).weight(650.0)].spacing(8.0).into()];
    if let Some(p) = &d.purpose {
        parts.push(prose([(Span::new(p.clone()), None)]).font(FontSpec::ui(13.5)).line_height(1.45).into());
    }
    match &d.kind {
        Kind::FileChange { path, diff } => {
            parts.push(text(path.clone()).mono().size(12.0).role(Role::Text2).into());
            parts.push(diff_box(diff));
        }
        _ => parts.push(code_box(&d.target)),
    }
    if let Gate::Pending { reason, root } = g {
        let why = format!("Why it asks: {}.", clause(reason));
        parts.push(
            row![
                super::lead(icon(Icon::Info).size(13.0).role(Role::Text3)),
                text(why).size(12.0).role(Role::Text2).wrap().line_height(1.4)
            ]
            .spacing(8.0)
            .align(Align::Start)
            .into(),
        );
        if *root {
            parts.push(
                row![
                    super::lead(icon(Icon::Lock).size(13.0).role(Role::Text3)),
                    text("After you choose Run as administrator, the system asks for your password.")
                        .size(12.0)
                        .role(Role::Text2)
                        .wrap()
                        .line_height(1.4),
                ]
                .spacing(8.0)
                .align(Align::Start)
                .into(),
            );
        }
        let run = if *root { "Run as administrator" } else { "Run" };
        let call = t.call_id.clone();
        let id = c.id;
        parts.push(
            row![
                button(run).on_press(Msg::Decide(id, call.clone(), Decision::Allow)),
                button("Don't run").on_press(Msg::Decide(id, call, Decision::Deny { note: None })),
            ]
            .spacing(8.0)
            .padding([4.0, 0.0, 0.0, 0.0])
            .into(),
        );
    } else {
        let (gl, rl, words) = decision_words(g, &t.state, root);
        let open = c.open.contains(&key);
        let has_output = t.output.as_ref().is_some_and(|o| !o.text.trim().is_empty());
        let mut line =
            row![icon(gl).size(14.0).role(rl), text(words).size(12.5).role(Role::Text2).width(Fill)].spacing(8.0);
        if has_output {
            line = line
                .push(text(if open { "Hide output" } else { "Show output" }).size(12.0).role(Role::Accent))
                .push(chevron(open));
        }
        let line: Element<Msg> = if has_output { opener(line, c.id, key) } else { line.into() };
        parts.push(container(line).padding([2.0, 0.0, 0.0, 0.0]).width(Fill).into());
        if open && has_output {
            parts.push(output_box(&output_text(t)));
        }
    }
    let edge = if pending { Edge::Focus } else { Edge::Line };
    container(column_of(parts).spacing(10.0).width(Fill))
        .padding([14.0, 16.0])
        .fill(Tone::Chrome)
        .border(edge)
        .width(Fill)
        .into()
}

/// What was decided, and what came of it.
fn decision_words(g: &Gate, s: &ToolState, root: bool) -> (Icon, Role, String) {
    match (g, s) {
        (Gate::Sent { root: true }, _) => {
            (Icon::Lock, Role::Accent, "You chose Run as administrator · waiting for your password…".into())
        }
        (Gate::Sent { .. }, _) => (Icon::Hourglass, Role::Accent, "You chose Run · starting…".into()),
        (Gate::Allowed, ToolState::Running) => (Icon::CircleDot, Role::Accent, "You chose Run · running…".into()),
        (Gate::Allowed, ToolState::Done { .. }) => {
            let who = if root { "You chose Run as administrator" } else { "You chose Run" };
            (Icon::CircleCheck, Role::Success, format!("{who} · done"))
        }
        (Gate::Allowed, ToolState::Failed { exit: Some(code) }) => {
            (Icon::CircleX, Role::Danger, format!("You chose Run · it failed (exit {code})"))
        }
        (Gate::Allowed, ToolState::TimedOut) => {
            (Icon::CircleX, Role::Danger, "You chose Run · it took too long and was stopped".into())
        }
        (Gate::Allowed, _) => (Icon::CircleCheck, Role::Text3, "You chose Run".into()),
        (Gate::Denied { .. }, _) => {
            (Icon::CircleMinus, Role::Text3, "You chose Don't run · nothing was changed".into())
        }
        (Gate::Withdrawn, _) => (Icon::Square, Role::Text3, "The turn ended before you answered · not run".into()),
        (Gate::Pending { .. }, _) => (Icon::Hourglass, Role::Text3, "Waiting for you".into()),
    }
}

/// A screenshot the agent took: its line, and the picture small.
fn screenshot(app: &Tasuke, c: &Conversation, key: u64, t: &Tool) -> Element<Msg> {
    let head = step(c, key, t);
    let Some(path) = t.output.as_ref().and_then(|o| o.image.clone()) else { return head };
    let w = f64::from(thumb::WIDTH);
    let picture: Element<Msg> = match app.thumb(&path) {
        Some(Thumb::Ready(img)) => {
            let size = img.size();
            let framed =
                container(image(img.clone()).width(size.width).height(size.height)).border(Edge::Line).padding(1.0);
            mouse_area(framed)
                .on_press(Msg::OpenLink(format!("file://{}", path.display())))
                .cursor(Cursor::Pointer)
                .tooltip("Open the screenshot")
                .into()
        }
        Some(Thumb::Missing(why)) => text(why.clone()).size(12.0).role(Role::Text3).into(),
        _ => container(text("Loading the picture…").size(12.0).role(Role::Text3))
            .center()
            .width(w)
            .height(w * 9.0 / 16.0)
            .fill(Tone::Sunk)
            .border(Edge::Line)
            .into(),
    };
    column![head, container(picture).padding([4.0, 0.0, 4.0, INDENT])].width(Fill).into()
}

/// How a notice stands: still going on (the turn waits on it), over, or
/// over and followed by an error (what it was doing didn't work).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeNow {
    Going,
    Over,
    Failed,
}

/// "Starting the model server…" while it's going on, and "Started the
/// model server" once the turn has moved on; the agent says the first.
pub fn progress_words(words: &str, now: NoticeNow) -> Option<String> {
    let doing = words.strip_suffix('…')?;
    match now {
        NoticeNow::Over => doing.strip_prefix("Starting ").map(|rest| format!("Started {rest}")),
        _ => Some(words.to_owned()),
    }
}

pub fn notice(level: Level, words: &str, now: NoticeNow) -> Element<Msg> {
    let (mut glyph, mut role, mut text_role) = match level {
        Level::Info => (Icon::Info, Role::Text3, Role::Text3),
        Level::Warning => (Icon::TriangleAlert, Role::Warning, Role::Text2),
        Level::Error => (Icon::CircleAlert, Role::Danger, Role::Text2),
    };
    let mut shown = words.to_owned();
    // Something under way the turn waits on (the server starting): it says
    // so as it happens, and that it happened.
    if let Some(p) = progress_words(words, now) {
        shown = p;
        (glyph, role, text_role) = match now {
            NoticeNow::Going => (Icon::Hourglass, Role::Accent, Role::Text2),
            NoticeNow::Over => (Icon::CircleCheck, Role::Success, Role::Text3),
            NoticeNow::Failed => (glyph, role, text_role),
        };
    }
    row![super::lead(icon(glyph).size(13.0).role(role)), text(shown).size(12.0).role(text_role).wrap().line_height(1.4)]
        .spacing(8.0)
        .align(Align::Start)
        .padding([2.0, 0.0])
        .width(Fill)
        .into()
}

/// An error as a headline and its detail: the agent's "the model server
/// can't be reached: connection refused" reads "The model server can't be
/// reached", then "Connection refused" quietly under it.
pub fn error_parts(words: &str) -> (String, Option<String>) {
    let cap = |s: &str| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
    };
    match words.split_once(": ") {
        Some((head, detail)) if head.chars().count() <= 70 && !detail.trim().is_empty() => {
            (cap(head.trim()), Some(cap(detail.trim())))
        }
        _ => (cap(words.trim()), None),
    }
}

pub fn error(conv: ConvId, words: &str, retryable: bool) -> Element<Msg> {
    let (head, detail) = error_parts(words);
    let server = head.to_lowercase().contains("model server");
    let mut lines = column![text(head).size(13.5).weight(600.0).wrap().line_height(1.4)].spacing(2.0).width(Fill);
    if let Some(d) = detail {
        lines = lines.push(text(d).size(12.0).role(Role::Text3).wrap().line_height(1.4));
    }
    if server {
        lines = lines.push(
            text("Models shows the server's state. Retry sends your message again.")
                .size(12.0)
                .role(Role::Text3)
                .wrap()
                .line_height(1.4),
        );
    }
    let mut buttons = aizome_kit::widgets::layout::row(Vec::new()).spacing(6.0);
    if server {
        buttons = buttons.push(button("Models").on_press(Msg::Show(crate::app::Page::Models)));
    }
    if retryable {
        buttons = buttons.push(button("Retry").icon(Icon::RotateCw).on_press(Msg::Retry(conv)));
    }
    let line = row![super::lead(icon(Icon::CircleAlert).size(15.0).role(Role::Danger)), lines, buttons]
        .spacing(10.0)
        .align(Align::Start)
        .width(Fill);
    container(line).padding([12.0, 14.0]).fill(Tone::Chrome).border(Edge::Line).width(Fill).into()
}

/// A command's output in a sunken box, with a button to copy it.
fn output_box(s: &str) -> Element<Msg> {
    let copy = icon_button(Icon::Copy).on_press(Msg::Copy(s.to_owned())).tooltip("Copy the output");
    container(
        row![
            prose([(Span::new(s.to_owned()), None)]).font(FontSpec::mono(12.0)).role(Role::Text2).line_height(1.4),
            copy
        ]
        .spacing(8.0)
        .align(Align::Start)
        .width(Fill),
    )
    .padding([6.0, 4.0, 6.0, 10.0])
    .fill(Tone::Sunk)
    .border(Edge::Line)
    .width(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_a_headline_and_a_detail() {
        assert_eq!(
            error_parts("the model server can't be reached: Connection refused (os error 111)"),
            ("The model server can't be reached".to_owned(), Some("Connection refused (os error 111)".to_owned()))
        );
        assert_eq!(
            error_parts("The model gave an empty answer."),
            ("The model gave an empty answer.".to_owned(), None)
        );
    }

    #[test]
    fn a_notice_of_something_under_way_says_when_it_happened() {
        let w = "Starting the model server…";
        assert_eq!(progress_words(w, NoticeNow::Going).as_deref(), Some(w));
        assert_eq!(progress_words(w, NoticeNow::Over).as_deref(), Some("Started the model server"));
        assert_eq!(progress_words(w, NoticeNow::Failed).as_deref(), Some(w));
        assert_eq!(progress_words("Trimmed older command output to make room.", NoticeNow::Over), None);
    }
}
