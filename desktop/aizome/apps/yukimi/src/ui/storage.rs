//! The store: how big it is, what keeps each part of it, what is heaviest,
//! and cleaning up what nothing needs any more.
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use yukimi_store::RootKind;
use yukimi_system::human_size;

use super::widgets::{Dye, bar, tap};
use super::{caption, group, group_digits, heading, mono, panel, swatch};
use crate::app::{Msg, Yukimi};
use crate::model::Composition;

/// The parts of the store: a title, what it is, its size, and its dye.
pub fn parts(c: &Composition) -> [(&'static str, &'static str, u64, Dye); 5] {
    [
        ("The running system", "Everything the system you are using needs.", c.system, Dye::Ai),
        ("Older generations", "Kept so you can return to them.", c.old_generations, Dye::Hanada),
        ("Packages just for users", "Installed into people's own profiles.", c.profiles, Dye::Violet),
        ("Projects and dev shells", "Build results and development environments.", c.projects, Dye::Green),
        ("Garbage", "Needed by nothing. Cleaning up removes it.", c.garbage, Dye::Faint),
    ]
}

/// The bar's segments: each part's share of the store.
pub fn segments(c: &Composition) -> Vec<(f64, Dye)> {
    let total = c.total.max(1) as f64;
    parts(c).iter().map(|(_, _, size, dye)| (*size as f64 / total, *dye)).collect()
}

fn root_kind(kind: &RootKind) -> String {
    match kind {
        RootKind::CurrentSystem => "The running system".to_owned(),
        RootKind::BootedSystem => "The system this computer started with".to_owned(),
        RootKind::SystemGeneration(n) => format!("System generation {n}"),
        RootKind::Profile { name, user, generation } => {
            let whose = user.as_deref().map(|u| format!("{u}'s ")).unwrap_or_default();
            match generation {
                Some(g) => format!("{whose}{name} profile, generation {g}"),
                None => format!("{whose}{name} profile"),
            }
        }
        RootKind::BuildResult => "A build result".to_owned(),
        RootKind::DevShell => "A development shell".to_owned(),
        RootKind::Other => "Other".to_owned(),
    }
}

/// How many roots show at once when opened; there can be thousands.
const ROOTS_SHOWN: usize = 200;

pub fn page(app: &Yukimi) -> Element<Msg> {
    let intro = heading(
        "The Nix store",
        "Everything installed lives in /nix/store, each version in its own folder, which is why several can live \
         side by side and why going back is instant. Here is what keeps each part of it.",
    );
    let Some(store) = app.model.store.clone() else {
        return column![intro, super::list(vec![super::pending("Reading the store…")])]
            .spacing(18.0)
            .width(Fill)
            .into();
    };
    let c = &store.composition;
    let total = c.total.max(1) as f64;

    let legend: Vec<Element<Msg>> = parts(c)
        .into_iter()
        .map(|(title, explanation, size, dye)| {
            super::item_row(
                Some(swatch(dye)),
                text(title).weight(600.0).into(),
                Some(text(explanation).size(12.0).role(Role::Text3).into()),
                vec![
                    mono(format!("{:.0}%", size as f64 / total * 100.0)).role(Role::Text3).width(44.0).into(),
                    mono(human_size(size)).width(72.0).into(),
                ],
            )
        })
        .collect();
    let whole = column![
        row![
            text(human_size(c.total)).mono().tabular().size(30.0).weight(600.0),
            column![
                text(format!("in {} store paths", group_digits(c.paths))).role(Role::Text2),
                text(format!("{} of them garbage", group_digits(c.garbage_paths))).size(12.0).role(Role::Text3),
            ]
            .spacing(1.0),
        ]
        .spacing(14.0)
        .align(Align::Center),
        bar(segments(c), 22.0).width(Fill),
    ]
    .spacing(12.0)
    .width(Fill);

    // Cleaning up.
    let choices = ["a week", "a month", "three months", "keep them all"];
    let clean = column![
        row![
            text("Delete system generations older than").width(Fill),
            dropdown(choices, app.older).on_select(Msg::Older).width(170.0),
        ]
        .spacing(12.0)
        .width(Fill),
        divider().edge(Edge::Line),
        row![
            column![
                text("Clean up now").weight(600.0),
                text(format!("Frees at least {} now, and more as old generations go.", human_size(c.garbage)))
                    .size(12.0)
                    .role(Role::Text3),
            ]
            .spacing(2.0)
            .width(Fill),
            button("Clean Up…").destructive().icon(Icon::Trash).on_press(Msg::CleanUp),
        ]
        .spacing(12.0)
        .width(Fill),
    ]
    .spacing(12.0)
    .width(Fill);
    let clean = column![
        caption("Clean up"),
        text(
            "Deleting old generations lets their packages go; collecting garbage removes everything nothing needs. \
             The running system and the one this computer started with are always kept."
        )
        .wrap()
        .size(12.0)
        .role(Role::Text3),
        panel(clean).width(Fill),
    ]
    .spacing(6.0)
    .width(Fill);

    // The heaviest packages, each with its share of the heaviest.
    let largest = store.heaviest.first().map(|h| h.size).unwrap_or(1).max(1) as f64;
    let heavy: Vec<Element<Msg>> = store
        .heaviest
        .iter()
        .map(|p| {
            row![
                column![
                    text(p.name.clone()).weight(600.0),
                    text(p.version.clone()).mono().size(11.0).role(Role::Text3),
                ]
                .spacing(1.0)
                .width(Length::FillPortion(2)),
                bar(vec![(p.size as f64 / largest, Dye::Hanada)], 8.0).width(Length::FillPortion(3)),
                mono(human_size(p.size)).width(72.0),
            ]
            .spacing(14.0)
            .padding([7.0, 12.0])
            .width(Fill)
            .into()
        })
        .collect();

    // What keeps things alive.
    let mut roots: Vec<Element<Msg>> = vec![
        tap(row![
            icon(if app.roots_open { Icon::ChevronDown } else { Icon::ChevronRight }).size(14.0).role(Role::Text3),
            text(format!("{} roots", group_digits(store.roots.len()))).weight(600.0),
            spacer(),
            text(if app.roots_open { "Hide" } else { "Show" }).size(12.0).role(Role::Text3),
        ]
        .spacing(8.0)
        .width(Fill)
        .height(24.0))
        .padding([8.0, 12.0])
        .on_press(Msg::ToggleRoots)
        .named("Garbage-collector roots")
        .into(),
    ];
    if app.roots_open {
        for root in store.roots.iter().take(ROOTS_SHOWN) {
            roots.push(super::item_row(
                None,
                text(root_kind(&root.kind)).into(),
                Some(text(root.link.display().to_string()).mono().size(11.0).role(Role::Text3).into()),
                vec![
                    text(root.target.full_name().to_owned())
                        .mono()
                        .size(11.0)
                        .role(Role::Text3)
                        .max_width(260.0)
                        .into(),
                ],
            ));
        }
        if store.roots.len() > ROOTS_SHOWN {
            roots.push(super::empty(&format!("And {} more.", store.roots.len() - ROOTS_SHOWN)));
        }
    }

    column![
        intro,
        whole,
        super::list(legend),
        clean,
        group("Heaviest in the running system", None, None, heavy),
        group(
            "What keeps things in the store",
            Some("Garbage-collector roots: links to store paths that keep them and all they need.".to_owned()),
            None,
            roots,
        ),
    ]
    .spacing(18.0)
    .width(Fill)
    .into()
}
