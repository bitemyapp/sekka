// SPDX-License-Identifier: GPL-3.0-or-later
//! Public SSH keys from GitHub, the SSH server, and Git's identity.
use super::{FIELD, group, note_row, sections};
use crate::app::{Field, Genkan, Msg};
use aizome_kit::prelude::*;
use aizome_kit::row;

pub fn page(app: &Genkan) -> Element<Msg> {
    let error = app
        .github_error()
        .or_else(|| (app.flagged == Some(Field::GithubUser)).then(String::new));
    let lookup = row![
        text_field(&app.github_user)
            .placeholder("Username")
            .on_change(Msg::GithubUser)
            .on_submit(Msg::Lookup)
            .error(error.filter(|e| !e.is_empty()))
            .id(Field::GithubUser.id())
            .width(FIELD - 96.0),
        button("Look Up")
            .on_press(Msg::Lookup)
            .enabled(app.can_look_up())
            .width(88.0),
    ]
    .spacing(8.0)
    .align(Align::Start);
    let mut status = row![].spacing(10.0).padding([9.0, 12.0]).width(Fill);
    if app.github_pending.is_some() {
        status = status.push(progress_indeterminate().width(48.0));
    }
    status = status.push(
        text(app.github_status.clone())
            .size(12.0)
            .role(Role::Text3)
            .wrap()
            .width(Fill)
            .id("github-status"),
    );
    let mut items = vec![group(
        "GitHub",
        "Bring your public SSH keys from GitHub, as Ubuntu's server installer does, so you can log in to this computer with them.",
        vec![form_row("GitHub username", lookup).into(), status.into()],
    )];
    let keys = app.github_keys();
    if !keys.is_empty() {
        let mut rows: Vec<Element<Msg>> = vec![
            form_row(
                "Allow these keys to log in",
                toggle(app.authorize).on_toggle(Msg::Authorize),
            )
            .description("Adds them to your account's authorized keys")
            .into(),
        ];
        for (index, key) in keys.iter().enumerate() {
            rows.push(
                form_row(key.kind().to_string(), space(0.0))
                    .description(key.fingerprint())
                    .icon(Icon::KeyRound)
                    .key(index)
                    .into(),
            );
        }
        items.push(group(
            "SSH Keys",
            "Fetched from github.com. Only the public halves of your keys are shared.",
            rows,
        ));
    }
    items.push(group(
        "Remote Login",
        "",
        vec![
            form_row(
                "Install and enable the SSH server",
                toggle(app.ssh_server).on_toggle(Msg::SshServer),
            )
            .description(app.ssh_server_description())
            .into(),
        ],
    ));
    items.push(group(
        "Git",
        "The name and email address recorded in your commits. Leave both empty to set them later.",
        vec![
            form_row(
                "Name",
                text_field(&app.git_name)
                    .on_change(Msg::GitName)
                    .error(app.git_name_error())
                    .id(Field::GitName.id())
                    .width(FIELD),
            )
            .into(),
            form_row(
                "Email",
                text_field(&app.git_email)
                    .on_change(Msg::GitEmail)
                    .error(app.git_email_error())
                    .id(Field::GitEmail.id())
                    .width(FIELD),
            )
            .into(),
            note_row("Saved in /etc/gitconfig for every account on this computer; git config --global sets your own."),
        ],
    ));
    sections(items)
}
