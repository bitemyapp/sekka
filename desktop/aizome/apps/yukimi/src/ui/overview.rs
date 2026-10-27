//! The first page: this computer at a glance, as a dashboard. Under a
//! night sky of falling snow, its name and version; then how many
//! packages it has, how fresh it is, how much room it takes and how many
//! versions of it can be gone back to; what fills the store and how each
//! generation grew; what the latest change brought; and where it comes
//! from.
use aizome_kit::prelude::*;
use aizome_kit::{column, row, stack};
use yukimi_config::lock::Movement;
use yukimi_system::completion::Action;
use yukimi_system::diff::ChangeKind;
use yukimi_system::setup::Kind;
use yukimi_system::{human_age, human_size};

use super::widgets::{self, columns, grid, monogram, snowfall, tap};
use super::{Pipe, banner, caption, link, mono, package_counts, panel, tag};
use crate::app::{Msg, Page, Yukimi};
use crate::model::Model;

/// Ai 藍, the sky the snow falls through, and the mark's colours, which
/// don't change with the theme.
const SKY: u32 = 0x2b3f78;
const SKY_HIGH: u32 = 0x24346a;
const HANADA: u32 = 0x7aa2f7;
const KAMENOZOKI: u32 = 0xa9c4ff;
const KOHAKU: u32 = 0xe0af68;
const SNOW: u32 = 0xeef1ff;

fn greeting(hour: u32) -> &'static str {
    match hour {
        5..12 => "Good morning",
        12..18 => "Good afternoon",
        18..23 => "Good evening",
        _ => "Still up?",
    }
}

/// "3 days ago" without the "ago", for a big number.
fn age(then: i64, now: i64) -> String {
    let text = human_age(then, now);
    text.strip_suffix(" ago").map(str::to_owned).unwrap_or(text)
}

/// The night sky with the computer's name in it.
fn hero(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let name = if model.info.hostname.is_empty() { "This computer" } else { &model.info.hostname };
    let mut facts = Vec::new();
    if !model.info.nixos_version.is_empty() {
        facts.push(format!("NixOS {}", model.info.release()));
    }
    if let Some(generation) = model.current_generation() {
        facts.push(format!("generation {}, built {}", generation.number, human_age(generation.created, app.now())));
    }
    if !model.info.kernel.is_empty() {
        facts.push(format!("Linux {}", model.info.kernel));
    }
    let facts = if app.loading && facts.is_empty() { "Looking around…".to_owned() } else { facts.join("  ·  ") };
    let tab =
        container(text(greeting(app.clock.hour())).size(12.0).weight(650.0).condensed().color(Color::hex(0x1a1b26)))
            .padding([3.0, 10.0])
            .fill(Tone::Color(Color::hex(KOHAKU)));
    let mut words = column![
        tab,
        text(name.to_owned()).size(34.0).weight(750.0).color(Color::hex(SNOW)),
        text(facts).mono().size(12.0).color(Color::hex(KAMENOZOKI)),
    ]
    .spacing(8.0)
    .padding([0.0, 0.0, 0.0, 0.0]);
    if model.completion.action != Action::None {
        let note = row![
            icon(Icon::TriangleAlert).size(14.0).color(Color::hex(0x1a1b26)),
            text(model.completion.title()).weight(600.0).color(Color::hex(0x1a1b26)),
            icon(Icon::ChevronRight).size(14.0).color(Color::hex(0x1a1b26)),
        ]
        .spacing(6.0);
        words = words.push(
            tap(container(note).padding([4.0, 10.0]).fill(Tone::Color(Color::hex(KOHAKU))))
                .on_press(Msg::Show(Page::Updates))
                .named(model.completion.title()),
        );
    }
    let seal = container(
        column![
            text("雪").size(20.0).color(Color::hex(KAMENOZOKI)),
            text("見").size(20.0).color(Color::hex(KAMENOZOKI))
        ]
        .spacing(0.0)
        .align(Align::Center),
    )
    .padding([6.0, 8.0])
    .border(Edge::Color(Color::hex(KAMENOZOKI).with_alpha(0.55)));
    let sky = column![
        container(spacer()).fill(Tone::Color(Color::hex(SKY_HIGH))).width(Fill).height(Length::FillPortion(1)),
        container(spacer()).fill(Tone::Color(Color::hex(SKY))).width(Fill).height(Length::FillPortion(2)),
        container(spacer()).fill(Tone::Color(Color::hex(HANADA))).width(Fill).height(4.0),
        container(spacer()).fill(Tone::Color(Color::hex(KAMENOZOKI))).width(Fill).height(4.0),
        container(spacer()).fill(Tone::Color(Color::hex(SNOW))).width(Fill).height(6.0),
    ]
    .width(Fill)
    .height(Fill);
    let content = row![words.width(Fill), seal].align(Align::Start).padding([20.0, 24.0, 30.0, 24.0]).width(Fill);
    stack![sky, snowfall(56, true).width(Fill).height(Fill), content]
        .width(Fill)
        .height(184.0)
        .pipe(|s| container(s).border(Edge::BevelLo).padding(1.0).clip().width(Fill))
        .into()
}

/// A tile with one big number.
fn tile(icon_name: Icon, role: Role, title: &str, big: String, line: String, go: (&str, Page)) -> Element<Msg> {
    let body = column![
        row![icon(icon_name).size(15.0).role(role), super::caption(title)].spacing(8.0),
        text(big).mono().tabular().size(26.0).weight(600.0),
        text(line).wrap().size(12.0).role(Role::Text2).line_height(1.3),
        spacer(),
        super::link(go.0, Msg::Show(go.1)),
    ]
    .spacing(8.0)
    .width(Fill)
    .height(Fill);
    panel(body).padding([12.0, 14.0, 8.0, 14.0]).width(Fill).height(Fill).into()
}

/// When the system's Nixpkgs was made, as Unix time: as the lock file says
/// for a flake, or as the NixOS version's date (`26.11.20261001.c59305b`).
fn nixpkgs_made(model: &Model) -> Option<i64> {
    let locked = model.inputs.iter().filter(|i| i.name == "nixpkgs").find_map(|i| i.locked.as_ref()?.last_modified);
    locked.or_else(|| {
        let date = model.info.nixos_version.split('.').nth(2).filter(|d| d.len() == 8)?;
        yukimi_system::updates::parse_time(&format!("{}-{}-{}T00:00:00Z", &date[..4], &date[4..6], &date[6..8]))
    })
}

/// The freshness tile's number and line.
fn freshness(app: &Yukimi) -> (String, String) {
    let model = &app.model;
    let (big, mut line) = match (nixpkgs_made(model), model.setup.kind) {
        (Some(made), _) => {
            (age(made, app.now()), "since the version of Nixpkgs your system is built from was published.".to_owned())
        }
        (None, Kind::Channels) if !model.channels.is_empty() => {
            let updated = model.channels.iter().map(|c| c.updated).max().unwrap_or(0);
            (age(updated, app.now()), "since the channels your system is built from were updated.".to_owned())
        }
        _ => ("—".to_owned(), "Yukimi couldn't tell how old your system's Nixpkgs is.".to_owned()),
    };
    if let Some(moves) = app.movements() {
        line.push_str(&match crate::checks::newer_count(&moves) {
            0 => " Everything was up to date when last checked.".to_owned(),
            1 => " One source has a newer version.".to_owned(),
            n => format!(" {n} sources have newer versions."),
        });
    }
    (big, line)
}

fn tiles(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let (chosen, mine) = package_counts(app);
    let mut line = format!("{chosen} chosen for everyone and {mine} just for you");
    match &model.store {
        Some(store) => line.push_str(&format!(
            ", with {} more that your desktops and settings bring.",
            store.system_packages.len().saturating_sub(chosen)
        )),
        None => line.push('.'),
    }
    let packages = tile(
        Icon::Package,
        Role::Accent,
        "Packages",
        (chosen + mine).to_string(),
        line,
        ("See what's installed", Page::Installed),
    );
    let (big, line) = freshness(app);
    let fresh = tile(Icon::RefreshCw, Role::Success, "Freshness", big, line, ("See updates", Page::Updates));
    let (big, line) = match &model.store {
        Some(store) => {
            let c = &store.composition;
            let line = if c.garbage + c.old_generations > 0 {
                format!(
                    "in the store. {} is garbage, and older generations keep {}.",
                    human_size(c.garbage),
                    human_size(c.old_generations)
                )
            } else {
                "in the store.".to_owned()
            };
            (human_size(c.total), line)
        }
        None => ("…".to_owned(), "Reading the store…".to_owned()),
    };
    let storage = tile(Icon::HardDrive, Role::Violet, "Storage", big, line, ("See storage", Page::Storage));
    let gens = &model.generations;
    let line = match gens.first() {
        Some(oldest) if gens.len() > 1 => {
            format!("versions of the system to go back to, the oldest from {}.", human_age(oldest.created, app.now()))
        }
        Some(_) => "version of the system so far. Each change makes another to go back to.".to_owned(),
        None => "No generations were found.".to_owned(),
    };
    let history = tile(
        Icon::GitCommitHorizontal,
        Role::Warning,
        "Generations",
        gens.len().to_string(),
        line,
        ("See history", Page::History),
    );
    grid(vec![packages, fresh, storage, history], 190.0, 12.0)
}

/// What fills the store, as a bar of dye colours with its legend.
fn store_panel(app: &Yukimi) -> Element<Msg> {
    let body: Element<Msg> = match &app.model.store {
        None => super::pending("Reading the store…"),
        Some(store) => {
            let c = &store.composition;
            let legend: Vec<Element<Msg>> = super::storage::parts(c)
                .into_iter()
                .map(|(title, _, size, dye)| {
                    row![super::swatch(dye), text(title).size(12.0).role(Role::Text2), spacer(), mono(human_size(size))]
                        .spacing(8.0)
                        .width(Fill)
                        .into()
                })
                .collect();
            column![
                row![
                    text(human_size(c.total)).mono().tabular().size(20.0).weight(600.0),
                    text("in the Nix store").role(Role::Text3)
                ]
                .spacing(8.0)
                .align(Align::End),
                widgets::bar(super::storage::segments(c), 14.0).width(Fill),
                grid(legend, 200.0, 4.0),
            ]
            .spacing(10.0)
            .width(Fill)
            .into()
        }
    };
    column![
        row![caption("The store"), spacer(), link("See storage", Msg::Show(Page::Storage))].width(Fill),
        panel(body).width(Fill)
    ]
    .spacing(6.0)
    .width(Fill)
    .into()
}

/// Each generation by its size, the running one in the focus colour.
fn generations_panel(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let sizes: Vec<(u32, u64)> = model.store.as_ref().map(|s| s.generation_sizes.clone()).unwrap_or_default();
    let body: Element<Msg> = if sizes.is_empty() {
        if model.store.is_none() {
            super::pending("Reading the store…")
        } else {
            super::empty("No generations were found.")
        }
    } else {
        // The last dozen, sized from a little under the smallest so growth shows.
        let shown = &sizes[sizes.len().saturating_sub(12)..];
        let max = shown.iter().map(|(_, s)| *s).max().unwrap_or(1).max(1) as f64;
        let min = shown.iter().map(|(_, s)| *s).min().unwrap_or(0) as f64;
        let floor = (min * 0.8).min(max * 0.8);
        let running = model.current_generation().map(|g| g.number);
        let bars = shown
            .iter()
            .map(|(n, s)| {
                let share = ((*s as f64 - floor) / (max - floor).max(1.0)).clamp(0.08, 1.0);
                (n.to_string(), format!("{:.1}", *s as f64 / 1e9), share)
            })
            .collect();
        let highlight = shown.iter().position(|(n, _)| Some(*n) == running);
        column![
            columns(bars, highlight, 118.0).width(Fill),
            text("Each generation's size in GB: everything that version of the system needs.")
                .size(11.0)
                .role(Role::Text3)
                .wrap(),
        ]
        .spacing(6.0)
        .width(Fill)
        .into()
    };
    column![
        row![caption("Generations"), spacer(), link("See history", Msg::Show(Page::History))].width(Fill),
        panel(body).width(Fill)
    ]
    .spacing(6.0)
    .width(Fill)
    .into()
}

/// What the latest change brought.
fn recent(app: &Yukimi) -> Option<Element<Msg>> {
    let model = &app.model;
    let n = model.generations.len();
    let body: Element<Msg> = if n >= 2 {
        let (before, after) = (&model.generations[n - 2], &model.generations[n - 1]);
        let said = format!(
            "Generation {} replaced generation {} {}.",
            after.number,
            before.number,
            human_age(after.created, app.now())
        );
        let Some(d) = model.store.as_ref().and_then(|s| s.latest.as_ref()) else {
            return Some(
                column![
                    caption("Most recently"),
                    panel(column![text(said), super::pending("Reading what it changed…")].spacing(6.0)).width(Fill)
                ]
                .spacing(6.0)
                .into(),
            );
        };
        let mut chips = row![].spacing(6.0);
        let mut any = false;
        for (kind, word, role) in [
            (ChangeKind::Upgraded, "upgraded", Role::Success),
            (ChangeKind::Added, "added", Role::Accent),
            (ChangeKind::Removed, "removed", Role::Danger),
            (ChangeKind::Downgraded, "downgraded", Role::Warning),
        ] {
            let count = d.count(kind);
            if count > 0 {
                any = true;
                chips = chips.push(tag(&format!("{count} {word}"), role));
            }
        }
        if !any {
            chips = chips.push(tag("only settings changed", Role::Text3));
        }
        let names: Vec<String> = d.changes.iter().take(8).map(|c| c.name.clone()).collect();
        let mut c =
            column![row![text(said).weight(600.0), spacer(), chips].spacing(12.0).width(Fill)].spacing(8.0).width(Fill);
        if !names.is_empty() {
            let more = if d.changes.len() > 8 { format!(" and {} more", d.changes.len() - 8) } else { String::new() };
            c = c.push(text(format!("{}{more}", names.join(", "))).mono().size(12.0).role(Role::Text2).wrap());
        }
        c.into()
    } else {
        let only = model.generations.last()?;
        text(format!(
            "Generation {} was built {}. Each change you make becomes a new generation, and what it changed shows up \
             here.",
            only.number,
            human_age(only.created, app.now())
        ))
        .wrap()
        .role(Role::Text2)
        .into()
    };
    Some(
        column![
            row![caption("Most recently"), spacer(), link("See what changed", Msg::Show(Page::History))].width(Fill),
            panel(body).width(Fill)
        ]
        .spacing(6.0)
        .width(Fill)
        .into(),
    )
}

/// Where the system comes from, and how fresh each source is.
fn sources(app: &Yukimi) -> Option<Element<Msg>> {
    let model = &app.model;
    let moves = app.movements();
    let rows: Vec<Element<Msg>> = match model.setup.kind {
        Kind::Flake => model
            .inputs
            .iter()
            .filter_map(|input| {
                let locked = input.locked.as_ref().filter(|_| input.follows.is_none())?;
                let source = input.original.as_ref().unwrap_or(locked).describe();
                let state = match moves.as_ref().and_then(|m| m.get(&input.name)) {
                    Some(Movement::Newer(_)) => Some(tag("newer version", Role::Accent)),
                    Some(Movement::Older(_)) => Some(tag("would go back", Role::Warning)),
                    Some(Movement::Current) => Some(tag("up to date", Role::Success)),
                    None => None,
                };
                let mut r = row![
                    monogram(&input.name, 24.0),
                    text(input.name.clone()).weight(600.0).width(150.0),
                    text(source).mono().size(11.5).role(Role::Text3).width(Fill),
                    mono(locked.last_modified.map(|t| human_age(t, app.now())).unwrap_or_default()),
                ]
                .spacing(10.0)
                .padding([7.0, 12.0])
                .width(Fill);
                if let Some(s) = state {
                    r = r.push(container(s).width(118.0).align_x(Align::End));
                }
                Some(r.into())
            })
            .collect(),
        Kind::Channels => model
            .channels
            .iter()
            .map(|c| {
                row![
                    monogram(&c.name, 24.0),
                    text(c.name.clone()).weight(600.0).width(150.0),
                    text(c.release.clone()).mono().size(11.5).role(Role::Text3).width(Fill),
                    mono(format!("updated {}", human_age(c.updated, app.now()))),
                ]
                .spacing(10.0)
                .padding([7.0, 12.0])
                .width(Fill)
                .into()
            })
            .collect(),
    };
    if rows.is_empty() {
        return None;
    }
    Some(
        column![
            row![caption("Where it comes from"), spacer(), link("See updates", Msg::Show(Page::Updates))].width(Fill),
            super::list(rows)
        ]
        .spacing(6.0)
        .width(Fill)
        .into(),
    )
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let mut c = column![hero(app)].spacing(18.0).width(Fill);
    for problem in &app.model.problems {
        c = c.push(banner(Icon::TriangleAlert, Role::Warning, problem.clone()));
    }
    c = c.push(tiles(app));
    c = c.push(
        row![store_panel(app).width(Length::FillPortion(3)), generations_panel(app).width(Length::FillPortion(2))]
            .spacing(14.0)
            .align(Align::Start)
            .width(Fill),
    );
    if let Some(recent) = recent(app) {
        c = c.push(recent);
    }
    if let Some(sources) = sources(app) {
        c = c.push(sources);
    }
    c.into()
}
