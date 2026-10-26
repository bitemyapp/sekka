//! Trackpad: tap to click, how a right-click is made, scrolling, typing.
//! `[input.touchpad]`, applied by the compositor to every touchpad. With
//! none connected (a desktop, a VM) the pane says so: the settings wait
//! for one.

use aizome_config::{AccelProfile, ClickMethod};
use aizome_kit::prelude::*;

use super::mouse::{SCROLL_FACTOR, extra_f64, speed_slider, speed_words};
use crate::form::{self, row};
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

#[derive(Default)]
pub struct Trackpad {
    /// Whether a touchpad is connected (`None` until looked).
    present: Option<bool>,
}

/// Whether `/proc/bus/input/devices` lists a touchpad: a pointer (not a
/// touchscreen) that knows how many fingers touch it.
pub fn touchpad_in(devices: &str) -> bool {
    // BTN_TOOL_FINGER, and INPUT_PROP_POINTER without INPUT_PROP_DIRECT.
    const BTN_TOOL_FINGER: usize = 0x145;
    let bit = |words: &str, n: usize| -> bool {
        // The kernel writes the bitmap as hex words, most significant first.
        let words: Vec<&str> = words.split_whitespace().collect();
        let (word, at) = (n / 64, n % 64);
        words.len() > word && u64::from_str_radix(words[words.len() - 1 - word], 16).is_ok_and(|w| w >> at & 1 == 1)
    };
    devices.split("\n\n").any(|dev| {
        let field = |key: &str| {
            dev.lines().find_map(|l| l.strip_prefix("B: ").and_then(|b| b.strip_prefix(key)).map(str::trim))
        };
        let prop = field("PROP=").unwrap_or("0");
        let keys = field("KEY=").unwrap_or("0");
        bit(prop, 0) && !bit(prop, 1) && bit(keys, BTN_TOOL_FINGER)
    })
}

#[derive(Clone, Debug)]
pub enum Msg {
    Speed(f64),
    Acceleration(bool),
    Tap(bool),
    RightClick(ClickMethod),
    Natural(bool),
    WhileTyping(bool),
    ScrollSpeed(f64),
}

impl Pane for Trackpad {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "trackpad"
    }

    fn title(&self) -> &'static str {
        "Trackpad"
    }

    fn icon(&self) -> Icon {
        Icon::Touchpad
    }

    fn tile(&self) -> Tile {
        Tile::Slate
    }

    fn group(&self) -> Group {
        Group::Devices
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["touchpad", "gestures", "click"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("tap", "Tap to click").keywords(&["touch", "tapping"]),
            SettingEntry::new("right-click", "Secondary click").keywords(&[
                "right click",
                "two finger",
                "corner",
                "context menu",
            ]),
            SettingEntry::new("speed", "Tracking speed").keywords(&["pointer speed", "sensitivity"]),
            SettingEntry::new("acceleration", "Pointer acceleration").keywords(&["accel", "flat", "adaptive"]),
            SettingEntry::new("natural-scroll", "Natural scrolling").keywords(&[
                "scroll direction",
                "reverse",
                "invert",
            ]),
            SettingEntry::new("scroll-speed", "Scrolling speed"),
            SettingEntry::new("disable-while-typing", "Ignore the trackpad while typing").keywords(&[
                "palm",
                "dwt",
                "disable while typing",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        self.present =
            Some(cx.sample() || std::fs::read_to_string("/proc/bus/input/devices").map_or(true, |d| touchpad_in(&d)));
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Speed(v) => cx.change(|s| s.input.touchpad.speed = (v * 20.0).round() / 20.0),
            Msg::Acceleration(on) => cx.change(|s| {
                s.input.touchpad.accel_profile = if on { AccelProfile::Adaptive } else { AccelProfile::Flat }
            }),
            Msg::Tap(on) => cx.change(|s| s.input.touchpad.tap = on),
            Msg::RightClick(m) => cx.change(|s| s.input.touchpad.click_method = m),
            Msg::Natural(on) => cx.change(|s| s.input.touchpad.natural_scroll = on),
            Msg::WhileTyping(on) => cx.change(|s| s.input.touchpad.disable_while_typing = on),
            Msg::ScrollSpeed(v) => cx.change(|s| {
                s.input.touchpad.extra.insert(SCROLL_FACTOR.into(), toml::Value::Float((v * 10.0).round() / 10.0));
            }),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let t = &vcx.settings().input.touchpad;
        let scroll = extra_f64(&t.extra, SCROLL_FACTOR, 1.0);
        let method = match t.click_method {
            ClickMethod::Clickfinger => 0,
            ClickMethod::ButtonAreas => 1,
        };
        let mut sections = vec![
            vcx.section(
                "Clicking",
                vec![
                    row("Tap to click", toggle(t.tap).on_toggle(Msg::Tap))
                        .entry("tap")
                        .help("A light tap clicks; two fingers tap for a secondary click."),
                    row(
                        "Secondary click",
                        form::choice(&["Click with two fingers", "Click in the bottom right corner"], method, |i| {
                            Msg::RightClick(if i == 0 { ClickMethod::Clickfinger } else { ClickMethod::ButtonAreas })
                        }),
                    )
                    .entry("right-click")
                    .help(if method == 0 {
                        "Two fingers click for the context menu, three for a middle click."
                    } else {
                        "The bottom corners of the pad are buttons: right for the context menu, the middle for a middle click."
                    }),
                ],
            ),
            vcx.section(
                "Pointer",
                vec![
                    row("Tracking speed", speed_slider(t.speed, -1.0..=1.0, "Slow", "Fast", Msg::Speed))
                        .entry("speed")
                        .detail(speed_words(t.speed)),
                    row("Pointer acceleration", toggle(t.accel_profile == AccelProfile::Adaptive).on_toggle(Msg::Acceleration))
                        .entry("acceleration"),
                ],
            ),
            vcx.section(
                "Scrolling",
                vec![
                    row("Natural scrolling", toggle(t.natural_scroll).on_toggle(Msg::Natural))
                        .entry("natural-scroll")
                        .help("Content moves with your fingers."),
                    row("Scrolling speed", speed_slider(scroll, 0.2..=3.0, "Slow", "Fast", Msg::ScrollSpeed))
                        .entry("scroll-speed")
                        .detail(format!("{scroll:.1}×")),
                ],
            ),
            vcx.section(
                "Typing",
                vec![
                    row("Ignore the trackpad while typing", toggle(t.disable_while_typing).on_toggle(Msg::WhileTyping))
                        .entry("disable-while-typing")
                        .help("A palm brushing the pad while you type doesn't move the pointer or click."),
                ],
            ),
        ];
        if self.present == Some(false) {
            sections.insert(
                0,
                form::note("No trackpad is connected. These settings wait for one, and apply as soon as it is."),
            );
        }
        form::page(sections)
    }
}

#[cfg(test)]
mod tests {
    use super::touchpad_in;

    const MOUSE: &str = "I: Bus=0011 Vendor=0002 Product=0013 Version=0006
N: Name=\"ImExPS/2 Generic Explorer Mouse\"
B: PROP=1
B: EV=7
B: KEY=1f0000 0 0 0 0
B: REL=143
";
    const TOUCHPAD: &str = "I: Bus=0018 Vendor=06cb Product=7e7e Version=0100
N: Name=\"SYNA2BA6:00 06CB:7E7E Touchpad\"
B: PROP=5
B: EV=1b
B: KEY=e520 10000 0 0 0 0
B: ABS=2e0800000000003
";
    const SCREEN: &str = "I: Bus=0018 Vendor=04f3 Product=2c4d Version=0100
N: Name=\"ELAN Touchscreen\"
B: PROP=2
B: EV=b
B: KEY=400 0 0 0 0 0
B: ABS=3273800000000003
";

    #[test]
    fn a_touchpad_is_told_from_mice_and_touchscreens() {
        assert!(touchpad_in(&format!("{MOUSE}\n{TOUCHPAD}")));
        assert!(!touchpad_in(MOUSE));
        assert!(!touchpad_in(&format!("{MOUSE}\n{SCREEN}")));
    }
}
