//! Hidden windows and windows drawn ahead (`Cx::hide_window`,
//! `Cx::show_window`, `Cx::prepare_window`): a hidden window is viewed
//! only while it's drawn ahead, which happens in bands at idle; its
//! widgets' ticks wait (a caret doesn't blink unseen) and its focus
//! leaves it; shown from its picture, its first frame draws only what
//! changed since; shown without one, it draws whole.

use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::testing::{Harness, difference};
use aizome_kit::{KeyboardInteractivity, Layer, LayerOptions};

#[derive(Default)]
struct Panel {
    window: Option<WindowKey>,
    text: String,
    views: std::cell::Cell<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Prepare(f64),
    Show,
    Hide,
    Text(String),
    Nothing,
}

impl Program for Panel {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.HiddenTest", "Hidden", "Hidden")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let opts = LayerOptions::new(Layer::Overlay, "panel").keyboard(KeyboardInteractivity::Exclusive);
        self.window = Some(cx.open_window(WindowSpec::layer(opts, 600.0, 400.0).hidden()));
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        let w = self.window.expect("the panel");
        match msg {
            Msg::Prepare(scale) => {
                assert!(cx.prepare_window(w, scale));
                cx.focus("field");
            }
            Msg::Show => cx.show_window(w),
            Msg::Hide => cx.hide_window(w),
            Msg::Text(t) => self.text = t,
            Msg::Nothing => {}
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        self.views.set(self.views.get() + 1);
        container(
            aizome_kit::column![
                text("Drawn ahead").size(20.0),
                Element::from(text_field(&self.text).on_change(Msg::Text)).id("field"),
                text(format!("{} characters", self.text.len())),
            ]
            .spacing(12.0),
        )
        .padding(16.0)
        .floating()
        .into()
    }
}

#[test]
fn a_window_drawn_ahead_shows_its_picture_and_draws_only_what_changed() {
    let mut h = Harness::new(Panel::default());
    let w = h.window();
    assert!(!h.is_shown(w));
    let views = h.program().views.get();
    h.send(Msg::Nothing);
    assert_eq!(h.program().views.get(), views, "hidden, it isn't viewed");

    // Drawn ahead: viewed now, drawn at idle in bands, which put together
    // are the window drawn whole, pixel for pixel.
    h.send(Msg::Prepare(2.0));
    assert!(h.picture(w).is_none(), "drawn at idle, not at once");
    h.advance(Duration::ZERO);
    let (pw, ph, drawn) = h.picture(w).expect("drawn ahead");
    let whole = h.render(w, 2.0);
    assert_eq!(whole.pixel_size(), (pw, ph));
    assert_eq!(difference(&drawn, &whole.to_rgba8().expect("pixels"), 0), 0.0);
    let bands = h.take_drawn_ahead(w);
    assert!(bands.len() > 1, "{bands:?}");
    let _ = h.check_frame(w);

    // Hidden, the focused field's caret doesn't blink: nothing is drawn.
    h.advance(Duration::from_secs(3));
    assert!(h.take_drawn_ahead(w).is_empty(), "drawn again while hidden");

    // Shown: the first frame is the picture, nothing drawn over it; the
    // caret blinks again from then on.
    h.send(Msg::Show);
    assert!(h.is_shown(w));
    let first = h.check_frame(w);
    assert_eq!((first.differs, first.damaged), (0.0, 0.0), "{first:?}");
    h.advance(Duration::from_millis(600));
    assert!(!h.take_damage(w).is_empty(), "the caret blinks once shown");

    // Hidden, the focus leaves it; shown again without a picture, it draws
    // whole, and the focus comes back.
    h.send(Msg::Hide);
    assert_eq!(h.focused_name(w), None);
    h.send(Msg::Text("changed while hidden".into()));
    h.send(Msg::Show);
    assert_eq!(h.focused_name(w).as_deref(), Some("field"));
    let again = h.check_frame(w);
    assert_eq!(again.differs, 0.0);
    assert!(again.damaged >= 600.0 * 400.0, "drawn whole: {again:?}");
}
