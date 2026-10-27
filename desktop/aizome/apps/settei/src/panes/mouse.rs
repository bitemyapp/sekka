//! Mouse: pointer speed and acceleration, scrolling, the primary button.
//! `[input.pointer]`, applied by the compositor to every mouse.

use aizome_config::AccelProfile;
use aizome_kit::prelude::*;

use crate::form::{self, row};
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

/// The extra key the compositor multiplies scrolling by.
pub const SCROLL_FACTOR: &str = "scroll_factor";

#[derive(Default)]
pub struct Mouse;

#[derive(Clone, Debug)]
pub enum Msg {
    Speed(f64),
    Acceleration(bool),
    Natural(bool),
    LeftHanded(bool),
    ScrollSpeed(f64),
}

/// A table's float, or `default`.
pub fn extra_f64(t: &toml::Table, key: &str, default: f64) -> f64 {
    t.get(key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(default)
}

/// A speed of -1…1 as words.
pub fn speed_words(v: f64) -> &'static str {
    match v {
        v if v < -0.6 => "Slowest",
        v if v < -0.2 => "Slow",
        v if v <= 0.2 => "Medium",
        v if v <= 0.6 => "Fast",
        _ => "Fastest",
    }
}

/// A slider with its ends named and a mono read-out.
pub fn speed_slider<Msg: Clone + Send + 'static>(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    left: &str,
    right: &str,
    f: impl Fn(f64) -> Msg + 'static,
) -> Element<Msg> {
    use aizome_kit::row;
    row![
        text(left.to_owned()).size(11.0).role(Role::Text3),
        slider(value, range).on_change(f).width(180.0),
        text(right.to_owned()).size(11.0).role(Role::Text3),
    ]
    .spacing(8.0)
    .align(Align::Center)
    .into()
}

impl Pane for Mouse {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "mouse"
    }

    fn title(&self) -> &'static str {
        "Mouse"
    }

    fn icon(&self) -> Icon {
        Icon::Mouse
    }

    fn tile(&self) -> Tile {
        Tile::Slate
    }

    fn group(&self) -> Group {
        Group::Devices
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["pointer", "cursor", "click"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("speed", "Pointer speed").keywords(&["tracking", "sensitivity", "fast", "slow"]),
            SettingEntry::new("acceleration", "Pointer acceleration").keywords(&[
                "accel",
                "flat",
                "adaptive",
                "precision",
            ]),
            SettingEntry::new("natural-scroll", "Natural scrolling").keywords(&[
                "scroll direction",
                "reverse",
                "invert",
            ]),
            SettingEntry::new("scroll-speed", "Scrolling speed").keywords(&["wheel"]),
            SettingEntry::new("primary-button", "Primary button").keywords(&[
                "left handed",
                "left-handed",
                "swap buttons",
            ]),
        ]
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Speed(v) => cx.change(|s| s.input.pointer.speed = (v * 20.0).round() / 20.0),
            Msg::Acceleration(on) => cx.change(|s| {
                s.input.pointer.accel_profile = if on { AccelProfile::Adaptive } else { AccelProfile::Flat }
            }),
            Msg::Natural(on) => cx.change(|s| s.input.pointer.natural_scroll = on),
            Msg::LeftHanded(on) => cx.change(|s| s.input.pointer.left_handed = on),
            Msg::ScrollSpeed(v) => {
                cx.change(|s| {
                    s.input.pointer.extra.insert(SCROLL_FACTOR.into(), toml::Value::Float((v * 10.0).round() / 10.0));
                });
            }
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let p = &vcx.settings().input.pointer;
        let scroll = extra_f64(&p.extra, SCROLL_FACTOR, 1.0);
        form::page(vec![
            vcx.section(
                "Pointer",
                vec![
                    row("Pointer speed", speed_slider(p.speed, -1.0..=1.0, "Slow", "Fast", Msg::Speed))
                        .entry("speed")
                        .detail(speed_words(p.speed)),
                    row("Pointer acceleration", toggle(p.accel_profile == AccelProfile::Adaptive).on_toggle(Msg::Acceleration))
                        .entry("acceleration")
                        .help("Faster movements carry the pointer further. Turn it off for one-to-one tracking, as games and drawing want."),
                ],
            ),
            vcx.section(
                "Scrolling",
                vec![
                    row("Natural scrolling", toggle(p.natural_scroll).on_toggle(Msg::Natural))
                        .entry("natural-scroll")
                        .help("Content follows the wheel, as it follows a finger on a trackpad."),
                    row("Scrolling speed", speed_slider(scroll, 0.2..=3.0, "Slow", "Fast", Msg::ScrollSpeed))
                        .entry("scroll-speed")
                        .detail(format!("{scroll:.1}×")),
                ],
            ),
            vcx.section(
                "Buttons",
                vec![
                    row(
                        "Primary button",
                        form::segments(&["Left", "Right"], usize::from(p.left_handed), |i| Msg::LeftHanded(i == 1)),
                    )
                    .entry("primary-button")
                    .help("Right for a mouse in the left hand."),
                ],
            ),
        ])
    }
}
