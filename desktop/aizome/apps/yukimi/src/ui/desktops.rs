//! The system's own choices, as its distribution declares them
//! (`programs.yukimi.choices`): on Sekka, which desktops it has, which one
//! starts by default, and the login screen. Picking changes nothing until
//! Apply, which asks for an administrator password and builds the system
//! once with every choice made.
use aizome_kit::column;
use aizome_kit::prelude::*;
use yukimi_system::completion::Action;
use yukimi_system::setup::Choice;

use super::{banner, group, heading, item_row, tag};
use crate::app::{Msg, Yukimi, option_title, words};

/// The toolbar's buttons: put the choices back, and apply them.
pub fn tools(app: &Yukimi) -> Vec<Element<Msg>> {
    if app.choice_changes().is_empty() {
        return Vec::new();
    }
    let ready = app.choice_problems().is_empty() && app.model.cannot_change_system().is_none();
    vec![
        button("Revert")
            .toolbar()
            .icon(Icon::Undo2)
            .on_press(Msg::RevertChoices)
            .tooltip("Put the choices back as the configuration has them")
            .into(),
        button("Apply…")
            .primary()
            .icon(Icon::Check)
            .on_press_maybe(ready.then_some(Msg::ApplyChoices))
            .tooltip("Build the system with these choices; this asks for an administrator password")
            .into(),
    ]
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let mut c = column![heading(
        "Desktops and login",
        "Which desktops this computer has, which one starts when you sign in, and the screen you sign in at. \
         Choose here, then Apply: Yukimi builds your system with your choices, which asks for an administrator \
         password. The version you have now stays in History.",
    )]
    .spacing(18.0)
    .width(Fill);
    if app.choices().is_empty() {
        return c
            .push(super::list(vec![super::empty(
                "This system doesn't say which of its settings Yukimi may change. Sekka's systems do \
                 (programs.yukimi.choices).",
            )]))
            .into();
    }
    if !app.loading && app.model.completion.action != Action::None {
        c = c.push(super::updates::completion(&app.model.completion));
    }
    if let Some(why) = app.model.cannot_change_system() {
        c = c.push(banner(Icon::TriangleAlert, Role::Warning, why));
    }
    let changes = app.choice_changes();
    for problem in app.choice_problems() {
        c = c.push(banner(Icon::TriangleAlert, Role::Warning, format!("{problem}.")));
    }
    if !changes.is_empty() && app.choice_problems().is_empty() {
        let names: Vec<String> = changes
            .iter()
            .filter_map(|(s, _)| app.choices().iter().find(|c| &c.setting == s))
            .map(|c| c.title.to_lowercase())
            .collect();
        c = c.push(banner(
            Icon::Info,
            Role::Accent,
            format!("You changed the {}. Apply builds the system with them; Revert puts them back.", words(&names)),
        ));
    }
    for choice in app.choices() {
        c = c.push(choice_group(app, choice));
    }
    c.into()
}

/// One choice: a checkbox for each option of a list, a radio button for
/// each of a single value.
fn choice_group(app: &Yukimi, choice: &Choice) -> Element<Msg> {
    let chosen = app.chosen(&choice.setting);
    let configured = app.model.configured.choices.get(&choice.setting).cloned().unwrap_or_default();
    let running = choice.value.values();
    // A default desktop is one of the desktops chosen.
    let within = choice.within.as_ref().map(|w| app.chosen(w));
    let selected = chosen.first().and_then(|v| choice.options.iter().position(|o| &o.id == v));
    let mut rows = Vec::new();
    for (i, option) in choice.options.iter().enumerate() {
        let on = chosen.contains(&option.id);
        if within.as_ref().is_some_and(|w| !w.contains(&option.id)) && !on {
            continue;
        }
        // What it needs, when that isn't chosen.
        let missing = option.needs().filter(|(setting, value)| !app.chosen(setting).iter().any(|v| v == value));
        let (setting, id) = (choice.setting.clone(), option.id.clone());
        let control: Element<Msg> = if choice.many {
            checkbox(option.title.clone(), on)
                .on_toggle(move |on| Msg::Pick { setting: setting.clone(), id: id.clone(), on })
                .id(format!("{}={}", choice.setting, option.id))
                .into()
        } else {
            radio(option.title.clone(), i, selected)
                .on_select(move |_| Msg::Pick { setting: setting.clone(), id: id.clone(), on: true })
                .enabled(missing.is_none() || on)
                .id(format!("{}={}", choice.setting, option.id))
                .into()
        };
        let mut lines: Vec<String> = option.detail.iter().cloned().collect();
        if let Some((setting, value)) = missing {
            let among = app.choices().iter().find(|c| c.setting == setting).map_or(setting, |c| c.title.as_str());
            lines.push(format!(
                "Needs {} among the {}.",
                option_title(app.choices(), setting, value),
                among.to_lowercase()
            ));
        }
        let subtitle = (!lines.is_empty()).then(|| {
            container(text(lines.join(" ")).wrap().size(12.0).role(Role::Text3))
                .padding([0.0, 0.0, 0.0, 26.0])
                .width(Fill)
                .into()
        });
        let mut trailing = Vec::new();
        let was = configured.contains(&option.id);
        match (choice.many, on, was) {
            (true, true, false) => trailing.push(tag("adding", Role::Accent)),
            (true, false, true) => trailing.push(tag("removing", Role::Warning)),
            (false, true, false) => trailing.push(tag("chosen", Role::Accent)),
            // Built, but the running system started before it.
            (_, true, true) if !running.contains(&option.id) => trailing.push(tag("after restart", Role::Warning)),
            (false, _, true) => trailing.push(tag("current", Role::Text3)),
            _ => {}
        }
        rows.push(item_row(None, control, subtitle, trailing));
    }
    let description = match (choice.many, &choice.within) {
        (true, _) => Some(format!(
            "Each is listed on the login screen. {} chosen.",
            match chosen.len() {
                1 => "One is".to_owned(),
                n => format!("{n} are"),
            }
        )),
        (false, Some(_)) => Some("One of the desktops chosen above.".to_owned()),
        (false, None) => None,
    };
    let trailing = (chosen != configured).then(|| {
        button("Undo")
            .plain()
            .compact()
            .icon(Icon::Undo2)
            .on_press(Msg::UndoChoice(choice.setting.clone()))
            .tooltip("Put this one back as the configuration has it")
            .into()
    });
    group(&choice.title, description, trailing, rows)
}
