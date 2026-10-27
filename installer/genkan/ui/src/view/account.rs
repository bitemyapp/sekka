// SPDX-License-Identifier: GPL-3.0-or-later
//! The account and the computer's name.
use super::{FIELD, group, note_row, sections};
use crate::app::{Field, Genkan, Msg};
use aizome_kit::prelude::*;

fn field(
    app: &Genkan,
    label: &str,
    which: Field,
    value: &str,
    on_change: fn(String) -> Msg,
    password: bool,
) -> Element<Msg> {
    let mut input = text_field(value)
        .on_change(on_change)
        .error(app.account_error(which))
        .id(which.id())
        .width(FIELD);
    if password {
        input = input.password();
    }
    form_row(label.to_string(), input).into()
}

pub fn page(app: &Genkan) -> Element<Msg> {
    sections(vec![
        group(
            "About You",
            "Root login is locked. Your account administers the system with sudo.",
            vec![
                field(
                    app,
                    "Full name",
                    Field::FullName,
                    &app.full_name,
                    Msg::FullName,
                    false,
                ),
                field(
                    app,
                    "Username",
                    Field::Username,
                    &app.username,
                    Msg::Username,
                    false,
                ),
                field(
                    app,
                    "Password",
                    Field::Password,
                    &app.password,
                    Msg::Password,
                    true,
                ),
                field(
                    app,
                    "Confirm password",
                    Field::Repeat,
                    &app.repeat,
                    Msg::Repeat,
                    true,
                ),
                note_row(app.password_hint()),
            ],
        ),
        group(
            "This Computer",
            "The name shown on your network: letters, numbers and inner hyphens.",
            vec![field(
                app,
                "Computer name",
                Field::Hostname,
                &app.hostname,
                Msg::Hostname,
                false,
            )],
        ),
    ])
}
