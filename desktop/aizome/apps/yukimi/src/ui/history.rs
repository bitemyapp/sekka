//! Every version of the system that can be returned to, newest first, each
//! with what changed from the one before it, worked out (away from the
//! interface thread) when it is opened.
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use yukimi_system::diff::{Change, ChangeKind};
use yukimi_system::{human_age, human_size};

use super::widgets::tap;
use super::{heading, mono, tag};
use crate::app::{Changes, Msg, Yukimi};

/// How many changes show for one generation.
const SHOWN: usize = 200;

fn change_row(change: &Change) -> Element<Msg> {
    let (icon_name, role, versions) = match change.kind {
        ChangeKind::Upgraded => {
            (Icon::ArrowUp, Role::Success, format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
        ChangeKind::Downgraded => {
            (Icon::ArrowDown, Role::Warning, format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
        ChangeKind::Added => (Icon::Plus, Role::Accent, change.after.join(", ")),
        ChangeKind::Removed => (Icon::Minus, Role::Danger, change.before.join(", ")),
        ChangeKind::Changed => {
            (Icon::RefreshCw, Role::Text3, format!("{} → {}", change.before.join(", "), change.after.join(", ")))
        }
    };
    let mut r = row![
        icon(icon_name).size(14.0).role(role),
        text(change.name.clone()).weight(600.0).width(Length::FillPortion(2)),
        text(versions).mono().size(12.0).role(Role::Text2).width(Length::FillPortion(3)),
    ]
    .spacing(10.0)
    .padding([5.0, 12.0, 5.0, 40.0])
    .width(Fill);
    let delta = if change.size_delta == 0 {
        String::new()
    } else {
        let sign = if change.size_delta > 0 { "+" } else { "−" };
        format!("{sign}{}", human_size(change.size_delta.unsigned_abs()))
    };
    r = r.push(mono(delta).role(Role::Text3).width(80.0));
    r.into()
}

/// What shows under an opened generation.
fn opened(app: &Yukimi, number: u32, current: bool) -> Vec<Element<Msg>> {
    let mut rows = Vec::new();
    let Some(changes) = app.changes.get(&number) else {
        return rows;
    };
    let Changes::Ready { kept, diff, store_read, has_previous } = changes else {
        rows.push(super::pending("Working out what changed…"));
        return rows;
    };
    if !current {
        rows.push(
            row![
                column![
                    text("Return to this version").weight(600.0),
                    text(if *kept {
                        "The running system and your choices go back to this generation. The newer ones stay too."
                    } else {
                        "The running system goes back to this generation. The newer ones stay too."
                    })
                    .size(12.0)
                    .role(Role::Text3)
                    .wrap(),
                ]
                .spacing(2.0)
                .width(Fill),
                button("Return…").icon(Icon::Undo2).on_press(Msg::ReturnTo(number)),
            ]
            .spacing(12.0)
            .padding([8.0, 12.0, 8.0, 40.0])
            .width(Fill)
            .into(),
        );
    }
    if !has_previous {
        rows.push(super::empty("The oldest generation kept."));
        return rows;
    }
    let Some(diff) = diff else {
        rows.push(super::empty(if *store_read {
            "What changed couldn't be worked out."
        } else {
            "The store is still being read; open this again in a moment."
        }));
        return rows;
    };
    if diff.changes.is_empty() {
        rows.push(super::empty("The same packages as the generation before; only settings changed."));
    }
    if diff.changes.len() > SHOWN {
        rows.push(super::empty(&format!("The first {SHOWN} of {} changes.", diff.changes.len())));
    }
    rows.extend(diff.changes.iter().take(SHOWN).map(change_row));
    rows
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let intro = heading(
        "Every version of your system",
        "Each time the system changes, NixOS keeps the version before as a generation. Any of them can be started \
         from the boot menu, or returned to here. Nothing is lost until old generations are cleaned up in Storage.",
    );
    let gens = &model.generations;
    let mut rows: Vec<Element<Msg>> = Vec::new();
    if gens.is_empty() {
        rows.push(super::empty(if app.loading { "Looking around…" } else { "No generations found." }));
    }
    let sizes = model.store.as_ref().map(|s| &s.generation_sizes);
    for generation in gens.iter().rev() {
        let open = app.opened.contains(&generation.number);
        let mut facts = vec![human_age(generation.created, app.now())];
        if let Some(version) = &generation.nixos_version {
            facts.push(format!("NixOS {version}"));
        }
        if let Some(kernel) = &generation.kernel_version {
            facts.push(format!("Linux {kernel}"));
        }
        let mut head = row![
            icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }).size(14.0).role(Role::Text3),
            column![
                text(format!("Generation {}", generation.number)).weight(650.0),
                text(facts.join("  ·  ")).mono().size(11.5).role(Role::Text3),
            ]
            .spacing(2.0)
            .width(Fill),
        ]
        .spacing(12.0)
        .width(Fill);
        if generation.active {
            head = head.push(tag("running", Role::Success));
        }
        if generation.current && !generation.active {
            head = head.push(tag("selected for boot", Role::Accent));
        }
        if generation.booted && !generation.active {
            head = head.push(tag("started with", Role::Violet));
        }
        if let Some(size) = sizes.and_then(|s| s.iter().find(|(n, _)| *n == generation.number)) {
            head = head.push(mono(human_size(size.1)).role(Role::Text3).width(64.0));
        }
        let label = format!("Generation {}", generation.number);
        rows.push(
            tap(head)
                .padding([8.0, 12.0])
                .on_press(Msg::ToggleGeneration(generation.number))
                .named(label)
                .pipe_key(generation.number),
        );
        if open {
            rows.extend(opened(app, generation.number, generation.active));
        }
    }
    column![intro, super::list(rows)].spacing(18.0).width(Fill).into()
}

/// Rows keep their place by their generation's number.
trait PipeKey: Sized {
    fn pipe_key(self, key: u32) -> Element<Msg>;
}

impl PipeKey for super::widgets::Tap<Msg> {
    fn pipe_key(self, key: u32) -> Element<Msg> {
        Element::from(self).key(key)
    }
}
