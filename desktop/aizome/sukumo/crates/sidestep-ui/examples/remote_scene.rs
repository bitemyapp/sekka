//! A real Sukumo window for retained-rendering integration and damage tests.
use sidestep_ui::kurbo::{Circle, Point, Rect, Size};
use sidestep_ui::*;
use std::time::Duration;
#[derive(Default)]
struct Demo {
    root: Option<WindowId>,
    popup: Option<WindowId>,
    green: bool,
    tick: u32,
    animate: bool,
    benchmark: bool,
}
impl Handler for Demo {
    fn launched(&mut self, cx: &mut Cx) {
        let w = cx.open_window(WindowOptions::new("Aizome retained scene fixture").size(1280.0, 800.0));
        self.root = Some(w);
        cx.window(w).unwrap().set_fullscreen(true);
        self.benchmark = std::env::var_os("AIZOME_SCENE_BENCH").is_some();
        self.animate = self.benchmark;
        cx.set_repeating_timer(Duration::from_nanos(1_000_000_000 / 60));
    }
    fn draw(&mut self, _: &mut Cx, window: WindowId, c: &mut Canvas) {
        let all = Rect::from_origin_size(Point::ZERO, c.size());
        if Some(window) == self.popup {
            c.fill_rect(all, Color::hex(0xe7a543));
            return;
        }
        c.fill_rect(all, Color::hex(0x142338));
        let title = TextStyle::new(Font::system(28.0), Color::WHITE);
        let text = TextLayout::new("Aizome native drawing — text, paths and damage", &title);
        c.draw_text(&text, Point::new(48.0, 40.0));
        c.fill_rect(
            Rect::new(48.0, 130.0, 420.0, 270.0),
            if self.green { Color::hex(0x29c56a) } else { Color::hex(0x287ce8) },
        );
        c.fill(&Circle::new((530.0, 200.0), 60.0), Color::rgba8(240, 80, 50, 190));
        let label = TextStyle::new(Font::system(18.0), Color::WHITE);
        c.draw_text(
            &TextLayout::new("Click the rectangle; P opens a popup; A animates", &label),
            Point::new(48.0, 320.0),
        );
        let x = 48.0 + f64::from(self.tick % 900);
        c.fill_rect(Rect::new(x, 400.0, x + 20.0, 440.0), Color::hex(0xf2d97a));
        if self.benchmark {
            let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
            let stamp = (now.tv_sec as u64 * 1000 + now.tv_nsec as u64 / 1_000_000) as u32;
            let fid = self.tick as u16;
            let checksum = stamp.to_le_bytes().into_iter().chain(fid.to_le_bytes()).fold(0xa5, |a, b| a ^ b);
            let bits = u64::from(stamp) | (u64::from(fid) << 32) | (u64::from(checksum) << 48);
            for bit in 0..56 {
                let x = 16.0 + f64::from(bit) * 8.0;
                c.fill_rect(
                    Rect::new(x, 16.0, x + 8.0, 32.0),
                    if (bits >> bit) & 1 != 0 { Color::WHITE } else { Color::BLACK },
                );
            }
        }
    }
    fn window_event(&mut self, cx: &mut Cx, window: WindowId, event: WindowEvent) {
        if Some(window) != self.root {
            return;
        }
        match event {
            WindowEvent::PointerButton { pressed: true, .. } => {
                self.green = !self.green;
                cx.window(window).unwrap().invalidate(Rect::new(48.0, 130.0, 420.0, 270.0));
                println!("button green={}", self.green);
            }
            WindowEvent::Key(k) if k.pressed => {
                if k.text.as_deref() == Some("p") {
                    if let Some(p) = self.popup.take() {
                        cx.close_window(p);
                    } else {
                        self.popup = Some(cx.open_window(WindowOptions::popup(
                            window,
                            Rect::new(650.0, 160.0, 670.0, 180.0),
                            Size::new(180.0, 120.0),
                            false,
                        )));
                    }
                }
                if k.text.as_deref() == Some("a") {
                    self.animate = !self.animate;
                }
            }
            _ => {}
        }
    }
    fn timer(&mut self, cx: &mut Cx, _: TimerId) {
        if self.animate {
            self.tick += 4;
            if let Some(w) = self.root.and_then(|w| cx.window(w)) {
                let mut w = w;
                w.invalidate(Rect::new(48.0, 400.0, 970.0, 440.0));
                if self.benchmark {
                    w.invalidate(Rect::new(16.0, 16.0, 464.0, 32.0));
                }
            }
        }
    }
}
fn main() {
    App::new().app_id("org.aizome.RemoteSceneFixture").run(Demo::default()).expect("Wayland compositor");
}
