//! The window: a sidebar (New Conversation, the conversations by recency,
//! Models, Memory) beside the page it shows; first run on its own.

mod bar;
mod cards;
mod conversation;
mod first_run;
mod memory;
mod models;

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};

use crate::app::{ConvId, Msg, Page, Tasuke};

/// The widest a reading column gets.
pub(crate) const READING: f64 = 720.0;

pub fn view(app: &Tasuke) -> Element<Msg> {
    if let Some(step) = &app.first_run {
        return first_run::view(app, step);
    }
    let page: Element<Msg> = match app.page {
        Page::Conversation(id) => match app.conversation(id) {
            Some(c) => conversation::view(app, c),
            None => space(0.0),
        },
        Page::Models => models::view(app),
        Page::Memory => memory::view(app),
    };
    sidebar_layout(sidebar(app), container(page).fill(Tone::Surface).width(Fill).height(Fill)).first_size(248.0).into()
}

fn brand() -> Element<Msg> {
    row![
        container(icon(Icon::MessageSquare).size(18.0).role(Role::Accent))
            .padding(5.0)
            .fill(Tone::Chrome)
            .border(Edge::Line),
        column![
            text("Tasuke").size(15.0).weight(700.0),
            text("助 Assistant · on this computer").size(11.0).role(Role::Text3),
        ]
        .spacing(1.0),
    ]
    .spacing(10.0)
    .padding([14.0, 12.0, 12.0, 12.0])
    .into()
}

fn sidebar(app: &Tasuke) -> Element<Msg> {
    let current = match app.page {
        Page::Conversation(id) => Some(id),
        _ => None,
    };
    let on_new = current.and_then(|id| app.conversation(id)).is_some_and(|c| c.is_empty());
    let new = sidebar_item(Icon::SquarePen, "New Conversation", on_new).on_press(Msg::NewConversation);
    let clock = app.clock();
    let recent = app.recent();
    let items: Vec<Element<Msg>> = recent
        .iter()
        .map(|c| {
            let detail = if c.needs_you() {
                "needs you".to_owned()
            } else if c.turn.is_some() {
                "working".to_owned()
            } else {
                clock.brief(c.updated)
            };
            let selected = current == Some(c.id);
            conversation_item(c.id, &c.title, &detail, selected, c.needs_you())
        })
        .collect();
    let list: Element<Msg> = if items.is_empty() {
        let words =
            if app.loading { "Reading your conversations…" } else { "Your conversations will be listed here." };
        container(text(words).size(12.0).role(Role::Text3).wrap().line_height(1.4)).padding([4.0, 12.0]).into()
    } else {
        scroll(column_of(items).spacing(1.0).padding([0.0, 6.0]).width(Fill)).height(Fill).into()
    };
    let models_detail = if app.models.download.is_some() {
        "downloading".to_owned()
    } else if app.chip.name.is_empty() {
        "choose one".to_owned()
    } else {
        String::new()
    };
    let mut models = sidebar_item(Icon::Cpu, "Models", app.page == Page::Models).on_press(Msg::Show(Page::Models));
    if !models_detail.is_empty() {
        models = models.detail(models_detail);
    }
    let memory = sidebar_item(Icon::NotebookPen, "Memory", app.page == Page::Memory)
        .on_press(Msg::Show(Page::Memory))
        .detail(app.notes.len().to_string());
    column![
        brand(),
        divider().edge(Edge::Line),
        container(new).padding([8.0, 6.0, 4.0, 6.0]).width(Fill),
        aizome_kit::caption("Conversations"),
        list,
        divider().edge(Edge::Line),
        container(column![models, memory].spacing(1.0).width(Fill)).padding([6.0, 6.0, 8.0, 6.0]).width(Fill),
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

/// A conversation in the list: its title, and when, or that it waits.
fn conversation_item(id: ConvId, title: &str, detail: &str, selected: bool, waits: bool) -> Element<Msg> {
    let item = sidebar_item(None, title.to_owned(), selected)
        .detail(detail.to_owned())
        .on_press(Msg::Show(Page::Conversation(id)));
    let el: Element<Msg> = if waits { tooltip(item, "A command is waiting for your answer") } else { item.into() };
    el.key(id.0)
}

/// How far an icon beside wrapped text comes down to sit on its first
/// line (text set at a reading line height has room above it).
const LEAD: f64 = 4.0;

/// An icon (or a small element) put level with the first line of wrapped
/// text beside it, in a row aligned to the top.
pub(crate) fn lead(e: impl Into<Element<Msg>>) -> Element<Msg> {
    container(e).padding([LEAD, 0.0, 0.0, 0.0]).into()
}
