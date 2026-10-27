//! The systray's part of the shell: each panel's content from the system
//! services, its popup under the widget, and what its controls do.

use std::collections::HashMap;

use aizome_kit::prelude::*;
use aizome_kit::{MenuPlacement, WindowKind};

use super::panels::{
    self, AwakePanel, BatteryPanel, BtDevice, BtPanel, ClockPanel, DisplayPanel, NetPanel, NetRow, Panel, PanelMsg,
    SoundPanel,
};
use crate::menubar::TrayWidget;
use crate::shell::{Msg, Shell, Surface};

/// A panel showing.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPanel {
    pub window: WindowKey,
    pub widget: TrayWidget,
    pub bar: WindowKey,
    pub size: Size,
    /// The rectangle of the bar it opened under (its widget's).
    pub under: Rect,
}

/// What panels remember while open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelUi {
    /// The Wi-Fi network whose password is asked for, and what's typed.
    pub asking: Option<(String, String)>,
    pub error: Option<String>,
}

impl Shell {
    /// What a widget's panel shows now.
    pub fn panel_data(&self, w: TrayWidget) -> Panel {
        if let Some(p) = self.panels_fixture.as_ref().and_then(|f| f.get(&w)) {
            let mut p = p.clone();
            if let Panel::Network(n) = &mut p {
                n.asking = self.panel_ui.asking.clone();
                n.error = self.panel_ui.error.clone();
            }
            if let Panel::Battery(b) = &mut p {
                b.keep_awake = self.awake;
            }
            return p;
        }
        match w {
            TrayWidget::Network => {
                let mut p = NetPanel {
                    asking: self.panel_ui.asking.clone(),
                    error: self.panel_ui.error.clone(),
                    ..NetPanel::default()
                };
                if let Some(n) = &self.sys.network {
                    let s = n.snapshot();
                    if s.wifi_device().is_some() {
                        p.wifi = Some(s.wifi_enabled);
                    }
                    p.networks = s
                        .networks
                        .iter()
                        .map(|n| NetRow {
                            ssid: n.ssid.clone(),
                            bars: n.bars(),
                            secure: n.security.needs_password(),
                            connected: n.connected,
                            connecting: n.connecting,
                            known: n.known,
                        })
                        .collect();
                    use aizome_sys::network::ConnectionKind;
                    p.vpns = s
                        .connections
                        .iter()
                        .filter(|c| matches!(c.kind, ConnectionKind::Vpn | ConnectionKind::WireGuard))
                        .map(|c| (c.uuid.clone(), c.id.clone(), s.active_connection(&c.uuid).is_some()))
                        .collect();
                    p.airplane = s.airplane_mode;
                }
                Panel::Network(p)
            }
            TrayWidget::Sound => {
                let mut p = SoundPanel::default();
                if let Some(a) = &self.sys.audio {
                    let s = a.snapshot();
                    p.output = s.default_output().map(|d| (d.volume, d.muted));
                    p.input = s.default_input().map(|d| (d.volume, d.muted));
                    p.outputs = s
                        .sinks
                        .iter()
                        .map(|d| (d.name.clone(), d.description.clone(), Some(&d.name) == s.default_sink.as_ref()))
                        .collect();
                }
                p.media = self.sys.media();
                Panel::Sound(p)
            }
            TrayWidget::Bluetooth => {
                let mut p = BtPanel::default();
                if let Some(b) = &self.sys.bluetooth {
                    let s = b.snapshot();
                    p.powered = s.powered();
                    p.devices = s
                        .paired()
                        .map(|d| BtDevice {
                            path: d.path.clone(),
                            name: d.name.clone(),
                            connected: d.connected,
                            battery: d.battery,
                        })
                        .collect();
                }
                Panel::Bluetooth(p)
            }
            TrayWidget::Display => {
                let mut p = DisplayPanel {
                    theme: match self.settings.appearance.theme {
                        aizome_config::ThemeChoice::Day => 1,
                        aizome_config::ThemeChoice::Auto => 2,
                        _ => 0,
                    },
                    size: match self.settings.appearance.size {
                        aizome_config::UiSize::Small => 0,
                        aizome_config::UiSize::Medium => 1,
                        aizome_config::UiSize::Large => 2,
                    },
                    night_light: self.night.as_ref().filter(|s| !s.outputs.is_empty()).map(|s| s.enabled),
                    ..DisplayPanel::default()
                };
                if let Some(b) = &self.sys.backlight
                    && self.shows_builtin_panel()
                {
                    p.backlights = b.snapshot().devices.iter().map(|d| (d.name.clone(), d.fraction())).collect();
                }
                Panel::Display(p)
            }
            TrayWidget::Awake => Panel::Awake(AwakePanel { status: self.awake }),
            TrayWidget::Battery => {
                let mut p = BatteryPanel { keep_awake: self.awake, ..BatteryPanel::default() };
                if let Some(u) = &self.sys.upower {
                    let s = u.snapshot();
                    if let Some(d) = &s.display {
                        use aizome_sys::power::BatteryState as B;
                        p.percent = d.percentage as f32;
                        p.state = match d.state {
                            B::Charging => "Charging",
                            B::Discharging => "On battery",
                            B::FullyCharged => "Charged",
                            B::PendingCharge => "Not charging",
                            B::Empty => "Empty",
                            _ => "",
                        }
                        .into();
                        let left = match d.state {
                            B::Charging => d.time_to_full.map(|t| (t, "until full")),
                            B::Discharging => d.time_to_empty.map(|t| (t, "left")),
                            _ => None,
                        };
                        p.time = left.map(|(t, what)| {
                            let m = t.as_secs() / 60;
                            format!("{}:{:02} {what}", m / 60, m % 60)
                        });
                    }
                }
                if let Some(pp) = &self.sys.profiles {
                    let s = pp.snapshot();
                    if s.available {
                        p.modes = s.profiles.iter().map(|x| x.name.clone()).collect();
                        p.mode = p.modes.iter().position(|m| *m == s.active);
                    }
                }
                Panel::Battery(p)
            }
            TrayWidget::Clock => {
                let format = self.bar_cfg.clock;
                let today = format!("{:04}-{:02}-{:02}", self.now.year, self.now.month, self.now.day);
                let events =
                    self.agenda.events.as_ref().map(|e| crate::agenda::event_rows(e, &today, format, panels::EVENTS));
                let zones = crate::agenda::zone_rows(
                    &self.agenda.places,
                    jiff::Timestamp::now(),
                    &jiff::tz::TimeZone::system(),
                    format,
                );
                Panel::Clock(ClockPanel { now: self.now, dnd: self.notes_cfg.do_not_disturb, events, zones })
            }
        }
    }

    /// Open a widget's panel under it (or close it if it's open).
    pub fn toggle_panel(&mut self, bar: WindowKey, w: TrayWidget, rect: Rect, cx: &mut Cx<Msg>) {
        let was = self.panel.as_ref().map(|p| p.widget);
        self.close_panel(cx);
        if was == Some(w) {
            return;
        }
        self.panel_ui = PanelUi::default();
        if w == TrayWidget::Clock {
            self.refresh_agenda(cx);
        }
        let data = self.panel_data(w);
        let size = panels::size(&data);
        // Its right edge under the widget's (the compositor slides it in
        // from the output's edge if need be).
        let right = rect.x1 + 10.0;
        let anchor = Rect::new((right - panels::WIDTH).max(0.0), 0.0, right, crate::menubar::HEIGHT - 1.0);
        let mut spec = WindowSpec::new(w.name()).size(size.width, size.height);
        spec.kind = WindowKind::Popup { parent: bar, anchor, placement: MenuPlacement::Below, grab: true };
        let window = cx.open_window(spec);
        self.surfaces.insert(window, Surface::Panel);
        self.panel = Some(OpenPanel { window, widget: w, bar, size, under: rect });
        cx.focus("panel");
    }

    /// Read Koyomi's next events and Tokei's world clocks again, on a
    /// thread (the clock's panel opened, or a minute passed with it open).
    pub fn refresh_agenda(&mut self, cx: &mut Cx<Msg>) {
        if self.opts.live {
            cx.spawn(|| Msg::Agenda(crate::agenda::read()));
        }
    }

    pub fn close_panel(&mut self, cx: &mut Cx<Msg>) {
        if let Some(p) = self.panel.take() {
            cx.close_window(p.window);
            self.surfaces.remove(&p.window);
        }
    }

    /// Keep the panel sized to its content.
    pub fn sync_panel(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &self.panel else { return };
        let size = panels::size(&self.panel_data(p.widget));
        if size != p.size {
            cx.request_size(p.window, size);
            if let Some(p) = &mut self.panel {
                p.size = size;
            }
        }
    }

    pub fn panel_msg(&mut self, m: PanelMsg, cx: &mut Cx<Msg>) {
        let sys = &self.sys;
        match m {
            PanelMsg::Close => self.close_panel(cx),
            PanelMsg::Settings(pane) => {
                self.close_panel(cx);
                self.run(crate::actions::Action::Settings { pane, anchor: None }, cx);
            }
            PanelMsg::OpenDay(date) => {
                self.close_panel(cx);
                self.run(
                    crate::actions::Action::Spawn { argv: vec!["aizome-koyomi".into(), "--show".into(), date] },
                    cx,
                );
            }
            PanelMsg::OpenClocks => {
                self.close_panel(cx);
                self.run(crate::actions::Action::Launch { app_id: "org.aizome.Tokei".into() }, cx);
            }
            PanelMsg::Wifi(on) => {
                if let Some(n) = &sys.network {
                    let _ = n.set_wifi_enabled(on);
                }
            }
            PanelMsg::Airplane(on) => {
                if let Some(n) = &sys.network {
                    let _ = n.set_airplane_mode(on);
                }
            }
            PanelMsg::Vpn(uuid, on) => {
                if let Some(n) = &sys.network {
                    let _ = if on { n.activate(&uuid) } else { n.deactivate(&uuid) };
                }
            }
            PanelMsg::Network(ssid) => {
                let row = match self.panel_data(TrayWidget::Network) {
                    Panel::Network(p) => p.networks.into_iter().find(|n| n.ssid == ssid),
                    _ => None,
                };
                let Some(row) = row else { return };
                if row.connected {
                    return;
                }
                if row.secure && !row.known {
                    self.panel_ui.asking = Some((ssid, String::new()));
                    self.panel_ui.error = None;
                    cx.focus("wifi-password");
                } else if let Some(n) = &sys.network {
                    let s = n.snapshot();
                    if let Some(net) = s.network(&ssid) {
                        let _ = n.connect(net, None);
                    }
                }
            }
            PanelMsg::Password(p) => {
                if let Some((_, pw)) = &mut self.panel_ui.asking {
                    *pw = p;
                }
            }
            PanelMsg::Join => {
                let Some((ssid, pw)) = self.panel_ui.asking.take() else { return };
                if let Some(n) = &sys.network {
                    let s = n.snapshot();
                    if let Some(net) = s.network(&ssid) {
                        let _ = n.connect(net, Some(&pw));
                    }
                }
            }
            PanelMsg::Volume(v) => {
                if let Some(a) = &sys.audio
                    && let Some(d) = a.snapshot().default_output()
                {
                    let _ = a.set_sink_volume(d.index, v);
                }
            }
            PanelMsg::VolumeSet => {
                let muted = sys.audio.as_ref().and_then(|a| a.snapshot().default_output().map(|d| d.muted));
                self.volume_feedback(muted.unwrap_or(false), cx);
            }
            PanelMsg::Mute => {
                if let Some(a) = &sys.audio {
                    let _ = a.toggle_mute();
                }
            }
            PanelMsg::Output(name) => {
                if let Some(a) = &sys.audio {
                    let _ = a.set_default_sink(&name);
                }
            }
            PanelMsg::InputVolume(v) => {
                if let Some(a) = &sys.audio
                    && let Some(d) = a.snapshot().default_input()
                {
                    let _ = a.set_source_volume(d.index, v);
                }
            }
            PanelMsg::InputMute => {
                if let Some(a) = &sys.audio {
                    let _ = a.toggle_microphone_mute();
                }
            }
            PanelMsg::Media(verb) => sys.media_control(verb),
            PanelMsg::Bluetooth(on) => {
                if let Some(b) = &sys.bluetooth {
                    let s = b.snapshot();
                    if let Some(a) = s.default_adapter() {
                        let _ = b.set_powered(&a.path, on);
                    }
                }
            }
            PanelMsg::Device(path, connect) => {
                if let Some(b) = &sys.bluetooth {
                    let _ = if connect { b.connect(&path) } else { b.disconnect(&path) };
                }
            }
            PanelMsg::Brightness(name, v) => {
                if let Some(b) = &sys.backlight {
                    let _ = b.set_fraction(&name, v);
                }
            }
            PanelMsg::Theme(i) => {
                let choice = [
                    aizome_config::ThemeChoice::Night,
                    aizome_config::ThemeChoice::Day,
                    aizome_config::ThemeChoice::Auto,
                ][i.min(2)];
                self.settings.appearance.theme = choice;
                self.save_appearance();
            }
            PanelMsg::Size(i) => {
                self.settings.appearance.size =
                    [aizome_config::UiSize::Small, aizome_config::UiSize::Medium, aizome_config::UiSize::Large]
                        [i.min(2)];
                self.save_appearance();
            }
            PanelMsg::KeepAwake(on) => {
                if (self.awake != aizome_awake::Status::Off) != on {
                    self.set_keep_awake(on, cx);
                }
            }
            PanelMsg::DoNotDisturb(on) => {
                if self.notes_cfg.do_not_disturb != on {
                    self.run(crate::actions::Action::ToggleDoNotDisturb, cx);
                }
            }
            PanelMsg::NightLight(on) => {
                // Now, through the compositor, and kept in the settings.
                if let Some(s) = &mut self.night {
                    s.enabled = on;
                }
                self.settings.night_light.enabled = on;
                if self.opts.live {
                    self.ipc.send(aizome_ipc::Request::NightLight { enabled: Some(on), temperature: None });
                    if let Err(e) = self.settings.save() {
                        log::warn!("saving Night Light: {e}");
                    }
                }
            }
            PanelMsg::PowerMode(mode) => {
                if let Some(p) = &sys.profiles {
                    let _ = p.set_active(&mode);
                }
            }
        }
    }

    /// Save `[appearance]` and have everything follow it.
    pub fn save_appearance(&mut self) {
        if self.opts.live {
            if let Err(e) = self.settings.save() {
                log::warn!("saving the appearance: {e}");
            }
            self.ipc.send(aizome_ipc::Request::ReloadConfig);
        }
        let day = self.settings.theme_now() == aizome_theme::ThemeName::Day;
        self.theme_changed(day);
    }
}

/// Fixed panels, for tests and pictures.
pub type Fixture = HashMap<TrayWidget, Panel>;
