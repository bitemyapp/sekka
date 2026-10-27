//! A sidestep-ui program for the compositor's tests: what Aizome's shell
//! and apps ask of a compositor, in miniature.
//!
//! `sidestep_client toplevel` opens a window that draws green, then
//! (paced by frame callbacks on its content subsurface) magenta;
//! `panel` puts a 30pt bar along the top with an exclusive zone;
//! `lock` locks the session with a lock surface per output, then unlocks
//! after a second; `glass` puts a see-through 200 × 100pt layer surface
//! at (100, 100): its left half solid, its right half's top undrawn and
//! its bottom at half alpha;
//! `note` puts a 340 × 92pt card where the shell's notification cards go
//! (namespace `aizome-notifications`, the top right) and closes it after
//! `NOTE_MS` milliseconds (1500 by default).
//! `frost` puts a 300 × 400pt panel at (200, 120) drawn as the shell draws its
//! frosted menus in Full (namespace `aizome-menu`): a face at 90% alpha with a
//! hard shadow at half alpha 3pt right and down, clear around it.

use std::time::Duration;

use sidestep_ui::kurbo::Rect;
use sidestep_ui::layer_shell::{Anchors, Layer, LayerOptions, Margins};
use sidestep_ui::{App, Background, Canvas, Color, Cx, Handler, TimerId, WindowId, WindowOptions};

pub const GREEN: Color = Color::rgb8(0x26, 0xa2, 0x69);
pub const MAGENTA: Color = Color::rgb8(0xc0, 0x26, 0x8a);
pub const BAR: Color = Color::rgb8(0x1d, 0x2b, 0x53);
pub const LOCK: Color = Color::rgb8(0x7e, 0x25, 0x53);
pub const NOTE: Color = Color::rgb8(0xe0, 0xaf, 0x68);
pub const FACE: Color = Color::rgb8(0x24, 0x28, 0x3b);

struct Client {
    mode: String,
    flipped: bool,
    windows: Vec<WindowId>,
    flip: Option<TimerId>,
    unlock: Option<TimerId>,
    quit: Option<TimerId>,
    close: Option<TimerId>,
    tick: Option<TimerId>,
    frame: u32,
}

impl Handler for Client {
    fn launched(&mut self, cx: &mut Cx) {
        match self.mode.as_str() {
            "toplevel" => {
                let w = cx.open_window(WindowOptions::new("Sidestep test").size(320.0, 200.0));
                self.windows.push(w);
                self.flip = Some(cx.set_timer(Duration::from_millis(700)));
            }
            "animate" => {
                // A window that draws a new frame every 16ms, for measuring.
                let w = cx.open_window(WindowOptions::new("Animating").size(800.0, 500.0));
                self.windows.push(w);
                self.tick = Some(cx.set_repeating_timer(Duration::from_millis(16)));
            }
            "panel" => {
                let opts = LayerOptions::new(Layer::Top, "aizome-test-panel")
                    .anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT)
                    .exclusive_zone(30);
                let w = cx.open_window(WindowOptions::layer(opts).size(0.0, 30.0));
                self.windows.push(w);
            }
            "glass" => {
                let opts = LayerOptions::new(Layer::Top, "aizome-test-glass")
                    .anchors(Anchors::TOP | Anchors::LEFT)
                    .margins(Margins { top: 100, right: 0, bottom: 0, left: 100 });
                let w =
                    cx.open_window(WindowOptions::layer(opts).size(200.0, 100.0).background(Background::Transparent));
                self.windows.push(w);
            }
            "note" => {
                let opts = LayerOptions::new(Layer::Top, "aizome-notifications")
                    .anchors(Anchors::TOP | Anchors::RIGHT)
                    .margins(Margins { top: 32, right: 12, bottom: 0, left: 0 });
                let w = cx.open_window(WindowOptions::layer(opts).size(340.0, 92.0));
                self.windows.push(w);
                let ms = std::env::var("NOTE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
                self.close = Some(cx.set_timer(Duration::from_millis(ms)));
            }
            "frost" => {
                let opts = LayerOptions::new(Layer::Top, "aizome-menu")
                    .anchors(Anchors::TOP | Anchors::LEFT)
                    .margins(Margins { top: 120, right: 0, bottom: 0, left: 200 });
                let size = (300.0 + 3.0, 400.0 + 3.0);
                let w =
                    cx.open_window(WindowOptions::layer(opts).size(size.0, size.1).background(Background::Transparent));
                self.windows.push(w);
            }
            "lock" => cx.lock_session(),
            m => panic!("unknown mode {m}"),
        }
    }

    fn session_locked(&mut self, cx: &mut Cx) {
        for o in cx.outputs() {
            let w = cx.open_window(WindowOptions::lock(o.id));
            self.windows.push(w);
        }
        self.unlock = Some(cx.set_timer(Duration::from_millis(1500)));
    }

    fn timer(&mut self, cx: &mut Cx, timer: TimerId) {
        if Some(timer) == self.tick {
            self.frame += 1;
            for w in self.windows.clone() {
                if let Some(mut win) = cx.window(w) {
                    win.redraw();
                }
            }
        }
        if Some(timer) == self.flip {
            self.flipped = true;
            for w in self.windows.clone() {
                if let Some(mut win) = cx.window(w) {
                    win.redraw();
                }
            }
        }
        if Some(timer) == self.unlock {
            cx.unlock_session();
            for w in std::mem::take(&mut self.windows) {
                cx.close_window(w);
            }
            // Sidestep sends the unlock with its next drawing; a window to
            // draw sends it now.
            self.mode = "toplevel".into();
            let w = cx.open_window(WindowOptions::new("After the lock").size(200.0, 100.0));
            self.windows.push(w);
            self.quit = Some(cx.set_timer(Duration::from_millis(1500)));
        }
        if Some(timer) == self.close {
            // Closing the last window ends the program.
            for w in std::mem::take(&mut self.windows) {
                cx.close_window(w);
            }
        }
        if Some(timer) == self.quit {
            cx.quit();
        }
    }

    fn draw(&mut self, _cx: &mut Cx, _window: WindowId, canvas: &mut Canvas) {
        let s = canvas.size();
        if self.mode == "frost" {
            let (w, h) = (s.width - 3.0, s.height - 3.0);
            canvas.fill_rect(Rect::new(w, 3.0, s.width, s.height), Color::rgb8(0, 0, 0).with_alpha(0.5));
            canvas.fill_rect(Rect::new(3.0, h, w, s.height), Color::rgb8(0, 0, 0).with_alpha(0.5));
            canvas.fill_rect(Rect::new(0.0, 0.0, w, h), FACE.with_alpha(0.9));
            return;
        }
        if self.mode == "glass" {
            canvas.fill_rect(Rect::new(0.0, 0.0, s.width / 2.0, s.height), BAR);
            canvas.fill_rect(Rect::new(s.width / 2.0, s.height / 2.0, s.width, s.height), BAR.with_alpha(0.5));
            return;
        }
        let color = match self.mode.as_str() {
            "panel" => BAR,
            "note" => NOTE,
            "lock" => LOCK,
            "animate" => Color::rgb8((self.frame * 7 % 256) as u8, 0x40, (255 - self.frame * 3 % 256) as u8),
            _ if self.flipped => MAGENTA,
            _ => GREEN,
        };
        canvas.fill_rect(Rect::new(0.0, 0.0, s.width, s.height), color);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "toplevel".into());
    let app = App::new().app_id("org.aizome.test-client").quit_when_last_window_closes(mode != "lock");
    app.run(Client {
        mode,
        flipped: false,
        windows: Vec::new(),
        flip: None,
        unlock: None,
        quit: None,
        close: None,
        tick: None,
        frame: 0,
    })
    .expect("run");
}
