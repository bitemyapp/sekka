//! The on-screen display: volume, mute, brightness and the keyboard's
//! backlight keys change the value at once and show a `raised` square at
//! the bottom centre of the focused output — a 32pt glyph and a 16-segment
//! level bar — for 1.2 s.

use std::time::Duration;

use aizome_kit::element::Element;
use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins};

use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::drawn;

pub const SIDE: f64 = 144.0;
pub const SHADOW: f64 = 3.0;
pub const SEGMENTS: usize = 16;
/// How long it shows after the last key.
pub const SHOWS: Duration = Duration::from_millis(1200);

/// What the OSD shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OsdKind {
    Volume,
    Microphone,
    Brightness,
    Keyboard,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Osd {
    pub kind: OsdKind,
    /// 0–1.
    pub value: f32,
    pub muted: bool,
}

impl Osd {
    pub fn glyph(&self) -> Icon {
        match self.kind {
            OsdKind::Volume => crate::menubar::sound_icon(self.value, self.muted),
            OsdKind::Microphone => {
                if self.muted {
                    Icon::MicOff
                } else {
                    Icon::Mic
                }
            }
            OsdKind::Brightness => Icon::Sun,
            OsdKind::Keyboard => Icon::Keyboard,
        }
    }

    /// Segments lit: a value's sixteenths, rounded (none when muted).
    pub fn lit(&self) -> usize {
        if self.muted { 0 } else { (self.value.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize }
    }
}

/// The OSD showing, and its surface.
#[derive(Clone, Debug, PartialEq)]
pub struct OsdState {
    pub window: WindowKey,
    pub osd: Osd,
    /// Bumped on each key, so only the last key's timer hides it.
    pub generation: u64,
}

pub fn view(o: &Osd) -> Element<Msg> {
    let o = *o;
    let label = format!("{:?} {:.0}%", o.kind, o.value * 100.0);
    drawn(SIDE + SHADOW, SIDE + SHADOW, (o.lit(), o.muted, o.glyph()), move |cx| {
        let t = cx.theme();
        let r = Rect::new(0.0, 0.0, SIDE, SIDE);
        crate::look::panel(cx.canvas, t, r);
        o.glyph().draw(
            cx.canvas,
            Point::new((SIDE - 32.0) / 2.0, 36.0),
            32.0,
            if o.muted { t.text3() } else { t.text() },
        );
        let (seg, gap) = (6.0, 1.0);
        let total = SEGMENTS as f64 * seg + (SEGMENTS - 1) as f64 * gap;
        let x0 = ((SIDE - total) / 2.0).round();
        let lit = o.lit();
        for i in 0..SEGMENTS {
            let x = x0 + i as f64 * (seg + gap);
            let c = if i < lit { t.text() } else { t.line2() };
            cx.canvas.fill_rect(Rect::new(x, 100.0, x + seg, 108.0), c);
        }
    })
    .text(label, SemanticRole::ProgressIndicator)
    .into()
}

impl Shell {
    /// Show the OSD (opening its surface on the focused output, or
    /// changing it), hidden again after [`SHOWS`].
    pub fn show_osd(&mut self, osd: Osd, cx: &mut Cx<Msg>) {
        let generation = self.osd.as_ref().map_or(0, |o| o.generation) + 1;
        match &mut self.osd {
            Some(s) => {
                s.osd = osd;
                s.generation = generation;
            }
            None => {
                let Some(output) = self.focused_screen().map(|s| s.output.id) else { return };
                let opts = LayerOptions::new(Layer::Overlay, "aizome-osd")
                    .anchors(Anchors::BOTTOM)
                    .margins(Margins::new(0, 0, 96, 0))
                    .keyboard(KeyboardInteractivity::None)
                    .output(output);
                let w = cx.open_window(layer(opts, SIDE + SHADOW, SIDE + SHADOW, "OSD"));
                // It takes no clicks.
                cx.set_input_region(w, Some(Vec::new()));
                self.surfaces.insert(w, Surface::Osd);
                self.osd = Some(OsdState { window: w, osd, generation });
            }
        }
        cx.after(SHOWS, Msg::OsdHide(generation));
    }

    pub fn hide_osd(&mut self, generation: u64, cx: &mut Cx<Msg>) {
        if self.osd.as_ref().is_some_and(|o| o.generation == generation)
            && let Some(o) = self.osd.take()
        {
            cx.close_window(o.window);
            self.surfaces.remove(&o.window);
        }
    }

    /// A media key from the compositor: change the value now, and show it.
    pub fn media_key(&mut self, action: &str, cx: &mut Cx<Msg>) -> bool {
        const STEP: f32 = 1.0 / SEGMENTS as f32;
        let osd = match action {
            "volume-up" | "volume-down" | "mute" => {
                let (mut v, mut m) = self
                    .sys
                    .audio
                    .as_ref()
                    .and_then(|a| a.snapshot().default_output().map(|d| (d.volume, d.muted)))
                    .unwrap_or((0.5, false));
                if let Some(a) = &self.sys.audio {
                    match action {
                        "volume-up" => {
                            let _ = a.step_volume(STEP);
                            v = (v + STEP).min(1.0);
                            m = false;
                        }
                        "volume-down" => {
                            let _ = a.step_volume(-STEP);
                            v = (v - STEP).max(0.0);
                        }
                        _ => {
                            let _ = a.toggle_mute();
                            m = !m;
                        }
                    }
                }
                Osd { kind: OsdKind::Volume, value: v, muted: m }
            }
            "mic-mute" => {
                let m = self
                    .sys
                    .audio
                    .as_ref()
                    .and_then(|a| a.snapshot().default_input().map(|d| d.muted))
                    .unwrap_or(false);
                if let Some(a) = &self.sys.audio {
                    let _ = a.toggle_microphone_mute();
                }
                Osd { kind: OsdKind::Microphone, value: 1.0, muted: !m }
            }
            "brightness-up" | "brightness-down" => {
                // No built-in panel shows: the backlight isn't this session's.
                if !self.shows_builtin_panel() {
                    return true;
                }
                let v = if let Some(b) = &self.sys.backlight {
                    let task = b.step(if action == "brightness-up" { 0.05 } else { -0.05 });
                    // Missing hardware or logind cannot change the panel.
                    // Do not invent a level for a refused request.
                    if matches!(task.take(), Some(Err(_))) {
                        return true;
                    }
                    let Some(v) = b.snapshot().primary().map(|d| d.fraction()) else {
                        return true;
                    };
                    // step() updates the snapshot with the rounded hardware
                    // level, including the minimum and the 5% grid.
                    v
                } else {
                    // The sample shell has no system services.
                    0.5
                };
                Osd { kind: OsdKind::Brightness, value: v, muted: false }
            }
            "kbd-brightness-up" | "kbd-brightness-down" | "kbd-brightness" => {
                if let Some(u) = &self.sys.upower {
                    let _ = u.cycle_keyboard_brightness();
                }
                let level = self
                    .sys
                    .upower
                    .as_ref()
                    .and_then(|u| {
                        u.snapshot().keyboard_backlights.first().map(|k| {
                            if k.max_brightness > 0 { k.brightness as f32 / k.max_brightness as f32 } else { 0.0 }
                        })
                    })
                    .unwrap_or(0.5);
                Osd { kind: OsdKind::Keyboard, value: level, muted: false }
            }
            _ => return false,
        };
        if matches!(action, "volume-up" | "volume-down") {
            // Heard at the new volume, if Sound asks for feedback.
            self.volume_feedback(osd.muted, cx);
        }
        self.show_osd(osd, cx);
        true
    }
}

pub fn size() -> Size {
    Size::new(SIDE + SHADOW, SIDE + SHADOW)
}

/// What `aizome-shell msg osd` names.
pub fn kind_named(s: &str) -> Option<OsdKind> {
    Some(match s {
        "volume" | "mute" => OsdKind::Volume,
        "mic" | "microphone" => OsdKind::Microphone,
        "brightness" => OsdKind::Brightness,
        "keyboard" => OsdKind::Keyboard,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_light_by_sixteenths() {
        assert_eq!(Osd { kind: OsdKind::Volume, value: 0.5, muted: false }.lit(), 8);
        assert_eq!(Osd { kind: OsdKind::Volume, value: 1.0, muted: false }.lit(), 16);
        assert_eq!(Osd { kind: OsdKind::Volume, value: 0.5, muted: true }.lit(), 0);
        assert_eq!(Osd { kind: OsdKind::Volume, value: 0.5, muted: true }.glyph(), Icon::VolumeX);
    }
}
