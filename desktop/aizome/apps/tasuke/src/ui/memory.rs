//! Memory: the notes Tasuke keeps between conversations, when each was
//! made, and a way to remove any. They're context, not instructions.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};

use crate::app::{Msg, Tasuke};

pub fn view(app: &Tasuke) -> Element<Msg> {
    let clock = app.clock();
    let header = column![
        text("Memory").size(22.0).weight(700.0),
        text(
            "Notes Tasuke keeps between conversations, so you needn't say things twice. \
             It reads them as background, not as instructions to follow."
        )
        .size(13.0)
        .role(Role::Text2)
        .wrap(),
    ]
    .spacing(4.0);
    let body: Element<Msg> = if app.notes.is_empty() {
        container(
            column![
                icon(Icon::NotebookPen).size(22.0).role(Role::Text3),
                text("Nothing remembered yet.").size(14.0).weight(600.0),
                text(
                    "When you tell Tasuke something worth keeping, such as which printer is yours, \
                     it notes it here, and you can remove it whenever you like."
                )
                .size(12.5)
                .role(Role::Text3)
                .wrap()
                .center(),
            ]
            .spacing(8.0)
            .align(Align::Center)
            .max_width(420.0),
        )
        .center()
        .padding([40.0, 0.0])
        .width(Fill)
        .into()
    } else {
        let mut notes: Vec<&crate::app::Note> = app.notes.iter().collect();
        notes.sort_by(|a, b| b.created.cmp(&a.created));
        let rows: Vec<Element<Msg>> = notes
            .iter()
            .enumerate()
            .flat_map(|(i, n)| {
                let line: Element<Msg> = row![
                    column![
                        text(n.text.clone()).size(13.5).wrap(),
                        text(format!("Noted {}", clock.when(n.created))).size(11.5).role(Role::Text3),
                    ]
                    .spacing(3.0)
                    .width(Fill),
                    button("Remove").on_press(Msg::Forget(n.id.clone())),
                ]
                .spacing(12.0)
                .padding([10.0, 14.0])
                .width(Fill)
                .key(n.id.clone())
                .into();
                let rule: Option<Element<Msg>> = (i > 0).then(|| divider().edge(Edge::Line).into());
                rule.into_iter().chain(std::iter::once(line))
            })
            .collect();
        let count = match app.notes.len() {
            1 => "1 note".to_owned(),
            n => format!("{n} notes"),
        };
        column![
            aizome_kit::caption(count),
            container(column_of(rows).width(Fill)).fill(Tone::Chrome).border(Edge::Line).width(Fill),
            row![
                icon(Icon::Info).size(13.0).role(Role::Text3),
                text("Tasuke adds a note when you ask it to remember something, and says so in the conversation.")
                    .size(12.0)
                    .role(Role::Text3)
                    .wrap(),
            ]
            .spacing(8.0)
            .padding([8.0, 2.0, 0.0, 2.0]),
        ]
        .width(Fill)
        .into()
    };
    scroll(
        container(column![header, body].spacing(22.0).max_width(760.0))
            .align_x(Align::Center)
            .padding([24.0, 32.0, 32.0, 32.0])
            .width(Fill),
    )
    .key("memory")
    .into()
}
