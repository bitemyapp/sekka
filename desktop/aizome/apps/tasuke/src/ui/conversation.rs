//! A conversation: its first page (a greeting, the model, suggestions)
//! until something is sent, then its transcript; the composer under both.

use std::rc::Rc;

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::{column as column_of, row as row_of};
use aizome_kit::{column, row};

use super::READING;
use super::cards;
use crate::app::{ConvId, Conversation, ModelState, Msg, Page, Tasuke, Turn};
use crate::fold::{self, Row};
use crate::markdown::view::{self as md, Look};
use crate::transcript::Body;
use crate::widgets::composer::composer;
use crate::widgets::follow::follow_scroll;
use crate::widgets::prose::prose;

/// Answers' text size.
pub(crate) const ANSWER: f64 = 14.0;

pub(crate) fn look(role: Role, size: f64) -> Look<Msg> {
    Look { size, open: Rc::new(Msg::OpenLink), copy: Rc::new(Msg::Copy), role }
}

pub fn view(app: &Tasuke, c: &Conversation) -> Element<Msg> {
    let body = if c.is_empty() { home(app, c) } else { transcript(app, c) };
    column![container(body).width(Fill).height(Fill), composer_bar(app, c)].width(Fill).height(Fill).into()
}

/// What a suggestion is: the kind of help it shows.
struct Suggestion {
    icon: Icon,
    text: &'static str,
    kind: &'static str,
}

static SUGGESTIONS: [Suggestion; 6] = [
    Suggestion { icon: Icon::Activity, text: "Give my computer a check-up", kind: "Check-up" },
    Suggestion { icon: Icon::HardDrive, text: "Why is my disk filling up?", kind: "Find a problem" },
    Suggestion { icon: Icon::Sun, text: "Switch to the Day theme", kind: "Do it for me" },
    Suggestion { icon: Icon::ScreenShare, text: "How do I share my screen?", kind: "How to" },
    Suggestion { icon: Icon::Volume2, text: "My sound stopped working", kind: "Fix a problem" },
    Suggestion { icon: Icon::MessageSquare, text: "What's a swap file?", kind: "Just ask" },
];

fn suggestion(id: ConvId, s: &'static Suggestion, hovered: bool) -> Element<Msg> {
    let card = container(
        row![
            icon(s.icon).size(16.0).role(Role::Accent),
            column![text(s.text).size(13.5), text(s.kind).size(11.0).role(Role::Text3)].spacing(2.0).width(Fill),
        ]
        .spacing(12.0),
    )
    .padding([10.0, 14.0])
    .fill(if hovered { Tone::Raised } else { Tone::Chrome })
    .border(if hovered { Edge::Line2 } else { Edge::Line })
    .width(Fill);
    let which = s.text;
    mouse_area(card)
        .on_press(Msg::Suggest(id, s.text.to_owned()))
        .on_hover(move |inside| Msg::Hover(inside.then_some(which)))
        .cursor(aizome_kit::sidestep_ui::Cursor::Pointer)
        .width(Fill)
        .key(s.text)
        .into()
}

/// The model as a line: "Gemma 4 31B runs on this computer · asleep".
fn model_line(app: &Tasuke) -> Element<Msg> {
    let (glyph, role) = state_glyph(app.chip.state);
    let name = &app.chip.name;
    let words = match app.chip.state {
        _ if name.is_empty() => "No model is downloaded yet. Choose one in Models.".to_owned(),
        ModelState::Ready => format!("{name} is ready, here on this computer."),
        ModelState::Asleep => format!("{name} is asleep here on this computer. It wakes when you send something."),
        ModelState::Loading => format!("{name} is waking up…"),
        ModelState::Missing => format!("{name} isn't downloaded yet. Models has it."),
        ModelState::Off => {
            format!("{name} is here, but the model server isn't running. It starts when you send something.")
        }
        ModelState::Failed => format!("{name} couldn't be loaded last time. Models says more."),
        ModelState::Cloud(p) => format!("{name} runs in {}'s cloud, through your account.", p.company()),
        ModelState::NoAccount(p) => {
            format!("{name} needs your {} account, which isn't connected. Models connects it.", p.company())
        }
    };
    row![icon(glyph).size(13.0).role(role), text(words).size(12.0).role(Role::Text3)].spacing(6.0).into()
}

fn home(app: &Tasuke, c: &Conversation) -> Element<Msg> {
    let id = c.id;
    let pairs: Vec<Element<Msg>> = SUGGESTIONS
        .chunks(2)
        .map(|pair| {
            row_of(pair.iter().map(|s| suggestion(id, s, app.hovered == Some(s.text))).collect())
                .spacing(10.0)
                .width(Fill)
                .into()
        })
        .collect();
    let greeting = format!("{}.", app.clock().greeting());
    let content = column![
        text(greeting).size(28.0).weight(700.0),
        text("Ask about your computer, or ask me to do something. I look around freely, and change nothing until you say Run.")
            .size(14.0)
            .role(Role::Text2)
            .wrap().line_height(1.4),
        container(model_line(app)).padding([2.0, 0.0, 10.0, 0.0]),
        column_of(pairs).spacing(10.0).width(Fill),
        container(privacy(app)).padding([10.0, 0.0, 0.0, 0.0]),
    ]
    .spacing(8.0)
    .max_width(600.0);
    container(content).center().padding([24.0, 32.0]).width(Fill).height(Fill).into()
}

/// Where what's asked goes: nowhere, or to the cloud model's company.
fn privacy(app: &Tasuke) -> Element<Msg> {
    let (glyph, words) = match app.chip.state {
        ModelState::Cloud(p) | ModelState::NoAccount(p) => (
            Icon::Cloud,
            format!(
                "Through your {} account: what you ask, and what Tasuke sees while it works, is sent to {}.",
                p.company(),
                p.company()
            ),
        ),
        _ => (Icon::Lock, "Private: it runs on this computer, and nothing you ask leaves it.".to_owned()),
    };
    row![
        super::lead(icon(glyph).size(13.0).role(Role::Text3)),
        text(words).size(12.0).role(Role::Text3).wrap().line_height(1.4)
    ]
    .spacing(6.0)
    .align(Align::Start)
    .into()
}

pub(crate) fn state_glyph(s: ModelState) -> (Icon, Role) {
    match s {
        ModelState::Ready => (Icon::CircleCheck, Role::Success),
        ModelState::Asleep => (Icon::Moon, Role::Text3),
        ModelState::Loading => (Icon::Hourglass, Role::Accent),
        ModelState::Missing => (Icon::CircleAlert, Role::Warning),
        ModelState::Off => (Icon::CircleMinus, Role::Warning),
        ModelState::Failed => (Icon::CircleX, Role::Danger),
        ModelState::Cloud(_) => (Icon::Cloud, Role::Accent),
        ModelState::NoAccount(_) => (Icon::CircleAlert, Role::Warning),
    }
}

/// Room above a row: messages apart, steps together.
fn gap(prev: Option<&Body>, this: &Body) -> f64 {
    // A card (a call that asked) and a picture stand apart; steps don't.
    let step = |b: &Body| match b {
        Body::Tool(t) => t.gate.is_none() && t.call.name != "screenshot",
        Body::Reasoning(_) | Body::Notice { .. } => true,
        _ => false,
    };
    match (prev, this) {
        (None, _) => 0.0,
        (_, Body::User { .. }) => 28.0,
        (Some(Body::User { .. }), _) => 18.0,
        (Some(p), t) if step(p) && step(t) => 4.0,
        _ => 14.0,
    }
}

fn transcript(app: &Tasuke, c: &Conversation) -> Element<Msg> {
    let items = &c.transcript.items;
    let rows = fold::rows(items);
    let answer = look(Role::Text, ANSWER);
    let mut children: Vec<Element<Msg>> = Vec::with_capacity(rows.len() + 1);
    let mut prev: Option<&Body> = None;
    for r in &rows {
        let (first, el, key) = match r {
            Row::Item(i) => {
                let item = &items[*i];
                (&item.body, item_view(app, c, *i, &answer), item.id)
            }
            Row::Looked(run) => {
                let first = &items[run.items[0]].body;
                (first, cards::looked(app, c, run), run.key)
            }
        };
        let g = gap(prev, first);
        prev = Some(match r {
            Row::Item(i) => &items[*i].body,
            Row::Looked(run) => &items[*run.items.last().expect("a run has items")].body,
        });
        children.push(container(el).padding([g, 0.0, 0.0, 0.0]).width(Fill).key(key).into());
    }
    if let Some(w) = working(app, c) {
        children.push(container(w).padding([12.0, 0.0, 0.0, 0.0]).key("working").into());
    }
    let id = c.id;
    let content = container(column_of(children).max_width(READING))
        .align_x(Align::Center)
        .padding([24.0, 32.0, 28.0, 32.0])
        .width(Fill);
    let scroller: Element<Msg> =
        follow_scroll(content).scroll_to_end(c.sent).restore(c.reading).on_read(move |r| Msg::Read(id, r)).into();
    scroller.key(("transcript", id.0))
}

/// While a turn runs with nothing showing it yet: what it's doing.
fn working(app: &Tasuke, c: &Conversation) -> Option<Element<Msg>> {
    c.turn.as_ref()?;
    if c.needs_you() {
        return None;
    }
    let last = c.transcript.items.last().map(|i| &i.body);
    let streaming = match last {
        Some(Body::Answer(a)) => a.open,
        Some(Body::Reasoning(r)) => r.ended.is_none(),
        Some(Body::Tool(t)) => t.state.is_open(),
        // A notice of something under way says it itself.
        Some(Body::Notice { text, .. }) => text.ends_with('…'),
        _ => false,
    };
    if streaming {
        return None;
    }
    let words = if app.chip.state == ModelState::Loading { "Waking the model…" } else { "Working…" };
    Some(
        row![icon(Icon::Hourglass).size(14.0).role(Role::Accent), text(words).size(12.5).role(Role::Text3)]
            .spacing(8.0)
            .into(),
    )
}

fn item_view(app: &Tasuke, c: &Conversation, index: usize, answer: &Look<Msg>) -> Element<Msg> {
    let items = &c.transcript.items;
    let item = &items[index];
    match &item.body {
        Body::User { text, .. } => {
            container(prose([(Span::new(text.clone()), None)]).font(FontSpec::ui(ANSWER)).line_height(1.35))
                .padding([10.0, 14.0])
                .fill(Tone::Chrome)
                .border(Edge::Line)
                .width(Fill)
                .into()
        }
        // Copy is offered on a turn's last answer, once the turn is over:
        // what the model says between steps is narration.
        Body::Answer(a)
            if a.open
                || c.turn.is_some() && index + 1 == items.len()
                || items.get(index + 1).is_some_and(|n| !matches!(n.body, Body::User { .. })) =>
        {
            md::doc(&a.doc, answer)
        }
        Body::Answer(a) => {
            let copy =
                icon_button(Icon::Copy).on_press(Msg::Copy(a.text.clone())).tooltip("Copy the answer as Markdown");
            column![md::doc(&a.doc, answer), row![spacer(), copy]].spacing(2.0).width(Fill).into()
        }
        Body::Reasoning(r) => cards::reasoning(c, item.id, r),
        Body::Tool(t) => cards::tool(app, c, item.id, t),
        Body::Notice { level, text } => {
            let now = match items.get(index + 1).map(|i| &i.body) {
                Some(Body::Error { .. }) => cards::NoticeNow::Failed,
                None if c.turn.is_some() => cards::NoticeNow::Going,
                _ => cards::NoticeNow::Over,
            };
            cards::notice(*level, text, now)
        }
        // Retry is offered on the last thing said, while nothing runs.
        Body::Error { text, retryable } => {
            cards::error(c.id, text, *retryable && index + 1 == items.len() && c.turn.is_none())
        }
    }
}

/// A mode's switch, its name and what it means now.
fn mode(
    on: bool,
    name: &'static str,
    on_words: &'static str,
    off_words: &'static str,
    f: impl Fn(bool) -> Msg + 'static,
) -> Element<Msg> {
    row![
        toggle(on).on_toggle(f),
        column![
            text(name).size(12.5).weight(600.0),
            text(if on { on_words } else { off_words }).size(11.0).role(Role::Text3)
        ]
        .spacing(1.0),
    ]
    .spacing(8.0)
    .into()
}

/// The model's chip: its name, where it runs, its state, its last speed.
fn chip(app: &Tasuke) -> Element<Msg> {
    let (glyph, role) = state_glyph(app.chip.state);
    let mut words = vec![
        Span::new(if app.chip.name.is_empty() { "No model".to_owned() } else { app.chip.name.clone() }).weight(600.0),
    ];
    let mut rest = if app.chip.state.is_cloud() {
        format!(" · {}", app.chip.state.words())
    } else {
        format!(" · on this computer · {}", app.chip.state.words())
    };
    if let (Some(tps), ModelState::Ready | ModelState::Asleep | ModelState::Cloud(_)) = (app.speed, app.chip.state) {
        rest.push_str(&format!(" · {tps:.0} tokens/s"));
    }
    words.push(Span::new(rest).role(Role::Text3));
    let chip =
        container(row![icon(glyph).size(13.0).role(role), rich_text(words).font(FontSpec::ui(12.0))].spacing(6.0))
            .padding([3.0, 8.0])
            .border(Edge::Line);
    mouse_area(chip)
        .on_press(Msg::Show(Page::Models))
        .cursor(aizome_kit::sidestep_ui::Cursor::Pointer)
        .tooltip("The model, in Models")
        .into()
}

fn composer_bar(app: &Tasuke, c: &Conversation) -> Element<Msg> {
    let id = c.id;
    let busy = c.turn.is_some();
    let has_text = !c.draft.text().trim().is_empty();
    let placeholder = if c.is_empty() { "Ask anything, or describe what's wrong…" } else { "Reply…" };
    let field = composer(&c.draft)
        .placeholder(placeholder)
        .font(FontSpec::ui(ANSWER))
        .lines(2, 10)
        .padding(14.0, 12.0)
        .on_action(move |a| Msg::Edit(id, a))
        .on_send((!busy && has_text).then_some(Msg::Send(id)))
        .accessible_label("Message");
    let action: Element<Msg> = if busy {
        button("Stop").icon(Icon::Square).on_press(Msg::Stop(id)).tooltip("Stop this turn (❖.)").into()
    } else {
        button("Send").icon(Icon::ArrowUp).primary().enabled(has_text).on_press(Msg::Send(id)).into()
    };
    let controls = row![
        mode(c.act, "Act", "changes things after you say Run", "only looks and explains", move |on| Msg::Act(id, on)),
        mode(c.desktop, "Desktop control", "can see and use the screen", "can't see or use the screen", move |on| {
            Msg::Desktop(id, on)
        }),
        spacer(),
        action,
    ]
    .spacing(22.0)
    .padding([2.0, 10.0, 10.0, 14.0]);
    let field: Element<Msg> = Element::from(field).id("composer");
    let boxed = container(column![field, controls].width(Fill)).fill(Tone::Chrome).border(Edge::Line).width(Fill);
    let hint = text("Return sends · ⇧Return starts a new line").size(11.0).role(Role::Faint);
    let under = row![chip(app), spacer(), hint].spacing(12.0);
    let _ = matches!(c.turn, Some(Turn::Script(_)));
    container(column![boxed, under].spacing(8.0).max_width(READING))
        .align_x(Align::Center)
        .padding([4.0, 32.0, 14.0, 32.0])
        .width(Fill)
        .into()
}
