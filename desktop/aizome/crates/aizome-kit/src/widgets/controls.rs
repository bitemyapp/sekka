//! Controls: buttons, switches, checkboxes, radios, sliders, steppers,
//! segmented controls, pop-up buttons, progress, meters, badges, keycaps
//! and images.
//!
//! Every control is at least 24pt tall to the pointer (a toolbar's 20pt
//! buttons sit in a 24pt target), square-cornered, edged in 1pt, and
//! reachable from the keyboard: Tab focuses it, Space or Return works it,
//! arrows move within it.

use std::marker::PhantomData;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use sidestep_ui::kurbo::Circle;
use sidestep_ui::{Color, Cursor, Image, ImageOptions, Key, NamedKey, PointerButton};

use crate::draw;
use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::hash::PaintHasher;
use crate::icons::Icon;
use crate::layout::{Align, Constraints, Length, Point, Rect, Size};
use crate::menu::{ActionSpec, ItemSpec, MenuSpec};
use crate::text::TextSpec;
use crate::theme::{FontSpec, Role, Theme};
use crate::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};

fn ui(_theme: &Theme, size: f64, weight: f32, color: Color) -> TextSpec {
    let mut f = FontSpec::ui(size);
    f.weight = weight;
    TextSpec::new(f, color)
}

fn is_key(ev: &Event, named: NamedKey) -> bool {
    matches!(ev, Event::Key(k) if k.pressed && k.key == Key::Named(named))
}

// Buttons.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum ButtonStyle {
    #[default]
    Normal,
    Primary,
    /// No face until hovered: a toolbar's B, I, </>.
    Plain,
}

/// A push button: `button("Save").primary().on_press(Msg::Save)`.
pub struct Button<Msg> {
    pub(crate) common: Common,
    label: Option<Arc<str>>,
    icon: Option<Icon>,
    on_press: Option<MsgFn>,
    style: ButtonStyle,
    destructive: bool,
    enabled: bool,
    default: bool,
    cancel: bool,
    compact: bool,
    selected: bool,
    mono: bool,
    weight: Option<f32>,
    italic: bool,
    wrap_label: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Button);

pub fn button<Msg>(label: impl Into<Arc<str>>) -> Button<Msg> {
    Button {
        common: Common::default(),
        label: Some(label.into()),
        icon: None,
        on_press: None,
        style: ButtonStyle::Normal,
        destructive: false,
        enabled: true,
        default: false,
        cancel: false,
        compact: false,
        selected: false,
        mono: false,
        weight: None,
        italic: false,
        wrap_label: false,
        _m: PhantomData,
    }
}

/// A button showing only an icon (its tooltip or label says what it is).
pub fn icon_button<Msg>(icon: Icon) -> Button<Msg> {
    Button { label: None, icon: Some(icon), style: ButtonStyle::Plain, ..button("") }
}

impl<Msg: Clone + Send + 'static> Button<Msg> {
    pub fn on_press(mut self, msg: Msg) -> Self {
        self.on_press = Some(MsgFn::new(msg));
        self
    }

    /// Sends `msg` if `Some`, else is disabled.
    pub fn on_press_maybe(mut self, msg: Option<Msg>) -> Self {
        self.enabled &= msg.is_some();
        self.on_press = msg.map(MsgFn::new);
        self
    }
}

impl<Msg> Button<Msg> {
    /// Filled with the selection colour: the one to press.
    pub fn primary(mut self) -> Self {
        self.style = ButtonStyle::Primary;
        self
    }

    /// Does something that can't be undone: red.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// No face until the pointer is over it (toolbars).
    pub fn plain(mut self) -> Self {
        self.style = ButtonStyle::Plain;
        self
    }

    /// 20pt tall, 12pt text: a toolbar's.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// A toolbar's button: compact and plain.
    pub fn toolbar(self) -> Self {
        self.compact().plain()
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Answers Return in its sheet (and is drawn primary).
    pub fn default(mut self) -> Self {
        self.default = true;
        self.style = ButtonStyle::Primary;
        self
    }

    /// Answers Escape in its sheet.
    pub fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }

    /// Shown pressed in (a toolbar's Bold while the selection is bold).
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    /// The label in IBM Plex Mono.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn weight(mut self, w: f32) -> Self {
        self.weight = Some(w);
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Alert actions must show their complete label even in a narrow sheet.
    pub(crate) fn wrap_label(mut self) -> Self {
        self.wrap_label = true;
        self
    }
}

impl<Msg> From<Button<Msg>> for Element<Msg> {
    fn from(b: Button<Msg>) -> Element<Msg> {
        let w = ButtonWidget {
            label: b.label,
            icon: b.icon,
            on_press: b.on_press,
            style: b.style,
            destructive: b.destructive,
            enabled: b.enabled,
            default: b.default,
            cancel: b.cancel,
            compact: b.compact,
            selected: b.selected,
            mono: b.mono,
            weight: b.weight,
            italic: b.italic,
            wrap_label: b.wrap_label,
        };
        Element::from_raw(RawElement::new(w, b.common, Vec::new()))
    }
}

struct ButtonWidget {
    label: Option<Arc<str>>,
    icon: Option<Icon>,
    on_press: Option<MsgFn>,
    style: ButtonStyle,
    destructive: bool,
    enabled: bool,
    default: bool,
    cancel: bool,
    compact: bool,
    selected: bool,
    mono: bool,
    weight: Option<f32>,
    italic: bool,
    wrap_label: bool,
}

impl ButtonWidget {
    fn label_spec(&self, t: &Theme, color: Color) -> TextSpec {
        let m = &t.metrics;
        let size = if self.compact { m.small + 1.0 } else { m.body };
        let weight = self.weight.unwrap_or(if self.style == ButtonStyle::Primary { 600.0 } else { 500.0 });
        let mut f = if self.mono { FontSpec::mono(size) } else { FontSpec::ui(size) };
        f.weight = weight;
        f.italic = self.italic;
        let mut spec = TextSpec::new(f, color);
        if self.wrap_label {
            spec.wrap = sidestep_ui::Wrap::Words;
        }
        spec
    }

    fn icon_size(&self, t: &Theme) -> f64 {
        if self.compact { 14.0 } else { t.metrics.icon }
    }

    /// The face: the whole target, or 20pt of it when compact.
    fn face(&self, t: &Theme, size: Size) -> Rect {
        let r = Rect::from_origin_size(Point::ZERO, size);
        if self.compact {
            let h = t.metrics.control_compact;
            Rect::new(r.x0, ((r.height() - h) / 2.0).round(), r.x1, ((r.height() - h) / 2.0).round() + h)
        } else {
            r
        }
    }

    fn usable(&self) -> bool {
        self.enabled && self.on_press.is_some()
    }
}

/// A button's face for its state. Returns the label's colour.
#[allow(clippy::too_many_arguments)]
fn paint_face(
    cx: &mut PaintCx,
    face: Rect,
    primary: bool,
    plain: bool,
    destructive: bool,
    enabled: bool,
    selected: bool,
) -> Color {
    let t = cx.theme();
    let (hover, pressed) = (cx.is_hovered() && enabled, cx.is_pressed() && enabled);
    if !enabled {
        if !plain {
            draw::border(cx.canvas, face, t.line(), 1.0);
        }
        return t.faint();
    }
    if primary {
        let base = if destructive { t.role(Role::Danger) } else { t.selection() };
        let fill = if pressed {
            base.mix(t.bevel_lo(), 0.25)
        } else if hover {
            base.mix(Color::WHITE, 0.08)
        } else {
            base
        };
        cx.canvas.fill_rect(face, fill);
        draw::border(cx.canvas, face, t.bevel_lo(), 1.0);
        if !pressed {
            draw::top(cx.canvas, face.inset(-1.0), Color::WHITE.with_alpha(0.16), 1.0);
        }
        return if destructive { t.on_focus() } else { t.on_selection() };
    }
    let fg = if destructive { t.role(Role::Danger) } else { t.text() };
    if plain && !hover && !pressed && !selected {
        return fg;
    }
    if pressed || selected {
        draw::sunken(cx.canvas, t, face, t.sunk(), t.line2());
    } else {
        let base = if plain { t.chrome() } else { t.raised() };
        let fill = if hover { t.hover(base) } else { base };
        draw::raised(cx.canvas, t, face, Some(fill), t.line2());
    }
    fg
}

impl Widget for ButtonWidget {
    fn name(&self) -> &'static str {
        "button"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let m = &t.metrics;
        let tw = match &self.label {
            Some(l) if !l.is_empty() => cx.measure(l, &self.label_spec(t, t.text())).width.ceil(),
            _ => 0.0,
        };
        let iw = if self.icon.is_some() { self.icon_size(t) } else { 0.0 };
        let gap = if tw > 0.0 && iw > 0.0 { 6.0 } else { 0.0 };
        let pad = if self.compact || self.style == ButtonStyle::Plain { 6.0 } else { 12.0 };
        let mut h = m.control;
        let mut w = tw + iw + gap + 2.0 * pad;
        if tw == 0.0 {
            w = w.max(if self.compact { 24.0 } else { h });
        } else if !self.compact && self.style != ButtonStyle::Plain {
            w = w.max(64.0);
        } else {
            w = w.max(24.0);
        }
        if self.wrap_label
            && let Some(label) = &self.label
        {
            let width = bc.constrain(Size::new(w.ceil(), h)).width;
            let available = (width - 2.0 * pad - iw - gap).max(1.0);
            let text = cx.env.text.layout(t, label, &self.label_spec(t, t.text()), Some(available));
            h = h.max(text.size().height.ceil() + 8.0);
        }
        bc.constrain(Size::new(w.ceil(), h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let face = self.face(t, cx.size());
        let enabled = self.usable() || (self.enabled && self.on_press.is_none() && self.selected);
        let fg = paint_face(
            cx,
            face,
            self.style == ButtonStyle::Primary,
            self.style == ButtonStyle::Plain,
            self.destructive,
            enabled,
            self.selected,
        );
        if self.wrap_label {
            let pad = if self.compact || self.style == ButtonStyle::Plain { 6.0 } else { 12.0 };
            let iw = if self.icon.is_some() { self.icon_size(t) } else { 0.0 };
            let gap = if self.label.as_ref().is_some_and(|l| !l.is_empty()) && iw > 0.0 { 6.0 } else { 0.0 };
            if let Some(icon) = self.icon {
                icon.draw(cx.canvas, Point::new(pad, face.y0 + ((face.height() - iw) / 2.0).round()), iw, fg);
            }
            if let Some(label) = &self.label {
                let mut spec = self.label_spec(t, fg);
                spec.align = sidestep_ui::Alignment::Center;
                let text = cx.layout_text(label, &spec, Some((face.width() - 2.0 * pad - iw - gap).max(1.0)));
                let y = face.y0 + ((face.height() - text.size().height) / 2.0).round();
                cx.canvas.draw_text(&text, Point::new(pad + iw + gap, y));
            }
            return;
        }
        let tw = match &self.label {
            Some(l) if !l.is_empty() => cx.layout_text(l, &self.label_spec(t, fg), None).size().width.ceil(),
            _ => 0.0,
        };
        let iw = if self.icon.is_some() { self.icon_size(t) } else { 0.0 };
        let gap = if tw > 0.0 && iw > 0.0 { 6.0 } else { 0.0 };
        let content = tw + iw + gap;
        let x0 = ((face.width() - content) / 2.0).round().max(4.0);
        if let Some(i) = self.icon {
            let c = if self.style == ButtonStyle::Plain && tw == 0.0 && enabled && !cx.is_hovered() {
                t.text2()
            } else {
                fg
            };
            i.draw(cx.canvas, Point::new(x0, face.y0 + ((face.height() - iw) / 2.0).round()), iw, c);
        }
        if let Some(l) = &self.label
            && !l.is_empty()
        {
            let spec = self.label_spec(t, fg);
            let r = Rect::new(x0 + iw + gap, face.y0, face.x1 - 4.0, face.y1);
            cx.draw_text_in(l, &spec, r, Align::Start);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.label.as_deref(), self.icon, self.style, self.destructive, self.enabled, self.compact));
        h.add(&(self.selected, self.mono, self.weight.map(f32::to_bits), self.italic, self.on_press.is_some()));
        h.add(&self.wrap_label);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } if self.usable() => {
                cx.invalidate();
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } if self.usable() => {
                cx.invalidate();
                if cx.bounds().contains(*pos)
                    && let Some(m) = &self.on_press
                {
                    cx.emit(m);
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        match &self.on_press {
            Some(m) if self.enabled => {
                cx.emit(m);
                cx.invalidate();
                true
            }
            _ => false,
        }
    }

    fn is_default(&self) -> bool {
        self.default && self.usable()
    }

    fn is_cancel(&self) -> bool {
        self.cancel && self.usable()
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Button,
            label: self.label.as_deref().map(str::to_owned).or(self.icon.map(|i| i.name().to_owned())),
            disabled: !self.usable(),
            selected: self.selected.then_some(true),
            actions: vec![SemanticAction::Click, SemanticAction::Focus],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

// Switches.

/// A switch: `toggle(self.wifi).on_toggle(Msg::Wifi)`.
pub struct Toggle<Msg> {
    pub(crate) common: Common,
    on: bool,
    on_toggle: Option<MsgMap<bool>>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Toggle);

pub fn toggle<Msg>(on: bool) -> Toggle<Msg> {
    Toggle { common: Common::default(), on, on_toggle: None, enabled: true, _m: PhantomData }
}

impl<Msg: Send + 'static> Toggle<Msg> {
    pub fn on_toggle(mut self, f: impl Fn(bool) -> Msg + 'static) -> Self {
        self.on_toggle = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Toggle<Msg> {
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg> From<Toggle<Msg>> for Element<Msg> {
    fn from(t: Toggle<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(
            ToggleWidget { on: t.on, on_toggle: t.on_toggle, enabled: t.enabled },
            t.common,
            Vec::new(),
        ))
    }
}

struct ToggleWidget {
    on: bool,
    on_toggle: Option<MsgMap<bool>>,
    enabled: bool,
}

impl ToggleWidget {
    fn usable(&self) -> bool {
        self.enabled && self.on_toggle.is_some()
    }

    fn flip(&self, cx: &mut EventCx) {
        if let Some(f) = &self.on_toggle {
            cx.emit_any(f.make(!self.on));
            cx.invalidate();
        }
    }
}

impl Widget for ToggleWidget {
    fn name(&self) -> &'static str {
        "toggle"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(36.0, cx.theme().metrics.control))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let track = draw::centered(b, 32.0, 16.0);
        let usable = self.usable();
        if self.on {
            let fill = if !usable {
                t.line2()
            } else if cx.is_hovered() {
                t.selection().mix(Color::WHITE, 0.08)
            } else {
                t.selection()
            };
            cx.canvas.fill_rect(track, fill);
            draw::border(cx.canvas, track, t.bevel_lo(), 1.0);
        } else {
            draw::sunken(
                cx.canvas,
                t,
                track,
                if cx.is_hovered() && usable { t.hover(t.sunk()) } else { t.sunk() },
                if usable { t.line2() } else { t.line() },
            );
        }
        let k = 12.0;
        let x = if self.on { track.x1 - 2.0 - k } else { track.x0 + 2.0 };
        let knob = Rect::new(x, track.y0 + 2.0, x + k, track.y0 + 2.0 + k);
        let face = match (usable, self.on) {
            (false, _) => t.line2(),
            (true, true) => t.on_selection(),
            (true, false) if cx.is_hovered() => t.text2(),
            (true, false) => t.text3(),
        };
        cx.canvas.fill_rect(knob, face);
        draw::border(cx.canvas, knob, t.bevel_lo(), 1.0);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.on, self.usable()));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } if self.usable() => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } if self.usable() => {
                if cx.bounds().contains(*pos) {
                    self.flip(cx);
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        if self.usable() {
            self.flip(cx);
        }
        self.usable()
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Switch,
            checked: Some(self.on),
            disabled: !self.usable(),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }
}

// Checkboxes and radios.

/// A checkbox with its label: `checkbox("Show hidden files", self.hidden).on_toggle(Msg::Hidden)`.
pub struct Checkbox<Msg> {
    pub(crate) common: Common,
    label: Arc<str>,
    checked: bool,
    on_toggle: Option<MsgMap<bool>>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Checkbox);

pub fn checkbox<Msg>(label: impl Into<Arc<str>>, checked: bool) -> Checkbox<Msg> {
    Checkbox {
        common: Common::default(),
        label: label.into(),
        checked,
        on_toggle: None,
        enabled: true,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> Checkbox<Msg> {
    pub fn on_toggle(mut self, f: impl Fn(bool) -> Msg + 'static) -> Self {
        self.on_toggle = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Checkbox<Msg> {
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg> From<Checkbox<Msg>> for Element<Msg> {
    fn from(c: Checkbox<Msg>) -> Element<Msg> {
        let w = Check {
            label: c.label,
            kind: CheckKind::Box,
            on: c.checked,
            on_toggle: c.on_toggle,
            on_select: None,
            enabled: c.enabled,
        };
        Element::from_raw(RawElement::new(w, c.common, Vec::new()))
    }
}

/// One of a set of choices: `radio("Night", Theme::Night, Some(self.theme)).on_select(Msg::Theme)`.
pub struct Radio<Msg, V> {
    pub(crate) common: Common,
    label: Arc<str>,
    value: V,
    selected: bool,
    on_select: Option<MsgFn>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg, V> crate::element::Layoutable for Radio<Msg, V> {
    fn common_mut(&mut self) -> &mut Common {
        &mut self.common
    }
}

pub fn radio<Msg, V: Copy + PartialEq + 'static>(
    label: impl Into<Arc<str>>,
    value: V,
    selected: Option<V>,
) -> Radio<Msg, V> {
    Radio {
        common: Common::default(),
        label: label.into(),
        value,
        selected: selected == Some(value),
        on_select: None,
        enabled: true,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static, V: Copy + 'static> Radio<Msg, V> {
    pub fn on_select(mut self, f: impl Fn(V) -> Msg + 'static) -> Self {
        let v = self.value;
        let map = MsgMap::new(f);
        self.on_select = Some(MsgFn::from_fn(move || map.make(v)));
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg, V> From<Radio<Msg, V>> for Element<Msg> {
    fn from(r: Radio<Msg, V>) -> Element<Msg> {
        let w = Check {
            label: r.label,
            kind: CheckKind::Radio,
            on: r.selected,
            on_toggle: None,
            on_select: r.on_select,
            enabled: r.enabled,
        };
        Element::from_raw(RawElement::new(w, r.common, Vec::new()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CheckKind {
    Box,
    Radio,
}

struct Check {
    label: Arc<str>,
    kind: CheckKind,
    on: bool,
    on_toggle: Option<MsgMap<bool>>,
    on_select: Option<MsgFn>,
    enabled: bool,
}

impl Check {
    fn usable(&self) -> bool {
        self.enabled && (self.on_toggle.is_some() || self.on_select.is_some())
    }

    fn fire(&self, cx: &mut EventCx) {
        if let Some(f) = &self.on_toggle {
            cx.emit_any(f.make(!self.on));
        } else if let Some(m) = &self.on_select
            && !self.on
        {
            cx.emit(m);
        }
        cx.invalidate();
    }
}

const MARK: f64 = 14.0;

impl Widget for Check {
    fn name(&self) -> &'static str {
        match self.kind {
            CheckKind::Box => "checkbox",
            CheckKind::Radio => "radio",
        }
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let tw = if self.label.is_empty() {
            0.0
        } else {
            cx.measure(&self.label, &ui(t, t.metrics.body, 400.0, t.text())).width.ceil() + 8.0
        };
        bc.constrain(Size::new(MARK + tw + 2.0, t.metrics.control))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let usable = self.usable();
        let mark =
            Rect::new(1.0, ((b.height() - MARK) / 2.0).round(), 1.0 + MARK, ((b.height() - MARK) / 2.0).round() + MARK);
        let hover = cx.is_hovered() && usable;
        match self.kind {
            CheckKind::Box => {
                if self.on {
                    let fill = if !usable {
                        t.line2()
                    } else if hover {
                        t.selection().mix(Color::WHITE, 0.08)
                    } else {
                        t.selection()
                    };
                    cx.canvas.fill_rect(mark, fill);
                    draw::border(cx.canvas, mark, t.bevel_lo(), 1.0);
                    Icon::Check.draw(cx.canvas, Point::new(mark.x0 + 1.0, mark.y0 + 1.0), 12.0, t.on_selection());
                } else {
                    let fill = if hover { t.hover(t.sunk()) } else { t.sunk() };
                    draw::sunken(cx.canvas, t, mark, fill, if usable { t.line2() } else { t.line() });
                }
            }
            CheckKind::Radio => {
                let c = Circle::new(mark.center(), MARK / 2.0 - 0.5);
                let fill = if self.on {
                    if usable { t.selection() } else { t.line2() }
                } else if hover {
                    t.hover(t.sunk())
                } else {
                    t.sunk()
                };
                cx.canvas.fill(&c, fill);
                let edge = if self.on {
                    t.bevel_lo()
                } else if usable {
                    t.line2()
                } else {
                    t.line()
                };
                cx.canvas.stroke(&c, &sidestep_ui::StrokeStyle::new(1.0), edge);
                if self.on {
                    cx.canvas.fill(&Circle::new(mark.center(), 3.0), t.on_selection());
                }
            }
        }
        if !self.label.is_empty() {
            let spec = ui(t, t.metrics.body, 400.0, if usable { t.text() } else { t.faint() });
            cx.draw_text_in(&self.label, &spec, Rect::new(mark.x1 + 8.0, 0.0, b.x1, b.y1), Align::Start);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label).add(&(self.kind, self.on, self.usable()));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } if self.usable() => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } if self.usable() => {
                if cx.bounds().contains(*pos) {
                    self.fire(cx);
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        if self.usable() {
            self.fire(cx);
        }
        self.usable()
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: if self.kind == CheckKind::Box { SemanticRole::CheckBox } else { SemanticRole::RadioButton },
            label: Some(self.label.to_string()),
            checked: Some(self.on),
            disabled: !self.usable(),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.label)
    }
}

// Sliders.

/// A slider: `slider(self.volume, 0.0..=1.0).step(0.05).on_change(Msg::Volume)`.
pub struct Slider<Msg> {
    pub(crate) common: Common,
    value: f64,
    range: RangeInclusive<f64>,
    step: Option<f64>,
    ticks: usize,
    on_change: Option<MsgMap<f64>>,
    on_release: Option<MsgFn>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Slider);

pub fn slider<Msg>(value: f64, range: RangeInclusive<f64>) -> Slider<Msg> {
    Slider {
        common: Common::sized(Length::Fill, Length::Shrink),
        value,
        range,
        step: None,
        ticks: 0,
        on_change: None,
        on_release: None,
        enabled: true,
        _m: PhantomData,
    }
}

impl<Msg: Clone + Send + 'static> Slider<Msg> {
    pub fn on_change(mut self, f: impl Fn(f64) -> Msg + 'static) -> Self {
        self.on_change = Some(MsgMap::new(f));
        self
    }

    /// Sent when the pointer lets go (to commit a value dragged through).
    pub fn on_release(mut self, msg: Msg) -> Self {
        self.on_release = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg> Slider<Msg> {
    /// Values snap to multiples of `step` from the start.
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    /// `n` tick marks under the track, ends included.
    pub fn ticks(mut self, n: usize) -> Self {
        self.ticks = n;
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg> From<Slider<Msg>> for Element<Msg> {
    fn from(s: Slider<Msg>) -> Element<Msg> {
        let w = SliderWidget {
            value: s.value,
            min: *s.range.start(),
            max: *s.range.end(),
            step: s.step,
            ticks: s.ticks,
            on_change: s.on_change,
            on_release: s.on_release,
            enabled: s.enabled,
        };
        Element::from_raw(RawElement::new(w, s.common, Vec::new()))
    }
}

struct SliderWidget {
    value: f64,
    min: f64,
    max: f64,
    step: Option<f64>,
    ticks: usize,
    on_change: Option<MsgMap<f64>>,
    on_release: Option<MsgFn>,
    enabled: bool,
}

const THUMB: (f64, f64) = (10.0, 16.0);

impl SliderWidget {
    fn usable(&self) -> bool {
        self.enabled && self.on_change.is_some() && self.max > self.min
    }

    fn fraction(&self) -> f64 {
        if self.max <= self.min { 0.0 } else { ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) }
    }

    fn snap(&self, v: f64) -> f64 {
        let v = v.clamp(self.min, self.max);
        match self.step {
            Some(s) if s > 0.0 => (self.min + ((v - self.min) / s).round() * s).clamp(self.min, self.max),
            _ => v,
        }
    }

    fn key_step(&self) -> f64 {
        self.step.unwrap_or((self.max - self.min) / 100.0)
    }

    fn at(&self, x: f64, width: f64) -> f64 {
        let t = ((x - THUMB.0 / 2.0) / (width - THUMB.0).max(1.0)).clamp(0.0, 1.0);
        self.snap(self.min + t * (self.max - self.min))
    }

    fn set(&self, cx: &mut EventCx, v: f64) {
        let v = self.snap(v);
        if v != self.value
            && let Some(f) = &self.on_change
        {
            cx.emit_any(f.make(v));
        }
    }
}

impl Widget for SliderWidget {
    fn name(&self) -> &'static str {
        "slider"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let h = cx.theme().metrics.control + if self.ticks > 1 { 4.0 } else { 0.0 };
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 160.0 }.max(80.0), h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let usable = self.usable();
        let cy = (t.metrics.control / 2.0).round();
        let track = Rect::new(THUMB.0 / 2.0, cy - 2.0, b.width() - THUMB.0 / 2.0, cy + 2.0);
        draw::sunken(cx.canvas, t, track, t.sunk(), if usable { t.line2() } else { t.line() });
        let x = (track.x0 + self.fraction() * track.width()).round();
        if usable {
            cx.canvas.fill_rect(Rect::new(track.x0 + 1.0, track.y0 + 1.0, x, track.y1 - 1.0), t.selection());
        }
        if self.ticks > 1 {
            for i in 0..self.ticks {
                let tx = (track.x0 + track.width() * i as f64 / (self.ticks - 1) as f64).round();
                cx.canvas
                    .fill_rect(Rect::new(tx, cy + THUMB.1 / 2.0 + 1.0, tx + 1.0, cy + THUMB.1 / 2.0 + 4.0), t.line2());
            }
        }
        let thumb = Rect::new(x - THUMB.0 / 2.0, cy - THUMB.1 / 2.0, x + THUMB.0 / 2.0, cy + THUMB.1 / 2.0);
        let face = if !usable {
            t.line()
        } else if cx.is_pressed() {
            t.sunk()
        } else if cx.is_hovered() {
            t.hover(t.raised())
        } else {
            t.raised()
        };
        cx.canvas.fill_rect(thumb, face);
        draw::border(cx.canvas, thumb, t.bevel_lo(), 1.0);
        if usable && !cx.is_pressed() {
            draw::top(cx.canvas, thumb.inset(-1.0), t.bevel_hi(), 1.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.value).f64(self.min).f64(self.max).add(&(self.ticks, self.usable()));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Click } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.usable() {
            return Status::Ignored;
        }
        let w = cx.size().width;
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                self.set(cx, self.at(pos.x, w));
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } if cx.is_pressed() => {
                self.set(cx, self.at(pos.x, w));
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                if let Some(m) = &self.on_release {
                    cx.emit(m);
                }
                cx.invalidate();
                Status::Handled
            }
            Event::Key(k) if k.pressed => {
                let s = self.key_step();
                let v = match &k.key {
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => self.value - s,
                    Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => self.value + s,
                    Key::Named(NamedKey::PageDown) => self.value - s * 10.0,
                    Key::Named(NamedKey::PageUp) => self.value + s * 10.0,
                    Key::Named(NamedKey::Home) => self.min,
                    Key::Named(NamedKey::End) => self.max,
                    _ => return Status::Ignored,
                };
                self.set(cx, v);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn cursor(&self) -> Option<Cursor> {
        self.usable().then_some(Cursor::Pointer)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Slider,
            value: Some(format!("{}", self.value)),
            range: Some((self.min, self.max, self.value)),
            disabled: !self.usable(),
            actions: vec![SemanticAction::Increment, SemanticAction::Decrement, SemanticAction::SetValue],
            ..Default::default()
        })
    }
}

// Steppers.

/// A number with − and + beside it: `stepper(self.size).range(8.0..=72.0).step(1.0).on_change(Msg::Size)`.
pub struct Stepper<Msg> {
    pub(crate) common: Common,
    value: f64,
    range: RangeInclusive<f64>,
    step: f64,
    decimals: usize,
    suffix: Option<Arc<str>>,
    on_change: Option<MsgMap<f64>>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Stepper);

pub fn stepper<Msg>(value: f64) -> Stepper<Msg> {
    Stepper {
        common: Common::default(),
        value,
        range: f64::MIN..=f64::MAX,
        step: 1.0,
        decimals: 0,
        suffix: None,
        on_change: None,
        enabled: true,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> Stepper<Msg> {
    pub fn on_change(mut self, f: impl Fn(f64) -> Msg + 'static) -> Self {
        self.on_change = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Stepper<Msg> {
    pub fn range(mut self, r: RangeInclusive<f64>) -> Self {
        self.range = r;
        self
    }

    pub fn step(mut self, s: f64) -> Self {
        self.step = s;
        let frac = format!("{s}");
        self.decimals = frac.split_once('.').map_or(0, |(_, f)| f.len());
        self
    }

    /// Text after the number ("pt", "%").
    pub fn suffix(mut self, s: impl Into<Arc<str>>) -> Self {
        self.suffix = Some(s.into());
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg> From<Stepper<Msg>> for Element<Msg> {
    fn from(s: Stepper<Msg>) -> Element<Msg> {
        let w = StepperWidget {
            value: s.value,
            min: *s.range.start(),
            max: *s.range.end(),
            step: s.step,
            decimals: s.decimals,
            suffix: s.suffix,
            on_change: s.on_change,
            enabled: s.enabled,
        };
        Element::from_raw(RawElement::new(w, s.common, Vec::new()))
    }
}

struct StepperWidget {
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    decimals: usize,
    suffix: Option<Arc<str>>,
    on_change: Option<MsgMap<f64>>,
    enabled: bool,
}

#[derive(Default)]
struct StepperState {
    /// The half under the pointer: −1, 0 (the number) or +1.
    zone: i8,
}

impl StepperWidget {
    fn label(&self) -> String {
        let mut s = format!("{:.*}", self.decimals, self.value);
        if let Some(x) = &self.suffix {
            s.push_str(x);
        }
        s
    }

    fn usable(&self) -> bool {
        self.enabled && self.on_change.is_some()
    }

    fn bump(&self, cx: &mut EventCx, by: f64) {
        let v = (self.value + by).clamp(self.min, self.max);
        if v != self.value
            && let Some(f) = &self.on_change
        {
            cx.emit_any(f.make(v));
            cx.invalidate();
        }
    }

    fn zone(&self, x: f64, w: f64, h: f64) -> i8 {
        if x < h {
            -1
        } else if x > w - h {
            1
        } else {
            0
        }
    }
}

impl Widget for StepperWidget {
    fn name(&self) -> &'static str {
        "stepper"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let h = t.metrics.control;
        let spec = TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(t.metrics.body) }, t.text());
        let tw = cx.measure(&self.label(), &spec).width.ceil().max(32.0);
        bc.constrain(Size::new(h * 2.0 + tw + 16.0, h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let h = b.height();
        let usable = self.usable();
        draw::sunken(cx.canvas, t, b, t.sunk(), if usable { t.line2() } else { t.line() });
        let zone = cx.state::<StepperState>().map_or(0, |s| s.zone);
        for (z, icon, r) in [
            (-1i8, Icon::Minus, Rect::new(b.x0, b.y0, b.x0 + h, b.y1)),
            (1, Icon::Plus, Rect::new(b.x1 - h, b.y0, b.x1, b.y1)),
        ] {
            let at_end = (z < 0 && self.value <= self.min) || (z > 0 && self.value >= self.max);
            let live = usable && !at_end;
            if live {
                let fill = if cx.is_pressed() && zone == z {
                    t.sunk()
                } else if cx.is_hovered() && zone == z {
                    t.hover(t.raised())
                } else {
                    t.raised()
                };
                draw::raised(cx.canvas, t, r, Some(fill), t.line2());
            } else {
                draw::border(cx.canvas, r, t.line(), 1.0);
            }
            icon.draw(
                cx.canvas,
                Point::new(r.x0 + (h - 12.0) / 2.0, r.y0 + (h - 12.0) / 2.0),
                12.0,
                if live { t.text() } else { t.faint() },
            );
        }
        let spec = TextSpec::new(
            FontSpec { tabular: true, ..FontSpec::mono(t.metrics.body) },
            if usable { t.text() } else { t.faint() },
        );
        cx.draw_text_in(&self.label(), &spec, Rect::new(b.x0 + h, b.y0, b.x1 - h, b.y1), Align::Center);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label()).add(&(self.usable(), self.value <= self.min, self.value >= self.max));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.usable() {
            return Status::Ignored;
        }
        let s = cx.size();
        match event {
            Event::PointerMove { pos, .. } => {
                let z = self.zone(pos.x, s.width, s.height);
                if cx.state::<StepperState>().zone != z {
                    cx.state::<StepperState>().zone = z;
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let z = self.zone(pos.x, s.width, s.height);
                cx.state::<StepperState>().zone = z;
                if z != 0 {
                    self.bump(cx, self.step * f64::from(z));
                }
                Status::Handled
            }
            Event::PointerUp { .. } => {
                cx.invalidate();
                Status::Handled
            }
            _ if is_key(event, NamedKey::ArrowUp) || is_key(event, NamedKey::ArrowRight) => {
                self.bump(cx, self.step);
                Status::Handled
            }
            _ if is_key(event, NamedKey::ArrowDown) || is_key(event, NamedKey::ArrowLeft) => {
                self.bump(cx, -self.step);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::SpinButton,
            value: Some(self.label()),
            range: Some((self.min, self.max, self.value)),
            disabled: !self.usable(),
            actions: vec![SemanticAction::Increment, SemanticAction::Decrement],
            ..Default::default()
        })
    }
}

// Segmented controls.

/// One of a few choices side by side: `segmented(["Night", "Day", "Auto"], self.theme).on_select(Msg::Theme)`.
pub struct Segmented<Msg> {
    pub(crate) common: Common,
    options: Vec<(Option<Arc<str>>, Option<Icon>)>,
    selected: Option<usize>,
    on_select: Option<MsgMap<usize>>,
    enabled: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Segmented);

pub fn segmented<Msg, S: Into<Arc<str>>>(
    options: impl IntoIterator<Item = S>,
    selected: impl Into<Option<usize>>,
) -> Segmented<Msg> {
    Segmented {
        common: Common::default(),
        options: options.into_iter().map(|s| (Some(s.into()), None)).collect(),
        selected: selected.into(),
        on_select: None,
        enabled: true,
        _m: PhantomData,
    }
}

/// Icon segments (each with a tooltip-like label for assistive tech).
pub fn segmented_icons<Msg>(
    options: impl IntoIterator<Item = Icon>,
    selected: impl Into<Option<usize>>,
) -> Segmented<Msg> {
    Segmented {
        common: Common::default(),
        options: options.into_iter().map(|i| (None, Some(i))).collect(),
        selected: selected.into(),
        on_select: None,
        enabled: true,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> Segmented<Msg> {
    pub fn on_select(mut self, f: impl Fn(usize) -> Msg + 'static) -> Self {
        self.on_select = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Segmented<Msg> {
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl<Msg> From<Segmented<Msg>> for Element<Msg> {
    fn from(s: Segmented<Msg>) -> Element<Msg> {
        let w = SegWidget {
            options: s.options,
            selected: s.selected,
            on_select: s.on_select,
            enabled: s.enabled,
            edges: Vec::new(),
        };
        Element::from_raw(RawElement::new(w, s.common, Vec::new()))
    }
}

struct SegWidget {
    options: Vec<(Option<Arc<str>>, Option<Icon>)>,
    selected: Option<usize>,
    on_select: Option<MsgMap<usize>>,
    enabled: bool,
    edges: Vec<f64>,
}

#[derive(Default)]
struct SegState {
    hover: Option<usize>,
}

impl SegWidget {
    fn spec(t: &Theme, selected: bool, color: Color) -> TextSpec {
        ui(t, t.metrics.small + 1.0, if selected { 650.0 } else { 400.0 }, color)
    }

    fn at(&self, x: f64) -> Option<usize> {
        self.edges.windows(2).position(|w| x >= w[0] && x < w[1])
    }

    fn usable(&self) -> bool {
        self.enabled && self.on_select.is_some()
    }

    fn choose(&self, cx: &mut EventCx, i: usize) {
        if Some(i) != self.selected
            && let Some(f) = &self.on_select
        {
            cx.emit_any(f.make(i));
            cx.invalidate();
        }
    }
}

impl Widget for SegWidget {
    fn name(&self) -> &'static str {
        "segmented"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let mut widths = Vec::new();
        for (i, (label, icon)) in self.options.iter().enumerate() {
            let tw = label
                .as_ref()
                .map_or(0.0, |l| cx.measure(l, &SegWidget::spec(t, self.selected == Some(i), t.text())).width.ceil());
            let iw = if icon.is_some() { 14.0 } else { 0.0 };
            widths.push((tw + iw + if tw > 0.0 && iw > 0.0 { 6.0 } else { 0.0 } + 20.0).max(32.0));
        }
        let natural: f64 = widths.iter().sum::<f64>() + 2.0;
        let total = if bc.min.width > natural { bc.min.width } else { natural };
        // Extra room shared evenly.
        let extra = (total - natural) / widths.len().max(1) as f64;
        let mut x = 1.0;
        self.edges = vec![x];
        for w in &widths {
            x += (w + extra).round();
            self.edges.push(x);
        }
        bc.constrain(Size::new(x + 1.0, t.metrics.control))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let usable = self.usable();
        cx.canvas.fill_rect(b, t.sunk());
        draw::border(cx.canvas, b, t.bevel_lo(), 1.0);
        let hover = cx.state::<SegState>().and_then(|s| s.hover).filter(|_| cx.is_hovered() && usable);
        for (i, (label, icon)) in self.options.iter().enumerate() {
            let cell = Rect::new(self.edges[i], 1.0, self.edges[i + 1], b.y1 - 1.0);
            let sel = self.selected == Some(i);
            let fg = if sel {
                cx.canvas.fill_rect(cell, if usable { t.selection() } else { t.line2() });
                t.on_selection()
            } else {
                if hover == Some(i) {
                    cx.canvas.fill_rect(cell, t.hover(t.sunk()));
                }
                if usable { t.text2() } else { t.faint() }
            };
            if i + 1 < self.options.len() && !sel && self.selected != Some(i + 1) {
                cx.canvas.fill_rect(Rect::new(cell.x1 - 1.0, cell.y0 + 4.0, cell.x1, cell.y1 - 4.0), t.line());
            }
            let spec = SegWidget::spec(t, sel, fg);
            let tw = label.as_ref().map_or(0.0, |l| cx.layout_text(l, &spec, None).size().width.ceil());
            let iw = if icon.is_some() { 14.0 } else { 0.0 };
            let gap = if tw > 0.0 && iw > 0.0 { 6.0 } else { 0.0 };
            let x = (cell.x0 + (cell.width() - tw - iw - gap) / 2.0).round();
            if let Some(ic) = icon {
                ic.draw(cx.canvas, Point::new(x, (cell.y0 + (cell.height() - 14.0) / 2.0).round()), 14.0, fg);
            }
            if let Some(l) = label {
                cx.draw_text_in(l, &spec, Rect::new(x + iw + gap, cell.y0, cell.x1, cell.y1), Align::Start);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for (l, i) in &self.options {
            h.add(&(l.as_deref(), i));
        }
        h.add(&(self.selected, self.usable()));
    }

    fn hit(&self) -> bool {
        true
    }

    /// The segments, the whole height (no height: the widget's own).
    fn parts(&self) -> Vec<(String, Rect)> {
        self.options
            .iter()
            .zip(self.edges.windows(2))
            .enumerate()
            .map(|(i, ((label, icon), w))| {
                let name = label.as_deref().map(str::to_owned).or(icon.map(|i| i.name().to_owned()));
                (name.unwrap_or_else(|| i.to_string()), Rect::new(w[0], 0.0, w[1], 0.0))
            })
            .collect()
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.usable() {
            return Status::Ignored;
        }
        match event {
            Event::PointerMove { pos, .. } => {
                let at = self.at(pos.x);
                if cx.state::<SegState>().hover != at {
                    cx.state::<SegState>().hover = at;
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                if let Some(i) = self.at(pos.x).filter(|_| cx.bounds().contains(*pos)) {
                    self.choose(cx, i);
                }
                Status::Handled
            }
            _ if is_key(event, NamedKey::ArrowLeft) => {
                let i = self.selected.map_or(0, |s| s.saturating_sub(1));
                self.choose(cx, i);
                Status::Handled
            }
            _ if is_key(event, NamedKey::ArrowRight) => {
                let i = self.selected.map_or(0, |s| (s + 1).min(self.options.len().saturating_sub(1)));
                self.choose(cx, i);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        let label = self
            .selected
            .and_then(|i| self.options.get(i))
            .and_then(|(l, i)| l.as_deref().map(str::to_owned).or(i.map(|i| i.name().to_owned())));
        Some(Semantics { role: SemanticRole::TabList, value: label, disabled: !self.usable(), ..Default::default() })
    }
}

// Pop-up buttons.

/// A pop-up button that opens a menu of choices, as the board's
/// "Markdown ⌄": `dropdown(["Markdown", "Plain Text"], Some(0)).on_select(Msg::Syntax)`.
pub struct Dropdown<Msg> {
    pub(crate) common: Common,
    options: Vec<String>,
    selected: Option<usize>,
    placeholder: Option<Arc<str>>,
    on_select: Option<MsgMap<usize>>,
    enabled: bool,
    compact: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Dropdown);

pub fn dropdown<Msg, S: Into<String>>(
    options: impl IntoIterator<Item = S>,
    selected: impl Into<Option<usize>>,
) -> Dropdown<Msg> {
    Dropdown {
        common: Common::default(),
        options: options.into_iter().map(Into::into).collect(),
        selected: selected.into(),
        placeholder: None,
        on_select: None,
        enabled: true,
        compact: false,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> Dropdown<Msg> {
    pub fn on_select(mut self, f: impl Fn(usize) -> Msg + 'static) -> Self {
        self.on_select = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Dropdown<Msg> {
    /// What it says with nothing chosen.
    pub fn placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// 20pt tall, 12pt text: a toolbar's.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl<Msg> From<Dropdown<Msg>> for Element<Msg> {
    fn from(d: Dropdown<Msg>) -> Element<Msg> {
        let w = DropdownWidget {
            options: d.options,
            selected: d.selected,
            placeholder: d.placeholder,
            on_select: d.on_select,
            enabled: d.enabled,
            compact: d.compact,
        };
        Element::from_raw(RawElement::new(w, d.common, Vec::new()))
    }
}

struct DropdownWidget {
    options: Vec<String>,
    selected: Option<usize>,
    placeholder: Option<Arc<str>>,
    on_select: Option<MsgMap<usize>>,
    enabled: bool,
    compact: bool,
}

impl DropdownWidget {
    fn usable(&self) -> bool {
        self.enabled && self.on_select.is_some() && !self.options.is_empty()
    }

    fn label(&self) -> &str {
        match self.selected.and_then(|i| self.options.get(i)) {
            Some(s) => s,
            None => self.placeholder.as_deref().unwrap_or(""),
        }
    }

    fn spec(&self, t: &Theme, color: Color) -> TextSpec {
        ui(t, if self.compact { t.metrics.small + 1.0 } else { t.metrics.body }, 400.0, color)
    }

    fn face(&self, t: &Theme, size: Size) -> Rect {
        let r = Rect::from_origin_size(Point::ZERO, size);
        if self.compact { draw::centered(r, r.width(), t.metrics.control_compact) } else { r }
    }

    fn open(&self, cx: &mut EventCx) {
        let Some(f) = &self.on_select else { return };
        let items = self
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let f = f.clone();
                ItemSpec::Action(ActionSpec {
                    label: o.clone(),
                    msg: Some(MsgFn::from_fn(move || f.make(i))),
                    shortcut: None,
                    enabled: true,
                    checked: self.selected == Some(i),
                    role: None,
                    id: None,
                    icon: None,
                    detail: None,
                    mark: None,
                })
            })
            .collect();
        let face = self.face(cx.theme(), cx.size());
        let w = face.width();
        cx.open_menu(MenuSpec { title: String::new(), items, icon: None }, face, w);
    }
}

impl Widget for DropdownWidget {
    fn name(&self) -> &'static str {
        "dropdown"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let spec = self.spec(t, t.text());
        let widest = self
            .options
            .iter()
            .map(String::as_str)
            .chain(self.placeholder.as_deref())
            .map(|o| cx.measure(o, &spec).width)
            .fold(0.0f64, f64::max);
        let w = (widest + 8.0 + 6.0 + 10.0 + 8.0).ceil().max(48.0);
        bc.constrain(Size::new(w, t.metrics.control))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let face = self.face(t, cx.size());
        let usable = self.usable();
        if usable {
            let fill = if cx.is_pressed() {
                Some(t.sunk())
            } else if cx.is_hovered() {
                Some(t.hover(if self.compact { t.chrome() } else { t.raised() }))
            } else if self.compact {
                None
            } else {
                Some(t.raised())
            };
            draw::raised(cx.canvas, t, face, fill, t.line2());
        } else {
            draw::border(cx.canvas, face, t.line(), 1.0);
        }
        let color = if !usable {
            t.faint()
        } else if self.selected.is_none() {
            t.text3()
        } else {
            t.text()
        };
        let spec = self.spec(t, color);
        cx.draw_text_in(self.label(), &spec, Rect::new(face.x0 + 8.0, face.y0, face.x1 - 22.0, face.y1), Align::Start);
        Icon::ChevronDown.draw(
            cx.canvas,
            Point::new(face.x1 - 18.0, face.y0 + ((face.height() - 10.0) / 2.0).round()),
            10.0,
            color,
        );
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(self.label()).add(&(self.usable(), self.compact, self.selected));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.usable() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.usable() {
            return Status::Ignored;
        }
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                self.open(cx);
                cx.invalidate();
                Status::Handled
            }
            Event::PointerUp { .. } => {
                cx.invalidate();
                Status::Handled
            }
            _ if is_key(event, NamedKey::ArrowDown) || is_key(event, NamedKey::ArrowUp) => {
                self.open(cx);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        if self.usable() {
            self.open(cx);
        }
        self.usable()
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::ComboBox,
            value: Some(self.label().to_owned()),
            disabled: !self.usable(),
            actions: vec![SemanticAction::ShowMenu],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(self.label())
    }
}

// Progress.

/// How far along something is (0.0–1.0), or `progress_indeterminate()`.
pub struct Progress<Msg> {
    pub(crate) common: Common,
    fraction: Option<f64>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Progress);

pub fn progress<Msg>(fraction: f64) -> Progress<Msg> {
    Progress {
        common: Common::sized(Length::Fill, Length::Shrink),
        fraction: Some(fraction.clamp(0.0, 1.0)),
        _m: PhantomData,
    }
}

/// Busy for a while unknown: a block slides along the track, slowly, and
/// only its own rectangle redraws; under reduced motion it stands still.
pub fn progress_indeterminate<Msg>() -> Progress<Msg> {
    Progress { common: Common::sized(Length::Fill, Length::Shrink), fraction: None, _m: PhantomData }
}

impl<Msg> From<Progress<Msg>> for Element<Msg> {
    fn from(p: Progress<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(ProgressWidget { fraction: p.fraction, moving: false }, p.common, Vec::new()))
    }
}

struct ProgressWidget {
    fraction: Option<f64>,
    moving: bool,
}

#[derive(Default)]
struct Sweep {
    started: Option<std::time::Instant>,
}

const SWEEP: Duration = Duration::from_millis(1600);
const SWEEP_FRAME: Duration = Duration::from_millis(50);

impl Widget for ProgressWidget {
    fn name(&self) -> &'static str {
        "progress"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        self.moving = self.fraction.is_none() && cx.theme().motion();
        if self.moving {
            let now = cx.now();
            cx.state::<Sweep>().started.get_or_insert(now);
            cx.request_tick(SWEEP_FRAME);
        }
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 120.0 }, 8.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let track = draw::centered(b, b.width(), 6.0);
        draw::sunken(cx.canvas, t, track, t.sunk(), t.line());
        let inner = track.inset(-1.0);
        match self.fraction {
            Some(f) => {
                let x = (inner.x0 + inner.width() * f).round();
                cx.canvas.fill_rect(Rect::new(inner.x0, inner.y0, x, inner.y1), t.selection());
            }
            None if self.moving => {
                let started = cx.state::<Sweep>().and_then(|s| s.started).unwrap_or(cx.now());
                let phase = (cx.now().duration_since(started).as_secs_f64() / SWEEP.as_secs_f64()).fract();
                let w = (inner.width() * 0.25).round();
                let x = (inner.x0 - w + (inner.width() + w) * phase).round();
                let block = Rect::new(x.max(inner.x0), inner.y0, (x + w).min(inner.x1), inner.y1);
                if block.width() > 0.0 {
                    cx.canvas.fill_rect(block, t.selection());
                }
            }
            None => {
                // Still stripes, every 8pt.
                let mut x = inner.x0;
                while x < inner.x1 {
                    cx.canvas.fill_rect(
                        Rect::new(x, inner.y0, (x + 4.0).min(inner.x1), inner.y1),
                        t.selection().with_alpha(0.6),
                    );
                    x += 8.0;
                }
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.fraction.unwrap_or(-1.0)).add(&self.moving);
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if *event == Event::Tick && self.moving {
            cx.invalidate();
            cx.request_tick(SWEEP_FRAME);
            return Status::Handled;
        }
        Status::Ignored
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::ProgressIndicator,
            range: self.fraction.map(|f| (0.0, 1.0, f)),
            ..Default::default()
        })
    }
}

// Meters.

/// Bars side by side, filled from the bottom: the menu bar's CPU cores.
pub struct Meter<Msg> {
    pub(crate) common: Common,
    values: Vec<f32>,
    role: Role,
    bar: f64,
    gap: f64,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Meter);

pub fn meter<Msg>(values: impl Into<Vec<f32>>) -> Meter<Msg> {
    Meter {
        common: Common::sized(Length::Shrink, Length::Fixed(14.0)),
        values: values.into(),
        role: Role::Success,
        bar: 3.0,
        gap: 1.0,
        _m: PhantomData,
    }
}

impl<Msg> Meter<Msg> {
    pub fn role(mut self, r: Role) -> Self {
        self.role = r;
        self
    }

    pub fn bar_width(mut self, w: f64) -> Self {
        self.bar = w;
        self
    }

    pub fn gap(mut self, g: f64) -> Self {
        self.gap = g;
        self
    }
}

impl<Msg> From<Meter<Msg>> for Element<Msg> {
    fn from(m: Meter<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(
            MeterWidget { values: m.values, role: m.role, bar: m.bar, gap: m.gap },
            m.common,
            Vec::new(),
        ))
    }
}

struct MeterWidget {
    values: Vec<f32>,
    role: Role,
    bar: f64,
    gap: f64,
}

impl Widget for MeterWidget {
    fn name(&self) -> &'static str {
        "meter"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let n = self.values.len() as f64;
        bc.constrain(Size::new(n * self.bar + (n - 1.0).max(0.0) * self.gap, 14.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let h = cx.size().height;
        let fill = t.role(self.role);
        for (i, v) in self.values.iter().enumerate() {
            let x = i as f64 * (self.bar + self.gap);
            cx.canvas.fill_rect(Rect::new(x, 0.0, x + self.bar, h), t.line2());
            let top = (h * (1.0 - f64::from(v.clamp(0.0, 1.0)))).round();
            if top < h {
                cx.canvas.fill_rect(Rect::new(x, top, x + self.bar, h), fill);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for v in &self.values {
            h.add(&v.to_bits());
        }
        h.add(&self.role).f64(self.bar).f64(self.gap);
    }

    fn semantics(&self) -> Option<Semantics> {
        let avg = self.values.iter().map(|v| f64::from(*v)).sum::<f64>() / self.values.len().max(1) as f64;
        Some(Semantics { role: SemanticRole::Meter, range: Some((0.0, 1.0, avg)), ..Default::default() })
    }
}

// Badges and keycaps.

/// A count or a word in a small block: `badge("3")`.
pub struct Badge<Msg> {
    pub(crate) common: Common,
    text: Arc<str>,
    role: Role,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Badge);

pub fn badge<Msg>(text: impl Into<Arc<str>>) -> Badge<Msg> {
    Badge { common: Common::default(), text: text.into(), role: Role::Danger, _m: PhantomData }
}

impl<Msg> Badge<Msg> {
    /// The fill's colour (red by default).
    pub fn role(mut self, r: Role) -> Self {
        self.role = r;
        self
    }
}

impl<Msg> From<Badge<Msg>> for Element<Msg> {
    fn from(b: Badge<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(Tag { text: b.text, role: b.role, keycap: false }, b.common, Vec::new()))
    }
}

/// A key as printed on a keycap: `keycap("❖")`, `keycap("F")`.
pub struct Keycap<Msg> {
    pub(crate) common: Common,
    text: Arc<str>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Keycap);

pub fn keycap<Msg>(text: impl Into<Arc<str>>) -> Keycap<Msg> {
    Keycap { common: Common::default(), text: text.into(), _m: PhantomData }
}

impl<Msg> From<Keycap<Msg>> for Element<Msg> {
    fn from(k: Keycap<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(Tag { text: k.text, role: Role::Text, keycap: true }, k.common, Vec::new()))
    }
}

struct Tag {
    text: Arc<str>,
    role: Role,
    keycap: bool,
}

impl Tag {
    fn spec(&self, t: &Theme) -> TextSpec {
        if self.keycap {
            TextSpec::new(FontSpec { weight: 500.0, ..FontSpec::mono(12.0 * t.metrics.factor) }, t.text())
        } else {
            let color = if t.is_dark() { t.on_focus() } else { Color::WHITE };
            TextSpec::new(FontSpec { weight: 600.0, tabular: true, ..FontSpec::mono(t.metrics.caption) }, color)
        }
    }
}

impl Widget for Tag {
    fn name(&self) -> &'static str {
        if self.keycap { "keycap" } else { "badge" }
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let spec = self.spec(cx.env.theme);
        let tw = cx.measure(&self.text, &spec).width.ceil();
        if self.keycap {
            bc.constrain(Size::new((tw + 12.0).max(22.0), 22.0))
        } else {
            bc.constrain(Size::new((tw + 10.0).max(16.0), 16.0))
        }
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let spec = self.spec(t);
        if self.keycap {
            cx.canvas.fill_rect(b, t.raised());
            draw::border(cx.canvas, b, t.line2(), 1.0);
            draw::bottom(cx.canvas, b, t.bevel_lo(), 2.0);
            draw::top(cx.canvas, b.inset(-1.0), t.bevel_hi(), 1.0);
            cx.draw_text_in(&self.text, &spec, Rect::new(b.x0, b.y0, b.x1, b.y1 - 1.0), Align::Center);
        } else {
            cx.canvas.fill_rect(b, t.role(self.role));
            cx.draw_text_in(&self.text, &spec, b, Align::Center);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.text).add(&(self.role, self.keycap));
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Label, label: Some(self.text.to_string()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }
}

// Images.

/// How an image fills the room it's given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Fit {
    /// All of it, as large as fits (the default).
    #[default]
    Contain,
    /// The room filled, cropping it.
    Cover,
    /// Stretched to the room.
    Fill,
    /// At its own size.
    None,
}

/// A picture: `image(Image::open(path)?).fit(Fit::Cover)`.
pub struct ImageEl<Msg> {
    pub(crate) common: Common,
    image: Image,
    fit: Fit,
    revision: u64,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(ImageEl);

pub fn image<Msg>(image: Image) -> ImageEl<Msg> {
    ImageEl { common: Common::default(), image, fit: Fit::Contain, revision: 0, _m: PhantomData }
}

impl<Msg> ImageEl<Msg> {
    pub fn fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    /// Bump when the picture changes but its size doesn't: the kit tells
    /// images apart by size and this.
    pub fn revision(mut self, r: u64) -> Self {
        self.revision = r;
        self
    }
}

impl<Msg> From<ImageEl<Msg>> for Element<Msg> {
    fn from(i: ImageEl<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(
            ImageWidget { image: i.image, fit: i.fit, revision: i.revision },
            i.common,
            Vec::new(),
        ))
    }
}

struct ImageWidget {
    image: Image,
    fit: Fit,
    revision: u64,
}

/// An image element's picture scaled to where it's drawn: the picture's
/// size and revision, the device size and the scale it's for.
#[derive(Default)]
struct Scaled {
    made: std::cell::RefCell<Option<(ScaleKey, Image)>>,
    /// The key drawn last without a picture made for it.
    seen: std::cell::Cell<Option<ScaleKey>>,
}

type ScaleKey = ((u32, u32), u64, (u32, u32), u64);

impl Scaled {
    /// The picture for `key`: made if it was asked for last time too.
    fn get(&self, key: ScaleKey, make: impl FnOnce() -> Image) -> Option<Image> {
        if let Some((k, img)) = &*self.made.borrow()
            && *k == key
        {
            return Some(img.clone());
        }
        if self.seen.replace(Some(key)) != Some(key) {
            return None;
        }
        let img = make();
        *self.made.borrow_mut() = Some((key, img.clone()));
        Some(img)
    }
}

impl Widget for ImageWidget {
    fn name(&self) -> &'static str {
        "image"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        // Where the picture scaled to its room is kept (see `paint`).
        cx.state::<Scaled>();
        let s = self.image.size();
        if s.width <= 0.0 || s.height <= 0.0 {
            return bc.min;
        }
        let k = (bc.max.width / s.width).min(bc.max.height / s.height).min(1.0);
        bc.constrain(Size::new(s.width * k, s.height * k))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let s = self.image.size();
        if s.width <= 0.0 || s.height <= 0.0 {
            return;
        }
        let dst = match self.fit {
            Fit::Fill => b,
            Fit::None => draw::centered(b, s.width, s.height),
            Fit::Contain | Fit::Cover => {
                let kx = b.width() / s.width;
                let ky = b.height() / s.height;
                let k = if self.fit == Fit::Contain { kx.min(ky) } else { kx.max(ky) };
                let (w, h) = (s.width * k, s.height * k);
                Rect::new(
                    b.x0 + (b.width() - w) / 2.0,
                    b.y0 + (b.height() - h) / 2.0,
                    b.x0 + (b.width() + w) / 2.0,
                    b.y0 + (b.height() + h) / 2.0,
                )
            }
        };
        cx.canvas.save();
        cx.canvas.clip_rect(b);
        // Scaling smoothly is slow: a picture drawn at another size than
        // its own is scaled once, the second time it's drawn at that size
        // (not while a window is being resized), and copied one to one
        // from then on.
        let scale = cx.canvas.scale();
        let [a, sb, sc, d, _, _] = cx.canvas.transform().as_coeffs();
        let device = ((dst.width() * scale).round() as u32, (dst.height() * scale).round() as u32);
        let upright = sb == 0.0 && sc == 0.0 && a == 1.0 && d == 1.0;
        let scaled = match cx.state::<Scaled>() {
            Some(cache) if upright && device != self.image.pixel_size() && device.0 > 0 && device.1 > 0 => {
                let key = (self.image.pixel_size(), self.revision, device, scale.to_bits());
                cache.get(key, || {
                    let size = Size::new(f64::from(device.0) / scale, f64::from(device.1) / scale);
                    let image = self.image.clone();
                    sidestep_ui::offscreen::render(size, scale, sidestep_ui::Appearance::DARK, |c| {
                        c.draw_image_with(
                            &image,
                            None,
                            Rect::from_origin_size(Point::ZERO, size),
                            ImageOptions::default(),
                        );
                    })
                })
            }
            _ => None,
        };
        match scaled {
            Some(img) => {
                // On whole device pixels, so it's copied one to one.
                let o = cx.canvas.transform() * dst.origin();
                let snap = |v: f64| (v * scale).round() / scale;
                let at = Point::new(snap(o.x) - (o.x - dst.x0), snap(o.y) - (o.y - dst.y0));
                cx.canvas.draw_image_with(&img, None, Rect::from_origin_size(at, img.size()), ImageOptions::default());
            }
            None => cx.canvas.draw_image_with(&self.image, None, dst, ImageOptions::default()),
        }
        cx.canvas.restore();
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.image.pixel_size(), self.fit, self.revision)).f64(self.image.size().width);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, ..Default::default() })
    }
}
