//! The on-screen display: volume, mute, brightness and the keyboard's
//! backlight keys change the value at once and show a `raised` square at
//! the bottom centre of the focused output (for brightness, of the display
//! whose brightness changed) — a 32pt glyph and a 16-segment level bar —
//! for 1.2 s.

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
    /// The output it shows on.
    pub output: u32,
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
    /// Show the OSD on the focused output, hidden again after [`SHOWS`].
    pub fn show_osd(&mut self, osd: Osd, cx: &mut Cx<Msg>) {
        let output = self.focused_screen().map(|s| s.output.id);
        self.show_osd_on(osd, output, cx);
    }

    /// Show the OSD on `output` (opening its surface there, or changing
    /// it), hidden again after [`SHOWS`].
    pub fn show_osd_on(&mut self, osd: Osd, output: Option<u32>, cx: &mut Cx<Msg>) {
        let Some(output) = output else { return };
        let generation = self.osd.as_ref().map_or(0, |o| o.generation) + 1;
        // Showing on another output: it moves there.
        if let Some(o) = self.osd.take_if(|o| o.output != output) {
            cx.close_window(o.window);
            self.surfaces.remove(&o.window);
        }
        match &mut self.osd {
            Some(s) => {
                s.osd = osd;
                s.generation = generation;
            }
            None => {
                let opts = LayerOptions::new(Layer::Overlay, "aizome-osd")
                    .anchors(Anchors::BOTTOM)
                    .margins(Margins::new(0, 0, 96, 0))
                    .keyboard(KeyboardInteractivity::None)
                    .output(output);
                let w = cx.open_window(layer(opts, SIDE + SHADOW, SIDE + SHADOW, "OSD"));
                // It takes no clicks.
                cx.set_input_region(w, Some(Vec::new()));
                self.surfaces.insert(w, Surface::Osd);
                self.osd = Some(OsdState { window: w, output, osd, generation });
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
    /// `output` is the display under the pointer, as the compositor says
    /// with the key.
    pub fn media_key(&mut self, action: &str, output: Option<&str>, cx: &mut Cx<Msg>) -> bool {
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
                let up = action == "brightness-up";
                // An external monitor's own brightness, where it can be set:
                // the one under the pointer, else the focused one.
                let looked_at = output.map(str::to_owned).or_else(|| self.focused_screen().map(|s| s.name()));
                if let Some(name) = looked_at
                    && let Some(m) = &self.sys.monitors
                    && let Some((id, connectors)) = m.snapshot().on(&name).map(|x| (x.id.clone(), x.connectors.clone()))
                {
                    let _ = m.step(&id, if up { 0.05 } else { -0.05 });
                    let value = m.snapshot().get(&id).map_or(0.5, |x| x.fraction());
                    // On its first connector that shows: a tiled monitor's main
                    // tile, not a hidden one the pointer may be over.
                    let on = connectors.iter().find_map(|c| self.screen_named(c)).map(|s| s.output.id);
                    self.show_osd_on(Osd { kind: OsdKind::Brightness, value, muted: false }, on, cx);
                    return true;
                }
                // Else the built-in panel's, as before Aizome set monitors;
                // with none showing, the backlight isn't this session's.
                if !self.shows_builtin_panel() {
                    return true;
                }
                let mut v = self
                    .sys
                    .backlight
                    .as_ref()
                    .and_then(|b| b.snapshot().primary().map(|d| d.fraction()))
                    .unwrap_or(0.5);
                if let Some(b) = &self.sys.backlight {
                    let _ = b.step(if up { 0.05 } else { -0.05 });
                    v = (v + if up { 0.05 } else { -0.05 }).clamp(0.0, 1.0);
                }
                // On the panel, whose brightness it is.
                let on = self.screens.iter().find(|s| crate::shell::builtin_panel(&s.name())).map(|s| s.output.id);
                self.show_osd_on(Osd { kind: OsdKind::Brightness, value: v, muted: false }, on, cx);
                return true;
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
