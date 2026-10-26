//! The field that records a shortcut: it takes the keyboard, shows the
//! modifiers held as they're pressed, and sends the chord typed. Escape
//! alone cancels; Backspace alone leaves the action without a shortcut.

use aizome_kit::FontSpec;
use aizome_kit::draw;
use aizome_kit::element::{Element, MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Size};
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};

use super::keys::{self, Chord};

pub fn recorder<Msg>(on_chord: impl Fn(Chord) -> Msg + 'static, cancel: Msg, clear: Msg) -> Element<Msg>
where
    Msg: Clone + Send + 'static,
{
    Element::new(
        Recorder {
            on_chord: MsgMap::new(on_chord),
            cancel: MsgFn::new(cancel),
            clear: MsgFn::new(clear),
            held: Modifiers::NONE,
        },
        Vec::new(),
    )
}

struct Recorder {
    on_chord: MsgMap<Chord>,
    cancel: MsgFn,
    clear: MsgFn,
    held: Modifiers,
}

/// What the field shows while the keys are held.
#[derive(Default)]
struct Held(Modifiers);

impl Widget for Recorder {
    fn name(&self) -> &'static str {
        "shortcut-recorder"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        self.held = cx.state::<Held>().0;
        bc.constrain(Size::new(180.0, 24.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        draw::sunken(cx.canvas, t, r, t.sunk(), if cx.is_focused() { t.focus() } else { t.line2() });
        if cx.is_focused() {
            draw::left(cx.canvas, r, t.focus(), 2.0);
        }
        let m = self.held;
        let mut s = String::new();
        for (bit, g) in
            [(Modifiers::CONTROL, "⌃"), (Modifiers::ALT, "⌥"), (Modifiers::SHIFT, "⇧"), (Modifiers::SUPER, "❖")]
        {
            if m.contains(bit) {
                s.push_str(g);
            }
        }
        let (label, color) =
            if s.is_empty() { ("Type the shortcut…".to_owned(), t.text3()) } else { (format!("{s} …"), t.text()) };
        let spec = TextSpec::new(FontSpec::ui(12.0), color);
        cx.draw_text_in(&label, &spec, r.inset((-8.0, 0.0)), Align::Start);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&format!("{:?}", self.held));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                cx.request_focus();
                Status::Handled
            }
            Event::Key(k) => {
                let held = k.modifiers;
                if cx.state::<Held>().0 != held {
                    cx.state::<Held>().0 = held;
                    cx.invalidate();
                    cx.relayout();
                }
                if !k.pressed || k.repeat {
                    return Status::Handled;
                }
                let bare = !held.contains(Modifiers::CONTROL)
                    && !held.contains(Modifiers::ALT)
                    && !held.contains(Modifiers::SUPER)
                    && !held.contains(Modifiers::SHIFT);
                match &k.key {
                    Key::Named(NamedKey::Escape) if bare => {
                        let m = self.cancel.clone();
                        cx.emit(&m);
                    }
                    Key::Named(NamedKey::Backspace) | Key::Named(NamedKey::Delete) if bare => {
                        let m = self.clear.clone();
                        cx.emit(&m);
                    }
                    _ => {
                        if let Some(chord) = keys::from_key(k) {
                            let map = self.on_chord.clone();
                            cx.emit(&MsgFn::from_fn(move || map.make(chord.clone())));
                        }
                    }
                }
                Status::Handled
            }
            Event::Focus(false) => {
                cx.state::<Held>().0 = Modifiers::NONE;
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::TextInput, label: Some("New shortcut".into()), ..Default::default() })
    }
}
