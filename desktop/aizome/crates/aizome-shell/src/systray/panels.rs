//! The systray's panels: a popup under each widget (`raised`, a hard
//! shadow, 320pt wide) of quick controls, its last row opening the
//! matching pane of Settei.

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Layoutable, column, segmented, slider, toggle};
use aizome_theme::typeface;

use crate::clock::Local;
use crate::look::{self, Hue};
use crate::menubar::TrayWidget;
use crate::widgets::drawn::{around, drawn};
use crate::widgets::field::field;

pub const WIDTH: f64 = 320.0;
pub const SHADOW: f64 = 3.0;
const PAD: f64 = 12.0;
const ROW: f64 = 30.0;
const HEAD: f64 = 36.0;
const SEP: f64 = 9.0;

/// What a panel's controls say.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelMsg {
    Wifi(bool),
    /// A network picked: joins it, or asks its password first.
    Network(String),
    Password(String),
    Join,
    Vpn(String, bool),
    Airplane(bool),
    Volume(f32),
    /// The output's volume slider let go (feedback plays then).
    VolumeSet,
    Mute,
    Output(String),
    InputVolume(f32),
    InputMute,
    Media(MediaVerb),
    Bluetooth(bool),
    /// Connect (true) or disconnect a device, by its path.
    Device(String, bool),
    Brightness(String, f32),
    Theme(usize),
    Size(usize),
    PowerMode(String),
    DoNotDisturb(bool),
    NightLight(bool),
    /// Keep Awake turned on or off.
    KeepAwake(bool),
    /// Open Koyomi at a day (`YYYY-MM-DD`).
    OpenDay(String),
    /// Open Tokei's world clocks.
    OpenClocks,
    /// Open Settei at a pane.
    Settings(String),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaVerb {
    Previous,
    PlayPause,
    Next,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetRow {
    pub ssid: String,
    pub bars: u8,
    pub secure: bool,
    pub connected: bool,
    pub connecting: bool,
    pub known: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetPanel {
    /// The Wi-Fi switch, when there's a Wi-Fi device.
    pub wifi: Option<bool>,
    pub networks: Vec<NetRow>,
    /// VPNs: uuid, name, on.
    pub vpns: Vec<(String, String, bool)>,
    pub airplane: bool,
    /// The network whose password is being typed, and what's typed.
    pub asking: Option<(String, String)>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundPanel {
    pub output: Option<(f32, bool)>,
    /// Outputs: name, description, the default.
    pub outputs: Vec<(String, String, bool)>,
    pub input: Option<(f32, bool)>,
    pub media: Option<Media>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BtDevice {
    pub path: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BtPanel {
    pub powered: bool,
    pub devices: Vec<BtDevice>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayPanel {
    /// Backlights: name and level.
    pub backlights: Vec<(String, f32)>,
    /// Night, Day, Auto.
    pub theme: usize,
    /// Small, Medium, Large.
    pub size: usize,
    /// Night Light, where the compositor can show it: on or not.
    pub night_light: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryPanel {
    pub percent: f32,
    pub state: String,
    pub time: Option<String>,
    /// The power modes there are (`power-saver`, `balanced`, `performance`)
    /// and the active one's index.
    pub modes: Vec<String>,
    pub mode: Option<usize>,
    pub keep_awake: aizome_awake::Status,
}

/// Keep Awake's panel, under its cup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AwakePanel {
    pub status: aizome_awake::Status,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClockPanel {
    pub now: Local,
    /// Do Not Disturb is on.
    pub dnd: bool,
    /// The next events from Koyomi (`None` without Koyomi).
    pub events: Option<Vec<crate::agenda::EventRow>>,
    /// Tokei's world clocks.
    pub zones: Vec<crate::agenda::ZoneRow>,
}

/// The most events the clock's panel lists.
pub const EVENTS: usize = 4;

/// What each panel shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    Awake(AwakePanel),
    Network(NetPanel),
    Sound(SoundPanel),
    Bluetooth(BtPanel),
    Display(DisplayPanel),
    Battery(BatteryPanel),
    Clock(ClockPanel),
}

impl Panel {
    pub fn widget(&self) -> TrayWidget {
        match self {
            Panel::Awake(_) => TrayWidget::Awake,
            Panel::Network(_) => TrayWidget::Network,
            Panel::Sound(_) => TrayWidget::Sound,
            Panel::Bluetooth(_) => TrayWidget::Bluetooth,
            Panel::Display(_) => TrayWidget::Display,
            Panel::Battery(_) => TrayWidget::Battery,
            Panel::Clock(_) => TrayWidget::Clock,
        }
    }
}

fn ui(t: &aizome_kit::Theme, size: f64, weight: f32, c: aizome_kit::Color) -> aizome_kit::text::TextSpec {
    look::ui(size * t.metrics.factor, weight, c)
}

/// A row with a label at the left and a control (or nothing) at the right.
fn labelled(label: &str, weight: f32, right: Option<Element<PanelMsg>>, h: f64) -> Element<PanelMsg> {
    let l = label.to_owned();
    let text = drawn(0.0, h, (l.clone(), weight.to_bits()), move |cx| {
        let t = cx.theme();
        let s = ui(t, typeface::BODY, weight, t.text());
        cx.draw_text_in(&l, &s, cx.bounds(), Align::Start);
    })
    .text(label.to_owned(), SemanticRole::Label);
    let mut items = vec![Element::from(text).width(aizome_kit::Length::Fill)];
    if let Some(r) = right {
        items.push(r);
    }
    aizome_kit::row(items).align(Align::Center).height(h).padding([0.0, PAD]).into()
}

fn head(title: &str, right: Option<Element<PanelMsg>>) -> Element<PanelMsg> {
    labelled(title, typeface::WEIGHT_TAB, right, HEAD)
}

fn sep() -> Element<PanelMsg> {
    drawn(WIDTH, SEP, "sep", |cx| {
        let t = cx.theme();
        aizome_kit::draw::separator(cx.canvas, t, 0.0, cx.bounds().x1, 4.0);
    })
    .into()
}

/// A row to click: a glyph, a label, and a mark or text at the right.
fn item_row(
    glyph: Option<(Icon, Hue)>,
    label: String,
    right: Option<String>,
    lock: bool,
    checked: bool,
    msg: Option<PanelMsg>,
    key: String,
) -> Element<PanelMsg> {
    let (l, r) = (label.clone(), right.clone());
    let d = drawn(WIDTH, ROW, (l.clone(), r.clone(), lock, checked, glyph.map(|g| g.0)), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let fg = if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.selection());
            t.on_selection()
        } else {
            t.text()
        };
        let mut x = PAD;
        if checked {
            Icon::Check.draw(
                cx.canvas,
                Point::new(x, ((ROW - 14.0) / 2.0).round()),
                14.0,
                if cx.is_hovered() { fg } else { t.focus() },
            );
        }
        if glyph.is_some() || checked {
            x += 20.0;
        }
        if let Some((g, hue)) = glyph {
            g.draw(
                cx.canvas,
                Point::new(x, ((ROW - 16.0) / 2.0).round()),
                16.0,
                if cx.is_hovered() { fg } else { hue.color(t) },
            );
            x += 24.0;
        }
        let s = ui(t, typeface::BODY, typeface::WEIGHT_REGULAR, fg);
        cx.draw_text_in(&l, &s, Rect::new(x, 0.0, b.x1 - PAD - 60.0, ROW), Align::Start);
        let mut x1 = b.x1 - PAD;
        if lock {
            Icon::Lock.draw(
                cx.canvas,
                Point::new(x1 - 13.0, ((ROW - 13.0) / 2.0).round()),
                13.0,
                if cx.is_hovered() { fg } else { t.text3() },
            );
            x1 -= 20.0;
        }
        if let Some(r) = &r {
            let rs = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, if cx.is_hovered() { fg } else { t.text3() });
            cx.draw_text_in(r, &rs, Rect::new(x1 - 80.0, 0.0, x1, ROW), Align::End);
        }
    })
    .text(label, SemanticRole::MenuItem)
    .key(key);
    match msg {
        Some(m) => d.on_click(m).into(),
        None => d.into(),
    }
}

/// The last row: Settei's pane.
fn settings_row(label: &str, pane: &str) -> Element<PanelMsg> {
    item_row(
        None,
        label.to_owned(),
        None,
        false,
        false,
        Some(PanelMsg::Settings(pane.to_owned())),
        format!("settings-{pane}"),
    )
}

fn slider_row(
    icon: Icon,
    value: f32,
    on_change: impl Fn(f64) -> PanelMsg + 'static,
    right: Option<Element<PanelMsg>>,
) -> Element<PanelMsg> {
    slider_row_released(icon, value, on_change, None, right)
}

fn slider_row_released(
    icon: Icon,
    value: f32,
    on_change: impl Fn(f64) -> PanelMsg + 'static,
    on_release: Option<PanelMsg>,
    right: Option<Element<PanelMsg>>,
) -> Element<PanelMsg> {
    let glyph = drawn(20.0, ROW, icon, move |cx| {
        let t = cx.theme();
        icon.draw(cx.canvas, Point::new(0.0, ((ROW - 16.0) / 2.0).round()), 16.0, t.text2());
    });
    let mut items: Vec<Element<PanelMsg>> = vec![
        glyph.into(),
        Element::from(match on_release {
            Some(m) => slider(value as f64, 0.0..=1.0).on_change(on_change).on_release(m),
            None => slider(value as f64, 0.0..=1.0).on_change(on_change),
        })
        .width(aizome_kit::Length::Fill),
    ];
    if let Some(r) = right {
        items.push(r);
    }
    aizome_kit::row(items).spacing(8.0).align(Align::Center).height(ROW + 4.0).padding([0.0, PAD]).into()
}

fn icon_btn(icon: Icon, msg: PanelMsg, label: &str) -> Element<PanelMsg> {
    drawn(24.0, 24.0, icon, move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.raised()));
        }
        icon.draw(cx.canvas, Point::new(4.0, 4.0), 16.0, t.text2());
    })
    .on_click(msg)
    .text(label.to_owned(), SemanticRole::Button)
    .into()
}

fn network(p: &NetPanel) -> Vec<Element<PanelMsg>> {
    let mut v = Vec::new();
    if let Some(on) = p.wifi {
        v.push(head("Wi-Fi", Some(toggle(on).on_toggle(PanelMsg::Wifi).into())));
        if on {
            for n in p.networks.iter().take(8) {
                let glyph = match n.bars {
                    0 | 1 => Icon::WifiLow,
                    2 | 3 => Icon::WifiHigh,
                    _ => Icon::Wifi,
                };
                let right = n.connecting.then(|| "joining…".to_owned());
                v.push(item_row(
                    Some((glyph, Hue::Text2)),
                    n.ssid.clone(),
                    right,
                    n.secure,
                    n.connected,
                    Some(PanelMsg::Network(n.ssid.clone())),
                    format!("net-{}", n.ssid),
                ));
                if let Some((ssid, pw)) = &p.asking
                    && *ssid == n.ssid
                {
                    let f = field(pw, |t| look::ui(13.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text()))
                        .secure()
                        .placeholder("Password")
                        .height(24.0)
                        .on_change(PanelMsg::Password)
                        .on_submit(PanelMsg::Join);
                    let f = Element::from(f).id("wifi-password");
                    let boxed = around(f, [0.0, 6.0], Size::new(0.0, 26.0), "pw-box", |cx| {
                        let t = cx.theme();
                        let b = cx.bounds();
                        aizome_kit::draw::sunken(cx.canvas, t, b, t.surface(), t.line2());
                        cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), t.focus());
                    });
                    let join = aizome_kit::button("Join").primary().compact().on_press(PanelMsg::Join);
                    v.push(
                        aizome_kit::row(vec![Element::from(boxed).width(aizome_kit::Length::Fill), join.into()])
                            .spacing(8.0)
                            .align(Align::Center)
                            .padding([2.0, PAD, 4.0, PAD + 44.0])
                            .into(),
                    );
                }
            }
            if let Some(e) = &p.error {
                let e = e.clone();
                v.push(
                    drawn(WIDTH, 22.0, e.clone(), move |cx| {
                        let t = cx.theme();
                        let s = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, Hue::Red.color(t));
                        cx.draw_text_in(&e, &s, Rect::new(PAD, 0.0, WIDTH - PAD, 22.0), Align::Start);
                    })
                    .into(),
                );
            }
        }
        v.push(sep());
    }
    for (uuid, name, on) in &p.vpns {
        let id = uuid.clone();
        v.push(labelled(
            name,
            typeface::WEIGHT_REGULAR,
            Some(toggle(*on).on_toggle(move |b| PanelMsg::Vpn(id.clone(), b)).into()),
            ROW,
        ));
    }
    v.push(labelled(
        "Airplane Mode",
        typeface::WEIGHT_REGULAR,
        Some(toggle(p.airplane).on_toggle(PanelMsg::Airplane).into()),
        ROW,
    ));
    v.push(sep());
    v.push(settings_row("Network Settings…", "network"));
    v
}

fn sound(p: &SoundPanel) -> Vec<Element<PanelMsg>> {
    let mut v = vec![head("Sound", None)];
    if let Some((vol, muted)) = p.output {
        let icon = crate::menubar::sound_icon(vol, muted);
        v.push(slider_row_released(
            icon,
            vol,
            |x| PanelMsg::Volume(x as f32),
            Some(PanelMsg::VolumeSet),
            Some(icon_btn(if muted { Icon::VolumeX } else { Icon::Volume2 }, PanelMsg::Mute, "Mute")),
        ));
    }
    if p.outputs.len() > 1 {
        for (name, desc, on) in &p.outputs {
            v.push(item_row(
                Some((Icon::Speaker, Hue::Text2)),
                desc.clone(),
                None,
                false,
                *on,
                Some(PanelMsg::Output(name.clone())),
                format!("out-{name}"),
            ));
        }
    }
    if let Some((vol, muted)) = p.input {
        v.push(sep());
        v.push(slider_row(
            if muted { Icon::MicOff } else { Icon::Mic },
            vol,
            |x| PanelMsg::InputVolume(x as f32),
            Some(icon_btn(if muted { Icon::MicOff } else { Icon::Mic }, PanelMsg::InputMute, "Mute input")),
        ));
    }
    if let Some(m) = &p.media {
        v.push(sep());
        let (title, artist, playing) = (m.title.clone(), m.artist.clone(), m.playing);
        let info = drawn(0.0, 44.0, (title.clone(), artist.clone()), move |cx| {
            let t = cx.theme();
            let b = cx.bounds();
            let art = Rect::new(0.0, 4.0, 36.0, 40.0);
            cx.canvas.fill_rect(art, t.sunk());
            aizome_kit::draw::border(cx.canvas, art, t.line(), 1.0);
            Icon::EighthNotes.draw(cx.canvas, Point::new(10.0, 14.0), 16.0, t.text3());
            let s = ui(t, typeface::BODY, typeface::WEIGHT_SEMIBOLD, t.text());
            cx.draw_text_in(&title, &s, Rect::new(46.0, 4.0, b.x1, 22.0), Align::Start);
            let a = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&artist, &a, Rect::new(46.0, 22.0, b.x1, 40.0), Align::Start);
        });
        let controls = aizome_kit::row(vec![
            icon_btn(Icon::SkipBack, PanelMsg::Media(MediaVerb::Previous), "Previous"),
            icon_btn(
                if playing { Icon::Pause } else { Icon::Play },
                PanelMsg::Media(MediaVerb::PlayPause),
                "Play or pause",
            ),
            icon_btn(Icon::SkipForward, PanelMsg::Media(MediaVerb::Next), "Next"),
        ])
        .spacing(2.0);
        v.push(
            aizome_kit::row(vec![Element::from(info).width(aizome_kit::Length::Fill), controls.into()])
                .align(Align::Center)
                .padding([0.0, PAD])
                .into(),
        );
    }
    v.push(sep());
    v.push(settings_row("Sound Settings…", "sound"));
    v
}

fn bluetooth(p: &BtPanel) -> Vec<Element<PanelMsg>> {
    let mut v = vec![head("Bluetooth", Some(toggle(p.powered).on_toggle(PanelMsg::Bluetooth).into()))];
    if p.powered {
        for d in &p.devices {
            let right = match (d.connected, d.battery) {
                (true, Some(b)) => Some(format!("{b}%")),
                (true, None) => Some("connected".into()),
                (false, _) => None,
            };
            v.push(item_row(
                Some((Icon::Bluetooth, Hue::Blue)),
                d.name.clone(),
                right,
                false,
                d.connected,
                Some(PanelMsg::Device(d.path.clone(), !d.connected)),
                format!("bt-{}", d.path),
            ));
        }
    }
    v.push(sep());
    v.push(settings_row("Bluetooth Settings…", "bluetooth"));
    v
}

fn display(p: &DisplayPanel) -> Vec<Element<PanelMsg>> {
    let mut v = vec![head("Display", None)];
    for (name, level) in &p.backlights {
        let n = name.clone();
        v.push(slider_row(Icon::SunDim, *level, move |x| PanelMsg::Brightness(n.clone(), x as f32), None));
    }
    v.push(labelled("Appearance", typeface::WEIGHT_REGULAR, None, 24.0));
    v.push(
        aizome_kit::container(segmented(["Night", "Day", "Auto"], p.theme).on_select(PanelMsg::Theme))
            .padding([0.0, PAD, 6.0, PAD])
            .into(),
    );
    v.push(labelled("Size", typeface::WEIGHT_REGULAR, None, 24.0));
    v.push(
        aizome_kit::container(segmented(["Small", "Medium", "Large"], p.size).on_select(PanelMsg::Size))
            .padding([0.0, PAD, 6.0, PAD])
            .into(),
    );
    if let Some(on) = p.night_light {
        v.push(labelled(
            "Night Light",
            typeface::WEIGHT_REGULAR,
            Some(toggle(on).on_toggle(PanelMsg::NightLight).into()),
            ROW,
        ));
    }
    v.push(sep());
    v.push(settings_row("Displays Settings…", "displays"));
    v
}

fn battery(p: &BatteryPanel) -> Vec<Element<PanelMsg>> {
    let mut v = vec![head(&format!("Battery  {:.0}%", p.percent), None)];
    let line = match &p.time {
        Some(t) => format!("{} · {t}", p.state),
        None => p.state.clone(),
    };
    let pct = p.percent;
    v.push(
        drawn(WIDTH, 34.0, (line.clone(), (pct * 10.0) as i32), move |cx| {
            let t = cx.theme();
            let bar = Rect::new(PAD, 4.0, WIDTH - PAD, 12.0);
            cx.canvas.fill_rect(bar, t.sunk());
            aizome_kit::draw::border(cx.canvas, bar, t.line2(), 1.0);
            let fill = Rect::new(
                bar.x0 + 1.0,
                bar.y0 + 1.0,
                bar.x0 + 1.0 + (bar.width() - 2.0) * (pct as f64 / 100.0).clamp(0.0, 1.0),
                bar.y1 - 1.0,
            );
            let c = if pct < 15.0 { Hue::Red.color(t) } else { Hue::Green.color(t) };
            cx.canvas.fill_rect(fill, c);
            let s = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&line, &s, Rect::new(PAD, 16.0, WIDTH - PAD, 32.0), Align::Start);
        })
        .into(),
    );
    if !p.modes.is_empty() {
        v.push(labelled("Power Mode", typeface::WEIGHT_REGULAR, None, 24.0));
        let labels: Vec<String> = p
            .modes
            .iter()
            .map(|m| match m.as_str() {
                "power-saver" => "Power Saver".to_owned(),
                "balanced" => "Balanced".to_owned(),
                "performance" => "Performance".to_owned(),
                o => o.to_owned(),
            })
            .collect();
        let modes = p.modes.clone();
        v.push(
            aizome_kit::container(
                segmented(labels, p.mode)
                    .on_select(move |i| PanelMsg::PowerMode(modes.get(i).cloned().unwrap_or_default())),
            )
            .padding([0.0, PAD, 6.0, PAD])
            .into(),
        );
    }
    v.push(labelled(
        "Keep Awake",
        typeface::WEIGHT_REGULAR,
        Some(toggle(p.keep_awake != aizome_awake::Status::Off).on_toggle(PanelMsg::KeepAwake).into()),
        ROW,
    ));
    v.push(sep());
    v.push(settings_row("Battery Settings…", "power"));
    v
}

/// Keep Awake's two lines under its switch.
const AWAKE_TEXT: f64 = 40.0;

fn awake(p: &AwakePanel) -> Vec<Element<PanelMsg>> {
    use aizome_awake::Status;
    let on = p.status != Status::Off;
    let mut v = vec![head("Keep Awake", Some(toggle(on).on_toggle(PanelMsg::KeepAwake).into()))];
    let (lines, hue) = match p.status {
        Status::On => (["Won't sleep, lock or turn the displays off,", "not even when the lid is closed."], Hue::Text3),
        Status::Unprotected => {
            (["Not working: the computer may sleep.", "Turn it off and on again to retry."], Hue::Red)
        }
        Status::Off => (["Off: the computer sleeps, locks and turns", "the displays off as usual."], Hue::Text3),
    };
    v.push(
        drawn(WIDTH, AWAKE_TEXT, (lines, hue), move |cx| {
            let t = cx.theme();
            let s = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, hue.color(t));
            for (i, line) in lines.iter().enumerate() {
                let y = 2.0 + i as f64 * 17.0;
                cx.draw_text_in(line, &s, Rect::new(PAD, y, WIDTH - PAD, y + 17.0), Align::Start);
            }
        })
        .text(lines.join(" "), SemanticRole::Label)
        .into(),
    );
    v.push(sep());
    v.push(settings_row("Battery Settings…", "power"));
    v
}

fn clock(p: &ClockPanel) -> Vec<Element<PanelMsg>> {
    let now = p.now;
    let title = format!("{} {}", crate::clock::MONTHS[(now.month as usize + 11) % 12], now.year);
    let mut v = vec![head(&now.date_label(), None)];
    let grid = crate::clock::month_grid(now.year, now.month);
    let rows = grid.len();
    let h = 40.0 + rows as f64 * 26.0 + 6.0;
    v.push(
        drawn(WIDTH, h, (now.year, now.month, now.day), move |cx| {
            let t = cx.theme();
            let cell = (WIDTH - 2.0 * PAD) / 7.0;
            let cap = look::caption(t);
            cx.draw_text_in(&title.to_uppercase(), &cap, Rect::new(PAD, 0.0, WIDTH - PAD, 16.0), Align::Start);
            for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                let r = Rect::new(PAD + i as f64 * cell, 20.0, PAD + (i + 1) as f64 * cell, 36.0);
                let s = look::mono(10.5, typeface::WEIGHT_REGULAR, if i >= 5 { t.faint() } else { t.text3() });
                cx.draw_text_in(d, &s, r, Align::Center);
            }
            for (w, week) in grid.iter().enumerate() {
                for (i, day) in week.iter().enumerate() {
                    let Some(day) = day else { continue };
                    let r = Rect::new(
                        PAD + i as f64 * cell,
                        40.0 + w as f64 * 26.0,
                        PAD + (i + 1) as f64 * cell,
                        40.0 + (w + 1) as f64 * 26.0,
                    );
                    let today = *day == now.day;
                    let fg = if today {
                        let c = aizome_kit::draw::centered(r, 24.0, 22.0);
                        cx.canvas.fill_rect(c, t.focus());
                        t.on_focus()
                    } else if i >= 5 {
                        t.text3()
                    } else {
                        t.text()
                    };
                    let s = look::digits(
                        12.0,
                        if today { typeface::WEIGHT_SEMIBOLD } else { typeface::WEIGHT_REGULAR },
                        fg,
                    );
                    cx.draw_text_in(&day.to_string(), &s, r, Align::Center);
                }
            }
        })
        .into(),
    );
    v.push(sep());
    v.push(labelled(
        "Do Not Disturb",
        typeface::WEIGHT_REGULAR,
        Some(toggle(p.dnd).on_toggle(PanelMsg::DoNotDisturb).into()),
        ROW,
    ));
    if let Some(events) = &p.events {
        v.push(sep());
        if events.is_empty() {
            v.push(faint_row("Nothing more today or tomorrow"));
        }
        for e in events.iter().take(EVENTS) {
            v.push(event_row(e));
        }
    }
    if !p.zones.is_empty() {
        v.push(sep());
        for z in &p.zones {
            v.push(zone_row(z));
        }
    }
    v.push(sep());
    v.push(settings_row("Date & Time Settings…", "datetime"));
    v
}

/// A row that only says something, in `faint`.
fn faint_row(text: &str) -> Element<PanelMsg> {
    let s = text.to_owned();
    drawn(WIDTH, ROW, s.clone(), move |cx| {
        let t = cx.theme();
        let spec = ui(t, typeface::BODY, typeface::WEIGHT_REGULAR, t.faint());
        cx.draw_text_in(&s, &spec, Rect::new(PAD, 0.0, WIDTH - PAD, ROW), Align::Start);
    })
    .text(text.to_owned(), SemanticRole::Label)
    .into()
}

/// An event coming up: its calendar's colour, when, and what; it opens
/// Koyomi at its day.
fn event_row(e: &crate::agenda::EventRow) -> Element<PanelMsg> {
    let e2 = e.clone();
    drawn(WIDTH, ROW, (&e.when, &e.title, e.color, e.now), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let hot = cx.is_hovered();
        if hot {
            cx.canvas.fill_rect(b, t.selection());
        }
        let fg = if hot { t.on_selection() } else { t.text() };
        let dye = e2.color.map_or(t.text3(), |c| aizome_kit::theme::color(aizome_theme::Rgba::hex(c)));
        cx.canvas.fill_rect(Rect::new(PAD, (ROW - 16.0) / 2.0, PAD + 3.0, (ROW + 16.0) / 2.0), dye);
        let when_color = if hot || e2.now { fg } else { t.text3() };
        let ws =
            look::mono(11.5, if e2.now { typeface::WEIGHT_SEMIBOLD } else { typeface::WEIGHT_REGULAR }, when_color);
        cx.draw_text_in(&e2.when, &ws, Rect::new(PAD + 10.0, 0.0, PAD + 84.0, ROW), Align::Start);
        let ts = ui(t, typeface::BODY, typeface::WEIGHT_REGULAR, fg);
        cx.draw_text_in(&e2.title, &ts, Rect::new(PAD + 88.0, 0.0, b.x1 - PAD, ROW), Align::Start);
    })
    .on_click(PanelMsg::OpenDay(e.date.clone()))
    .text(format!("{} {}", e.when, e.title), SemanticRole::MenuItem)
    .key(("event", e.date.as_str(), e.when.as_str(), e.title.as_str()))
    .into()
}

/// A world clock: the place, how it stands to here, and its time; it
/// opens Tokei.
fn zone_row(z: &crate::agenda::ZoneRow) -> Element<PanelMsg> {
    let z2 = z.clone();
    drawn(WIDTH, ROW, (&z.name, &z.time, &z.note), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let hot = cx.is_hovered();
        if hot {
            cx.canvas.fill_rect(b, t.selection());
        }
        let fg = if hot { t.on_selection() } else { t.text() };
        Icon::Globe.draw(
            cx.canvas,
            Point::new(PAD, ((ROW - 16.0) / 2.0).round()),
            16.0,
            if hot { fg } else { Hue::Cyan.color(t) },
        );
        let ns = ui(t, typeface::BODY, typeface::WEIGHT_REGULAR, fg);
        cx.draw_text_in(&z2.name, &ns, Rect::new(PAD + 24.0, 0.0, b.x1 - PAD - 150.0, ROW), Align::Start);
        let ds = look::digits(12.5, typeface::WEIGHT_SEMIBOLD, fg);
        let w = cx.draw_text_in(&z2.time, &ds, Rect::new(b.x1 - PAD - 80.0, 0.0, b.x1 - PAD, ROW), Align::End);
        let ss = ui(t, typeface::SMALL, typeface::WEIGHT_REGULAR, if hot { fg } else { t.text3() });
        cx.draw_text_in(&z2.note, &ss, Rect::new(b.x1 - PAD - 160.0, 0.0, b.x1 - PAD - w - 8.0, ROW), Align::End);
    })
    .on_click(PanelMsg::OpenClocks)
    .text(format!("{} {}", z.name, z.time), SemanticRole::MenuItem)
    .key(("zone", z.name.as_str()))
    .into()
}

/// A panel's content.
pub fn view(p: &Panel) -> Element<PanelMsg> {
    let rows = match p {
        Panel::Network(n) => network(n),
        Panel::Sound(s) => sound(s),
        Panel::Bluetooth(b) => bluetooth(b),
        Panel::Display(d) => display(d),
        Panel::Awake(a) => awake(a),
        Panel::Battery(b) => battery(b),
        Panel::Clock(c) => clock(c),
    };
    let content = column(rows).width(WIDTH).padding([4.0, 0.0, 6.0, 0.0]);
    let body = around(content, [1.0, 1.0, 1.0, 1.0], Size::ZERO, "panel-frame", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::panel(cx.canvas, t, Rect::new(0.0, 0.0, b.x1, b.y1));
    });
    let keyed = crate::widgets::keys::focus_keys(body, |k| {
        matches!(k.key, aizome_kit::sidestep_ui::Key::Named(aizome_kit::sidestep_ui::NamedKey::Escape))
            .then_some(PanelMsg::Close)
    });
    // The keys widget itself takes the focus (and so Escape).
    aizome_kit::container(keyed.id("panel")).padding([0.0, SHADOW, SHADOW, 0.0]).into()
}

/// The panel's height, for its popup: the rows', its edges and shadow.
pub fn height(p: &Panel) -> f64 {
    let sep = SEP;
    let rows: f64 = match p {
        Panel::Network(n) => {
            let mut h = 0.0;
            if let Some(on) = n.wifi {
                h += HEAD;
                if on {
                    h += n.networks.len().min(8) as f64 * ROW;
                    if n.asking.is_some() {
                        h += 32.0;
                    }
                    if n.error.is_some() {
                        h += 22.0;
                    }
                }
                h += sep;
            }
            h + n.vpns.len() as f64 * ROW + ROW + sep + ROW
        }
        Panel::Sound(s) => {
            let mut h = HEAD;
            if s.output.is_some() {
                h += ROW + 4.0;
            }
            if s.outputs.len() > 1 {
                h += s.outputs.len() as f64 * ROW;
            }
            if s.input.is_some() {
                h += sep + ROW + 4.0;
            }
            if s.media.is_some() {
                h += sep + 44.0;
            }
            h + sep + ROW
        }
        Panel::Bluetooth(b) => HEAD + if b.powered { b.devices.len() as f64 * ROW } else { 0.0 } + sep + ROW,
        Panel::Display(d) => {
            HEAD + d.backlights.len() as f64 * (ROW + 4.0)
                + 2.0 * (24.0 + 30.0)
                + if d.night_light.is_some() { ROW } else { 0.0 }
                + sep
                + ROW
        }
        Panel::Awake(_) => HEAD + AWAKE_TEXT + sep + ROW,
        Panel::Battery(b) => HEAD + 34.0 + if b.modes.is_empty() { 0.0 } else { 24.0 + 30.0 } + ROW + sep + ROW,
        Panel::Clock(c) => {
            let weeks = crate::clock::month_grid(c.now.year, c.now.month).len();
            let events = c.events.as_ref().map_or(0.0, |e| sep + e.len().clamp(1, EVENTS) as f64 * ROW);
            HEAD + 40.0
                + weeks as f64 * 26.0
                + 6.0
                + sep
                + ROW
                + events
                + if c.zones.is_empty() { 0.0 } else { sep + c.zones.len() as f64 * ROW }
                + sep
                + ROW
        }
    };
    (rows + 10.0 + 2.0 + SHADOW).ceil()
}

pub fn size(p: &Panel) -> Size {
    Size::new(WIDTH + 2.0 + SHADOW, height(p))
}
