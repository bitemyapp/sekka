//! The Displays pane's page and its "Keep these settings?" sheet.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use super::arrangement::{Tile as ArrTile, arrangement};
use super::model::{self, Display, Use};
use super::{Displays, Keep, Msg};
use crate::form::{self, Row};
use crate::pane::ViewCx;
use crate::widgets::{PressStyle, press};

pub fn page(p: &Displays, vcx: &ViewCx) -> Element<Msg> {
    let ds = p.displays_now();
    if !p.is_loaded() {
        return form::page(vec![
            row![crate::panes::hw::spinner(14.0), text("Asking the compositor about the displays…").role(Role::Text3)]
                .spacing(8.0)
                .into(),
        ]);
    }
    if ds.is_empty() {
        let why = p.error_now().unwrap_or("The compositor reports no displays.").to_owned();
        return form::page(vec![
            form::heading("No displays to arrange", why),
            form::note(
                "Displays are arranged by Aizome's compositor; run Settei inside an Aizome session to change them.",
            ),
        ]);
    }
    let mut sections: Vec<Element<Msg>> = vec![arrangement_section(p, vcx, ds)];
    if let Some(d) = ds.iter().find(|d| d.name == p.selected_now()) {
        sections.extend(display_sections(p, vcx, d));
    }
    sections.extend(night_section(p, vcx));
    form::page(sections)
}

/// Night Light, for every display that can show it; where none can, why
/// there's no switch.
fn night_section(p: &Displays, vcx: &ViewCx) -> Option<Element<Msg>> {
    let n = p.night.as_ref()?;
    let row = if n.outputs.is_empty() {
        form::value("Night Light", "Not on these displays").help(
            "Night Light warms the colours through a display's gamma table, which these displays don't have \
             (virtual displays and some external ones). It shows here for a display that has one.",
        )
    } else {
        let s = &vcx.settings().night_light;
        let when = match s.schedule {
            aizome_config::NightSchedule::Sunset => sunset_words(n, vcx.settings()),
            aizome_config::NightSchedule::Manual => format!("From {} to {}", s.from, s.to),
            aizome_config::NightSchedule::Always => "All the time".to_owned(),
        };
        form::row("Night Light", toggle(s.enabled).on_toggle(Msg::NightLight))
            .help(format!("{when}, warmer colours, easier on the eyes at night."))
    };
    Some(vcx.section("Night Light", vec![row.entry("night-light")]))
}

/// When the sunset schedule warms the displays today, as the compositor
/// reckons it: "From sunset (18:57) to sunrise (08:15) in Paris".
pub fn sunset_words(n: &aizome_ipc::NightLightState, s: &aizome_config::Settings) -> String {
    let place = match n.place.as_deref() {
        None => return "From 19:00 to 07:00 (where you are isn't known)".to_owned(),
        Some("located" | "settings") => "where you are".to_owned(),
        Some(zone) => {
            aizome_config::location::zone_city(zone).map_or("in your time zone".into(), |c| format!("in {c}"))
        }
    };
    match (&n.from, &n.to) {
        (Some(from), Some(to)) => format!("From sunset ({from}) to sunrise ({to}) {place}"),
        _ => {
            let now = aizome_config::LocalNow::now();
            match aizome_config::daylight(s.place().as_ref(), now.day_of_year, now.utc_offset) {
                Some(aizome_config::Span::Never) => format!("All day: the sun doesn't rise {place} today"),
                _ => format!("Not today: the sun doesn't set {place}"),
            }
        }
    }
}

/// A display's name, with its connector when another has the same name.
pub fn label(d: &Display, ds: &[Display]) -> String {
    let t = d.title();
    if ds.iter().any(|o| o.name != d.name && o.title() == t) { format!("{t} ({})", d.name) } else { t }
}

fn tiles(p: &Displays, ds: &[Display]) -> Vec<ArrTile> {
    let mut names: Vec<&str> = ds.iter().map(|d| d.name.as_str()).collect();
    names.sort();
    ds.iter()
        .filter(|d| model::arranged(&d.settings))
        .map(|d| {
            let (w, h) = d.settings.logical_size();
            ArrTile {
                name: d.name.clone(),
                rect: model::rect(&d.settings),
                title: d.title(),
                size: model::size_label(w, h),
                main: p.main_now() == Some(d.name.as_str()),
                mirrors: ds
                    .iter()
                    .filter(|o| o.settings.enabled && o.settings.mirror.as_deref() == Some(d.name.as_str()))
                    .map(Display::title)
                    .collect(),
                dye: names.iter().position(|n| *n == d.name).unwrap_or(0),
            }
        })
        .collect()
}

fn arrangement_section(p: &Displays, vcx: &ViewCx, ds: &[Display]) -> Element<Msg> {
    let tiles = tiles(p, ds);
    let selected = p.selected_now().to_owned();
    let mut col = column![
        form::caption("Arrangement"),
        arrangement(tiles, &selected, Msg::Select, Msg::Moved, !p.is_applying()).id(vcx.entry_name("arrangement")),
        container(
            text("Drag displays to arrange them. Select one to change its settings.").size(11.0).role(Role::Text3)
        )
        .center()
        .width(Fill),
    ]
    .spacing(8.0)
    .width(Fill);
    if ds.len() > 1 {
        let labels: Vec<String> = ds
            .iter()
            .map(|d| if d.settings.enabled { label(d, ds) } else { format!("{} (Off)", label(d, ds)) })
            .collect();
        let index = ds.iter().position(|d| d.name == selected);
        let names: Vec<String> = ds.iter().map(|d| d.name.clone()).collect();
        col = col.push(
            container(segmented(labels, index).on_select(move |i| Msg::Select(names[i.min(names.len() - 1)].clone())))
                .center()
                .width(Fill),
        );
    }
    col.into()
}

/// "27-inch display · DP-2 · 3840 × 2160 at 144 Hz".
fn describe(d: &Display) -> String {
    let mut parts = Vec::new();
    if let Some(inches) = d.diagonal_inches() {
        parts.push(format!("{inches:.0}-inch display"));
    }
    parts.push(d.name.clone());
    let s = &d.settings;
    if s.enabled {
        parts.push(format!(
            "{} at {}",
            model::size_label(s.mode.width, s.mode.height),
            model::refresh_label(s.mode.refresh)
        ));
    } else {
        parts.push("off".into());
    }
    parts.join(" · ")
}

fn display_sections(p: &Displays, vcx: &ViewCx, d: &Display) -> Vec<Element<Msg>> {
    let ds = p.displays_now();
    let name = d.name.clone();
    let s = &d.settings;
    let mut out: Vec<Element<Msg>> = vec![form::heading(label(d, ds), describe(d))];

    // What it's used as.
    let mut uses: Vec<(String, Use)> = vec![("Main display".into(), Use::Main)];
    if ds.len() > 1 {
        uses.push(("Extended display".into(), Use::Extended));
    }
    for src in ds.iter().filter(|o| o.name != name && model::arranged(&o.settings)) {
        uses.push((format!("Mirror for {}", label(src, ds)), Use::Mirror(src.name.clone())));
    }
    let others_on = ds.iter().any(|o| o.name != name && o.settings.enabled);
    if others_on || !s.enabled {
        uses.push(("Off".into(), Use::Off));
    }
    let now = if !s.enabled {
        Use::Off
    } else if let Some(src) = &s.mirror {
        Use::Mirror(src.clone())
    } else if p.main_now() == Some(name.as_str()) {
        Use::Main
    } else {
        Use::Extended
    };
    let labels: Vec<&str> = uses.iter().map(|(l, _)| l.as_str()).collect();
    let selected = uses.iter().position(|(_, u)| *u == now).unwrap_or(0);
    let help = match now {
        Use::Off => "Stays off until you turn it on here".to_owned(),
        Use::Mirror(_) => "Shows the same picture, at the same size where its pixels allow".to_owned(),
        Use::Main => "The main display comes first: the shell favours it for what it opens".to_owned(),
        Use::Extended => "Part of the desktop, beside the others".to_owned(),
    };
    let values: Vec<Use> = uses.iter().map(|(_, u)| u.clone()).collect();
    let n = name.clone();
    let mut rows: Vec<Row<Msg>> = vec![
        form::row(
            "Use as",
            form::choice(&labels, selected, move |i| {
                Msg::UseAs(n.clone(), values.get(i).cloned().unwrap_or(Use::Extended))
            }),
        )
        .help(help)
        .entry("use-as"),
    ];
    if d.is_builtin()
        && s.enabled
        && let Some(level) = p.brightness_now()
    {
        rows.push(
            form::row(
                "Brightness",
                row![
                    icon(Icon::SunDim).size(14.0).role(Role::Text3),
                    slider(level as f64, 0.01..=1.0).step(0.01).on_change(|v| Msg::Brightness(v as f32)).width(200.0),
                    icon(Icon::Sun).size(14.0).role(Role::Text3)
                ]
                .spacing(8.0),
            )
            .entry("brightness"),
        );
    }
    out.push(vcx.section("", rows));
    if !s.enabled || s.mirror.is_some() {
        return out;
    }

    // Resolution: scaled sizes first, as in macOS, or every one.
    let mut res: Vec<Row<Msg>> = Vec::new();
    if p.show_all_now() {
        let sizes = d.resolutions();
        let labels: Vec<String> = sizes
            .iter()
            .map(|(w, h)| {
                let mut l = model::size_label(*w, *h);
                if d.preferred.is_some_and(|m| m.width == *w && m.height == *h) {
                    l.push_str(" (native)");
                }
                l
            })
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = sizes.iter().position(|sz| *sz == (s.mode.width, s.mode.height)).unwrap_or(0);
        let n = name.clone();
        res.push(
            form::row(
                "Resolution",
                form::choice(&labels, at, move |i| Msg::Resolution(n.clone(), sizes.get(i).copied().unwrap_or((0, 0)))),
            )
            .entry("resolution"),
        );
        let mut scales = model::integer_scales(s.mode.width, s.mode.height, s.scale);
        if p.fractional_now() {
            scales.extend(model::fractional_scales(s.mode.width, s.mode.height, s.scale));
            scales.sort_by(|a, b| a.total_cmp(b));
            scales.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        }
        let labels: Vec<String> = scales
            .iter()
            .map(|sc| {
                let (w, h) = model::logical_size(&s.mode, *sc, aizome_ipc::Transform::Normal);
                format!("{} — looks like {}", model::scale_label(*sc), model::size_label(w, h))
            })
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = scales.iter().position(|sc| (sc - s.scale).abs() < 1e-6).unwrap_or(0);
        let n = name.clone();
        res.push(form::row(
            "Scale",
            form::choice(&labels, at, move |i| Msg::Scale(n.clone(), scales.get(i).copied().unwrap_or(1.0))),
        ));
    } else {
        res.push(form::whole(scale_cards(p, d)).entry("resolution"));
    }
    res.push(
        form::row("Fractional scaling", toggle(p.fractional_now()).on_toggle(Msg::Fractional))
            .help("Sizes between the whole steps. Apps draw at the next whole scale and are shrunk to fit, so text is a little softer.")
            .entry("fractional"),
    );
    res.push(form::row("Show all resolutions", toggle(p.show_all_now()).on_toggle(Msg::ShowAll)));
    out.push(vcx.section("Resolution", res));

    // Refresh rate and rotation.
    let rates = d.refresh_rates(s.mode.width, s.mode.height);
    let labels: Vec<String> = rates.iter().map(|r| model::refresh_label(*r)).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let at = model::nearest(&rates, s.mode.refresh).unwrap_or(0);
    let n = name.clone();
    let mut picture: Vec<Row<Msg>> = Vec::new();
    if rates.len() > 1 {
        picture.push(
            form::row(
                "Refresh rate",
                form::choice(&labels, at, move |i| Msg::Refresh(n.clone(), rates.get(i).copied().unwrap_or(60_000))),
            )
            .entry("refresh-rate"),
        );
    } else {
        picture.push(form::value("Refresh rate", model::refresh_label(s.mode.refresh)).entry("refresh-rate"));
    }
    if d.vrr_supported {
        let n = name.clone();
        let at = match s.vrr {
            aizome_config::VrrMode::Off => 0,
            aizome_config::VrrMode::Fullscreen => 1,
            aizome_config::VrrMode::Always => 2,
        };
        picture.push(
            form::row(
                "Adaptive sync",
                form::choice(&["Off", "Fullscreen", "Always"], at, move |i| Msg::Vrr(n.clone(), i)),
            )
            .help("Match the display to the game's frame rate. Choose Off if the screen flickers.")
            .entry("adaptive-sync"),
        );
    } else {
        picture.push(form::value("Adaptive sync", "Unavailable on this display or mode").entry("adaptive-sync"));
    }
    let labels: Vec<&str> = model::ROTATIONS.iter().map(|(_, l)| *l).collect();
    let n = name.clone();
    picture.push(
        form::row(
            "Rotation",
            form::choice(&labels, model::rotation_index(s.transform), move |i| Msg::Rotation(n.clone(), i)),
        )
        .entry("rotation"),
    );
    out.push(vcx.section("", picture));
    out
}

/// The scaled sizes as macOS offers them: from larger text to more space,
/// each as a small screen of that size with "Aa" at the size text takes.
fn scale_cards(p: &Displays, d: &Display) -> Element<Msg> {
    let s = &d.settings;
    let mut scales = model::integer_scales(s.mode.width, s.mode.height, s.scale);
    if p.fractional_now() {
        scales.extend(model::fractional_scales(s.mode.width, s.mode.height, s.scale));
    }
    // Larger text first: the largest scale.
    scales.sort_by(|a, b| b.total_cmp(a));
    scales.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let sizes: Vec<(i32, i32)> =
        scales.iter().map(|sc| model::logical_size(&s.mode, *sc, aizome_ipc::Transform::Normal)).collect();
    let widest = sizes.iter().map(|(w, _)| *w).max().unwrap_or(1).max(1) as f64;
    let narrowest = sizes.iter().map(|(w, _)| *w).min().unwrap_or(1).max(1) as f64;
    let mut cards: Vec<Element<Msg>> = Vec::new();
    for (sc, (w, h)) in scales.iter().zip(&sizes) {
        let gw = (28.0 + 44.0 * *w as f64 / widest).round();
        let gh = (gw * *h as f64 / *w as f64).round();
        let aa = (7.0 + 9.0 * narrowest / *w as f64).round();
        let screen = stack![
            canvas((gw as u32, gh as u32), |c, r, _, t| {
                c.fill_rect(r, t.selection());
                c.fill_rect(aizome_kit::Rect::new(r.x0, r.y0, r.x1, r.y0 + 3.0), t.bar());
                aizome_kit::draw::border(c, r, t.bevel_lo(), 1.0);
            })
            .width(gw)
            .height(gh),
            container(text("Aa").size(aa).weight(600.0).color(Color::rgba8(238, 241, 255, 255)))
                .width(gw)
                .height(gh)
                .center(),
        ];
        let current = (sc - s.scale).abs() < 1e-6;
        let face = column![
            container(screen).height(56.0).width(Fill).align_x(Align::Center).align_y(Align::End),
            text(model::size_label(*w, *h)).mono().size(10.5).role(if current { Role::Text } else { Role::Text2 }),
            text(model::scale_label(*sc)).size(10.5).role(Role::Text3),
        ]
        .spacing(4.0)
        .align(Align::Center)
        .width(Fill);
        let name = d.name.clone();
        let sc = *sc;
        cards.push(
            press(face)
                .style(PressStyle::Card)
                .selected(current)
                .padding([8.0, 6.0])
                .on_press(Msg::Scale(name, sc))
                .named(format!("Looks like {} ({})", model::size_label(*w, *h), model::scale_label(sc)))
                .width(Fill)
                .into(),
        );
    }
    column![
        row(cards).spacing(8.0).width(Fill),
        row![
            text("Larger Text").size(11.0).role(Role::Text3),
            spacer(),
            text("More Space").size(11.0).role(Role::Text3)
        ]
        .width(Fill),
    ]
    .spacing(6.0)
    .padding([12.0, 12.0])
    .width(Fill)
    .into()
}

fn countdown(seconds: u32) -> String {
    match seconds {
        1 => "Going back to the settings before in 1 second.".to_owned(),
        s => format!("Going back to the settings before in {s} seconds."),
    }
}

pub fn keep_sheet(k: &Keep) -> Element<Msg> {
    container(
        column![
            row![
                icon(Icon::Monitor).size(28.0).role(Role::Accent),
                text("Keep these display settings?").size(15.0).weight(650.0)
            ]
            .spacing(12.0),
            text(countdown(k.left)).role(Role::Text2).wrap(),
            progress(k.left as f64 / super::KEEP_SECONDS as f64),
            row![
                spacer(),
                button("Revert").cancel().on_press(Msg::Revert),
                button("Keep Changes").primary().default().on_press(Msg::Keep)
            ]
            .spacing(8.0)
            .width(Fill),
        ]
        .spacing(12.0),
    )
    .padding([20.0, 20.0, 16.0, 20.0])
    .width(400.0)
    .into()
}
