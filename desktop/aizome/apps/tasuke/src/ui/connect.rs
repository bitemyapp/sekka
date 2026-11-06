//! The sheet that connects a Claude or OpenAI account: which company, what
//! using it means, where to make a key, the key pasted and checked; then
//! what the account can use.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};
use tasuke_models::CloudProvider;

use crate::app::{Connect, ConnectStep, Msg, Tasuke};

const WIDTH: f64 = 500.0;

/// "console.anthropic.com/settings/keys", as a link's words.
fn bare(url: &str) -> &str {
    url.trim_start_matches("https://")
}

/// A numbered step: its number in a circle, and what to do.
fn step(n: u32, body: Element<Msg>) -> Element<Msg> {
    row![
        container(text(n.to_string()).size(11.0).weight(700.0).role(Role::Text2))
            .padding([1.0, 6.0])
            .border(Edge::Line),
        body,
    ]
    .spacing(10.0)
    .align(Align::Start)
    .width(Fill)
    .into()
}

pub fn view(app: &Tasuke, c: &Connect) -> Element<Msg> {
    let p = c.provider;
    let company = p.company();
    let checking = c.step == ConnectStep::Checking;
    let picker = segmented(["Claude · Anthropic", "OpenAI"], CloudProvider::ALL.iter().position(|q| *q == p))
        .on_select(|i| Msg::ConnectProvider(CloudProvider::ALL[i.min(1)]))
        .enabled(!checking);
    let mut parts: Vec<Element<Msg>> =
        vec![text("Connect a Cloud Account").size(15.0).weight(700.0).into(), picker.into()];
    if let ConnectStep::Connected { models } = &c.step {
        let names = match models.as_slice() {
            [] => "its models".to_owned(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        parts.push(
            row![
                icon(Icon::CircleCheck).size(16.0).role(Role::Success),
                text(format!("Connected. The account can use {names}.")).size(13.0).wrap().line_height(1.4)
            ]
            .spacing(8.0)
            .align(Align::Start)
            .into(),
        );
        if let Some(r) = &c.then_use {
            parts.push(
                text(format!(
                    "Tasuke now uses {}. What you ask, and what Tasuke sees while it works, goes to {company}. \
                     Models has the others, and the models on this computer are still there to go back to.",
                    app.models.cloud_name(r)
                ))
                .size(12.5)
                .role(Role::Text2)
                .wrap()
                .line_height(1.4)
                .into(),
            );
        }
        parts.push(
            row![spacer(), button("Done").default().on_press(Msg::ConnectClose).id("connect-done")].width(Fill).into(),
        );
        return sheet(parts);
    }
    parts.push(
        text(format!(
            "Tasuke can use {} models through your own {company} account, which {company} bills by use. While one \
             is in use, what you ask and what Tasuke sees while it works, screenshots included, is sent to \
             {company}. Commands still run here, and still wait for your Run.",
            p.family()
        ))
        .size(12.5)
        .role(Role::Text2)
        .wrap()
        .line_height(1.45)
        .into(),
    );
    let keys = p.keys_page().to_owned();
    parts.push(step(
        1,
        column![
            text(format!("Make an API key in {company}'s console.")).size(13.0),
            button(bare(p.keys_page()).to_owned()).icon(Icon::ExternalLink).compact().on_press(Msg::OpenLink(keys)),
        ]
        .spacing(6.0)
        .width(Fill)
        .into(),
    ));
    let error = match &c.step {
        ConnectStep::Editing { error } => error.clone(),
        _ => None,
    };
    let field = text_field(&c.key)
        .password()
        .placeholder(format!("{}…", p.key_prefix()))
        .on_change(Msg::ConnectKey)
        .on_submit(Msg::ConnectCheck)
        .error(error)
        .enabled(!checking)
        .id("connect-key");
    parts.push(step(
        2,
        column![
            text("Paste it here.").size(13.0),
            field,
            text("It's checked with the company, then kept in this computer's keyring, never in Tasuke's files.")
                .size(11.5)
                .role(Role::Text3)
                .wrap()
                .line_height(1.4),
        ]
        .spacing(6.0)
        .width(Fill)
        .into(),
    ));
    let status: Element<Msg> = if checking {
        row![
            progress_indeterminate().width(80.0),
            text(format!("Checking with {company}…")).size(12.0).role(Role::Text2)
        ]
        .spacing(8.0)
        .align(Align::Center)
        .into()
    } else {
        space(0.0)
    };
    parts.push(
        row![
            status,
            spacer(),
            button("Cancel").cancel().on_press(Msg::ConnectClose),
            button("Connect")
                .default()
                .enabled(!checking && !c.key.trim().is_empty())
                .on_press(Msg::ConnectCheck)
                .id("connect-ok"),
        ]
        .spacing(8.0)
        .align(Align::Center)
        .width(Fill)
        .into(),
    );
    sheet(parts)
}

fn sheet(parts: Vec<Element<Msg>>) -> Element<Msg> {
    column_of(parts).spacing(14.0).padding(20.0).width(WIDTH).into()
}
