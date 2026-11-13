//! Text: the layout cache, and the `text` and `rich_text` elements.
//!
//! Shaping is the costly part of drawing text, so every layout the kit
//! makes goes through [`TextCache`], keyed by the text, its style and the
//! width it was broken at: rebuilding a view reshapes nothing that didn't
//! change.

use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;

use sidestep_ui::{Alignment, Color, TextLayout, TextStyle, Wrap};

use crate::element::{Common, Element, RawElement};
use crate::hash::{Fx, FxHashMap, PaintHasher};
use crate::layout::{Constraints, Length, Point, Size};
use crate::theme::{Family, FontSpec, Role, Theme};
use crate::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};

/// How a piece of text is set.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    pub font: FontSpec,
    pub color: Color,
    pub wrap: Wrap,
    /// At most this many lines (0: no limit), the last ending in an
    /// ellipsis if text is left out.
    pub max_lines: usize,
    pub align: Alignment,
    /// Lines this many times their natural height (0: natural).
    pub line_height: f64,
    pub underline: bool,
    pub strikethrough: bool,
    pub letter_spacing: f64,
}

impl TextSpec {
    pub fn new(font: FontSpec, color: Color) -> TextSpec {
        TextSpec {
            font,
            color,
            wrap: Wrap::Truncate,
            max_lines: 0,
            align: Alignment::Start,
            line_height: 0.0,
            underline: false,
            strikethrough: false,
            letter_spacing: 0.0,
        }
    }

    fn hash_into(&self, h: &mut Fx) {
        let f = &self.font;
        (f.family, f.size.to_bits(), f.weight.to_bits(), f.italic, f.tabular, f.condensed).hash(h);
        for c in [self.color.r, self.color.g, self.color.b, self.color.a] {
            h.write_u32(c.to_bits());
        }
        (self.wrap as u8, self.max_lines, self.align as u8, self.line_height.to_bits()).hash(h);
        (self.underline, self.strikethrough, self.letter_spacing.to_bits()).hash(h);
    }

    pub(crate) fn style(&self, theme: &Theme) -> TextStyle {
        let mut s = TextStyle::new(theme.font(self.font), self.color);
        s.underline = self.underline;
        s.strikethrough = self.strikethrough;
        s.letter_spacing = self.letter_spacing;
        s
    }
}

/// A run of rich text with its own look, over the text's base style.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub role: Option<Role>,
    pub color: Option<Color>,
    pub weight: Option<f32>,
    pub italic: bool,
    pub mono: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub background: Option<Color>,
    pub size: Option<f64>,
    /// Its colour at this strength (0.0–1.0): a role made quieter (a
    /// heading's #s).
    pub alpha: Option<f32>,
}

impl Span {
    pub fn new(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            role: None,
            color: None,
            weight: None,
            italic: false,
            mono: false,
            underline: false,
            strikethrough: false,
            background: None,
            size: None,
            alpha: None,
        }
    }

    pub fn role(mut self, role: Role) -> Span {
        self.role = Some(role);
        self
    }

    pub fn color(mut self, color: Color) -> Span {
        self.color = Some(color);
        self
    }

    pub fn weight(mut self, w: f32) -> Span {
        self.weight = Some(w);
        self
    }

    pub fn bold(self) -> Span {
        self.weight(aizome_theme::typeface::WEIGHT_BOLD)
    }

    pub fn italic(mut self) -> Span {
        self.italic = true;
        self
    }

    pub fn mono(mut self) -> Span {
        self.mono = true;
        self
    }

    pub fn underline(mut self) -> Span {
        self.underline = true;
        self
    }

    pub fn strikethrough(mut self) -> Span {
        self.strikethrough = true;
        self
    }

    pub fn background(mut self, c: Color) -> Span {
        self.background = Some(c);
        self
    }

    pub fn size(mut self, s: f64) -> Span {
        self.size = Some(s);
        self
    }

    /// Its colour at `a` of its strength.
    pub fn alpha(mut self, a: f32) -> Span {
        self.alpha = Some(a.clamp(0.0, 1.0));
        self
    }

    fn hash_into(&self, h: &mut Fx) {
        self.text.hash(h);
        (self.role, self.weight.map(f32::to_bits), self.italic, self.mono, self.underline, self.strikethrough).hash(h);
        for c in self.color.iter().chain(self.background.iter()) {
            for v in [c.r, c.g, c.b, c.a] {
                h.write_u32(v.to_bits());
            }
        }
        self.size.map(f64::to_bits).hash(h);
        self.alpha.map(f32::to_bits).hash(h);
    }

    /// Its style over `base`.
    pub(crate) fn style(&self, base: &TextSpec, theme: &Theme) -> TextStyle {
        let mut font = base.font;
        if self.mono {
            font.family = Family::Mono;
        }
        if let Some(w) = self.weight {
            font.weight = w;
        }
        if self.italic {
            font.italic = true;
        }
        if let Some(s) = self.size {
            font.size = s;
        }
        let color = self.color.or(self.role.map(|r| theme.role(r))).unwrap_or(base.color);
        let color = match self.alpha {
            Some(a) => color.with_alpha(color.a * a),
            None => color,
        };
        let mut s = TextStyle::new(theme.font(font), color);
        s.underline = self.underline || base.underline;
        s.strikethrough = self.strikethrough || base.strikethrough;
        s.background = self.background;
        s
    }
}

/// Text laid out, by what it was laid out from.
///
/// A layout that something else still holds (the widget showing it) stays
/// however many there are: letting it go would free nothing, and whoever
/// shows it asks again at the next build. Layouts nothing holds any more
/// go least recently used first, once there are too many of those. So a
/// window keeps every label it shows, and scrolling or typing leaves a
/// bounded trail behind.
pub struct TextCache {
    map: FxHashMap<u64, (TextLayout, u64)>,
    tick: u64,
    /// How many layouts nothing else holds are kept, at most.
    capacity: usize,
    /// How many layouts there are before the next look for ones to let go.
    limit: usize,
    pub(crate) hits: u64,
    pub(crate) misses: u64,
}

/// Layouts nothing shows that a cache keeps, for text shown again.
const KEPT_UNSHOWN: usize = 4096;

impl Default for TextCache {
    fn default() -> TextCache {
        TextCache::with_capacity(KEPT_UNSHOWN)
    }
}

/// The modifier glyphs, set in Noto Sans Symbols 2 wherever they show.
pub(crate) fn is_symbol(c: char) -> bool {
    matches!(c, '❖' | '⇧' | '⌥' | '⌃' | '⌘' | '⎋' | '⌫' | '⌦' | '⇥' | '↩' | '⏎')
}

/// What [`TextCache::layout`] keeps a layout under.
fn text_key(text: &str, spec: &TextSpec, width: Option<f64>) -> u64 {
    let mut h = Fx::default();
    text.hash(&mut h);
    spec.hash_into(&mut h);
    width.map(f64::to_bits).hash(&mut h);
    h.finish()
}

/// Text's layout, to build.
fn text_builder(theme: &Theme, text: &str, spec: &TextSpec, width: Option<f64>) -> sidestep_ui::TextLayoutBuilder {
    let mut b = TextLayout::builder(text, &spec.style(theme))
        .width(width)
        .wrap(spec.wrap)
        .alignment(spec.align)
        .max_lines(spec.max_lines)
        .ellipsis(spec.max_lines > 0)
        .line_height_multiple(spec.line_height);
    for r in symbol_runs(text) {
        b = b.style(r, symbol_style(spec, theme));
    }
    b
}

impl TextCache {
    /// A cache that keeps at most `unshown` layouts nothing else holds.
    pub fn with_capacity(unshown: usize) -> TextCache {
        let capacity = unshown.max(4);
        TextCache { map: FxHashMap::default(), tick: 0, capacity, limit: capacity, hits: 0, misses: 0 }
    }

    /// `text` set as `spec`, broken at `width` (none: unbounded).
    pub fn layout(&mut self, theme: &Theme, text: &str, spec: &TextSpec, width: Option<f64>) -> TextLayout {
        self.get(text_key(text, spec, width), || text_builder(theme, text, spec, width).build())
    }

    /// Rich text: `spans` over `base`.
    pub fn layout_spans(&mut self, theme: &Theme, spans: &[Span], base: &TextSpec, width: Option<f64>) -> TextLayout {
        let key = spans_key(spans, base, width);
        self.get(key, || spans_builder(theme, spans, base, width).build())
    }

    /// Lay out together each of `texts` (rich text, as
    /// [`layout_spans`](Self::layout_spans) takes it) that isn't kept, for
    /// the calls after to find: Sidestep shares the shaping out to its text
    /// threads when there's enough of it. A view about to show many lines
    /// nothing has shown (an editor after a jump) asks for them first.
    pub fn prefetch_spans(&mut self, theme: &Theme, texts: &[(Vec<Span>, TextSpec, Option<f64>)]) {
        let mut keys = Vec::new();
        let mut builders = Vec::new();
        for (spans, base, width) in texts {
            let key = spans_key(spans, base, *width);
            if self.map.contains_key(&key) || keys.contains(&key) {
                continue;
            }
            keys.push(key);
            builders.push(spans_builder(theme, spans, base, *width));
        }
        self.build_together(keys, builders);
    }

    /// Lay `builders` out together and keep each under its key, if there
    /// are enough of them for that to be worth it.
    fn build_together(&mut self, keys: Vec<u64>, builders: Vec<sidestep_ui::TextLayoutBuilder>) {
        if builders.len() < 2 {
            return;
        }
        for (key, layout) in keys.into_iter().zip(sidestep_ui::TextLayoutBuilder::build_all(builders)) {
            self.tick += 1;
            self.misses += 1;
            self.insert(key, layout);
        }
    }

    /// A layout by a key of the caller's, made by `make` if it isn't kept.
    pub fn get(&mut self, key: u64, make: impl FnOnce() -> TextLayout) -> TextLayout {
        if let Some(layout) = self.find(key) {
            return layout;
        }
        self.tick += 1;
        self.misses += 1;
        let layout = make();
        self.insert(key, layout.clone());
        layout
    }

    /// The layout kept under `key`, if there is one.
    fn find(&mut self, key: u64) -> Option<TextLayout> {
        let (layout, used) = self.map.get_mut(&key)?;
        self.tick += 1;
        *used = self.tick;
        self.hits += 1;
        Some(layout.clone())
    }

    fn insert(&mut self, key: u64, layout: TextLayout) {
        if self.map.len() >= self.limit {
            self.evict();
        }
        self.map.insert(key, (layout, self.tick));
    }

    /// Let the least recently used of the layouts nothing else holds go,
    /// down to three quarters of how many are kept, and look again once a
    /// quarter more have come: a look reads every layout, so it mustn't
    /// come with each new one.
    fn evict(&mut self) {
        let keep = self.capacity - self.capacity / 4;
        let mut ticks: Vec<u64> = self.map.values().filter(|(l, _)| !l.is_shared()).map(|(_, t)| *t).collect();
        if ticks.len() > keep {
            let cut = ticks.len() - keep;
            let (_, &mut newest_gone, _) = ticks.select_nth_unstable(cut - 1);
            self.map.retain(|_, (l, t)| *t > newest_gone || l.is_shared());
        }
        self.limit = self.capacity.max(self.map.len() + self.capacity / 4);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.limit = self.capacity;
    }
}

/// What [`TextCache::layout_spans`] keeps a layout under.
fn spans_key(spans: &[Span], base: &TextSpec, width: Option<f64>) -> u64 {
    let mut h = Fx::default();
    for s in spans {
        s.hash_into(&mut h);
    }
    base.hash_into(&mut h);
    width.map(f64::to_bits).hash(&mut h);
    h.finish()
}

/// Rich text's layout, to build.
fn spans_builder(theme: &Theme, spans: &[Span], base: &TextSpec, width: Option<f64>) -> sidestep_ui::TextLayoutBuilder {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let mut b = TextLayout::builder(&text, &base.style(theme))
        .width(width)
        .wrap(base.wrap)
        .alignment(base.align)
        .max_lines(base.max_lines)
        .ellipsis(base.max_lines > 0)
        .line_height_multiple(base.line_height);
    let mut at = 0;
    for s in spans {
        let end = at + s.text.len();
        b = b.style(at..end, s.style(base, theme));
        at = end;
    }
    for r in symbol_runs(&text) {
        b = b.style(r, symbol_style(base, theme));
    }
    b
}

fn symbol_runs(text: &str) -> Vec<Range<usize>> {
    if !text.chars().any(is_symbol) {
        return Vec::new();
    }
    text.char_indices().filter(|(_, c)| is_symbol(*c)).map(|(i, c)| i..i + c.len_utf8()).collect()
}

fn symbol_style(spec: &TextSpec, theme: &Theme) -> TextStyle {
    let mut font = spec.font;
    font.family = Family::Symbols;
    font.weight = 400.0;
    font.italic = false;
    TextStyle::new(theme.font(font), spec.color)
}

/// A role's colour in a selected row: text takes `on_selection`, quieter
/// text a little less of it; colours that mean something keep theirs.
pub(crate) fn selected_color(theme: &Theme, role: Role) -> Color {
    match role {
        Role::Text | Role::Text2 | Role::Accent => theme.on_selection(),
        Role::Text3 | Role::Faint => theme.on_selection().with_alpha(0.72),
        other => theme.role(other),
    }
}

// The `text` element.

/// Text in one style: a label, a paragraph.
pub struct Text<Msg> {
    pub(crate) common: Common,
    content: Arc<str>,
    role: Role,
    color: Option<Color>,
    font: Option<FontSpec>,
    size: Option<f64>,
    weight: Option<f32>,
    mono: bool,
    italic: bool,
    tabular: bool,
    condensed: bool,
    wrap: bool,
    max_lines: usize,
    align: Alignment,
    line_height: f64,
    underline: bool,
    strikethrough: bool,
    letter_spacing: f64,
    uppercase: bool,
    selectable: bool,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

crate::builder!(Text);

/// `s` at most `most` characters long as people count them (a flag, an
/// emoji with its skin tone or a letter with its accents is one), the last
/// an ellipsis if any were left out: for text with no width to fit, such
/// as a label for the shell's menus. The positive limit includes the ellipsis.
pub fn clip(s: &str, most: NonZeroUsize) -> std::borrow::Cow<'_, str> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut graphemes = s.grapheme_indices(true);
    let Some((end, _)) = graphemes.nth(most.get() - 1) else {
        return s.into();
    };
    if graphemes.next().is_none() {
        return s.into();
    }
    format!("{}…", s[..end].trim_end()).into()
}

/// Text: `text("Hello").size(15.0).weight(600.0)`. One line, ending in an
/// ellipsis where it doesn't fit, unless it [`wrap`](Text::wrap)s.
pub fn text<Msg>(content: impl Into<Arc<str>>) -> Text<Msg> {
    Text {
        common: Common::default(),
        content: content.into(),
        role: Role::Text,
        color: None,
        font: None,
        size: None,
        weight: None,
        mono: false,
        italic: false,
        tabular: false,
        condensed: false,
        wrap: false,
        max_lines: 0,
        align: Alignment::Start,
        line_height: 0.0,
        underline: false,
        strikethrough: false,
        letter_spacing: 0.0,
        uppercase: false,
        selectable: false,
        _m: std::marker::PhantomData,
    }
}

impl<Msg> Text<Msg> {
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// A colour outside the roles (a chart's series).
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn size(mut self, size: f64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn weight(mut self, weight: f32) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn bold(self) -> Self {
        self.weight(aizome_theme::typeface::WEIGHT_BOLD)
    }

    pub fn semibold(self) -> Self {
        self.weight(aizome_theme::typeface::WEIGHT_SEMIBOLD)
    }

    /// IBM Plex Mono.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Digits of one width, for numbers that change in place.
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }

    /// Archivo at 87.5% width.
    pub fn condensed(mut self) -> Self {
        self.condensed = true;
        self
    }

    /// A whole font of the caller's.
    pub fn font(mut self, font: FontSpec) -> Self {
        self.font = Some(font);
        self
    }

    /// Break lines at the width given.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    /// At most `n` lines (wrapping), the last ending in an ellipsis.
    pub fn max_lines(mut self, n: usize) -> Self {
        self.wrap = true;
        self.max_lines = n;
        self
    }

    pub fn align(mut self, align: Alignment) -> Self {
        self.align = align;
        self
    }

    pub fn center(self) -> Self {
        self.align(Alignment::Center)
    }

    pub fn line_height(mut self, multiple: f64) -> Self {
        self.line_height = multiple;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn letter_spacing(mut self, points: f64) -> Self {
        self.letter_spacing = points;
        self
    }

    /// Small caps style captions: upper case, spaced.
    pub fn caps(mut self) -> Self {
        self.uppercase = true;
        self
    }

    /// The user can select and copy it.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }
}

impl<Msg> From<Text<Msg>> for Element<Msg> {
    fn from(t: Text<Msg>) -> Element<Msg> {
        let w = TextWidget {
            content: if t.uppercase { t.content.to_uppercase().into() } else { t.content },
            role: t.role,
            color: t.color,
            font: t.font,
            size: t.size,
            weight: t.weight,
            mono: t.mono,
            italic: t.italic,
            tabular: t.tabular,
            condensed: t.condensed,
            wrap: t.wrap,
            max_lines: t.max_lines,
            align: t.align,
            line_height: t.line_height,
            underline: t.underline,
            strikethrough: t.strikethrough,
            letter_spacing: t.letter_spacing,
            selectable: t.selectable,
            key: None,
            laid: None,
        };
        Element::from_raw(RawElement::new(w, t.common, Vec::new()))
    }
}

impl<Msg> From<&str> for Element<Msg> {
    fn from(s: &str) -> Element<Msg> {
        text(s).into()
    }
}

impl<Msg> From<String> for Element<Msg> {
    fn from(s: String) -> Element<Msg> {
        text(s).into()
    }
}

pub(crate) struct TextWidget {
    content: Arc<str>,
    role: Role,
    color: Option<Color>,
    font: Option<FontSpec>,
    size: Option<f64>,
    weight: Option<f32>,
    mono: bool,
    italic: bool,
    tabular: bool,
    condensed: bool,
    wrap: bool,
    max_lines: usize,
    align: Alignment,
    line_height: f64,
    underline: bool,
    strikethrough: bool,
    letter_spacing: f64,
    selectable: bool,
    /// What its text and the way it's set are kept under in the cache,
    /// once it's laid out.
    key: Option<u64>,
    laid: Option<TextLayout>,
}

impl TextWidget {
    fn spec(&self, theme: &Theme, selected: bool) -> TextSpec {
        let m = &theme.metrics;
        let mut font = self.font.unwrap_or_else(|| {
            let size = self.size.unwrap_or(m.body);
            if self.mono { FontSpec::mono(size) } else { FontSpec::ui(size) }
        });
        if let Some(w) = self.weight {
            font.weight = w;
        }
        font.italic |= self.italic;
        font.tabular |= self.tabular;
        font.condensed |= self.condensed;
        let color = match self.color {
            Some(c) => c,
            None if selected => selected_color(theme, self.role),
            None => theme.role(self.role),
        };
        let mut spec = TextSpec::new(font, color);
        // One line, or a paragraph, or at most `max_lines` of one, the last
        // ending in an ellipsis when text is left out.
        spec.wrap = if self.wrap && self.max_lines != 1 { Wrap::Words } else { Wrap::Truncate };
        spec.max_lines = self.max_lines;
        spec.align = self.align;
        spec.line_height = self.line_height;
        spec.underline = self.underline;
        spec.strikethrough = self.strikethrough;
        spec.letter_spacing = self.letter_spacing;
        spec
    }
}

/// Labels a look ahead lays out together, at most.
const AHEAD: usize = 512;

/// Labels in a row found kept before a look ahead stops.
const KEPT_AHEAD: usize = 8;

/// `text` set as `spec` is a label nothing has shown, and where there's
/// one there are often many: a window opening, a list whose every row
/// changed. Sidestep lays texts out together on its text threads, so this
/// looks at the labels mounted after this one and lays out with it the
/// ones that are new too, for their own layouts to find. A label alone in
/// being new finds the next few kept and stops there, and one that was
/// kept never looks.
fn lay_out_ahead(cx: &mut LayoutCx, key: u64, text: &str, spec: &TextSpec) {
    let (theme, selected) = (cx.env.theme, cx.env.on_selection);
    let mut keys = vec![key];
    let mut builders = vec![text_builder(theme, text, spec, None)];
    let mut kept = 0;
    for node in cx.tree.nodes[cx.node + 1..].iter().filter(|n| !n.dead) {
        let widget: &dyn std::any::Any = node.widget.as_ref();
        let Some(label) = widget.downcast_ref::<TextWidget>() else { continue };
        // What its own layout asks for, if it's on a selected row or not
        // as this one is; if it isn't, it lays itself out when it comes.
        let spec = label.spec(theme, selected);
        let key = text_key(&label.content, &spec, None);
        if cx.env.text.map.contains_key(&key) || keys.contains(&key) {
            kept += 1;
            if kept == KEPT_AHEAD {
                break;
            }
            continue;
        }
        kept = 0;
        keys.push(key);
        builders.push(text_builder(theme, &label.content, &spec, None));
        if builders.len() == AHEAD {
            break;
        }
    }
    cx.env.text.build_together(keys, builders);
}

impl Widget for TextWidget {
    fn name(&self) -> &'static str {
        "text"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let spec = self.spec(cx.theme(), cx.on_selection());
        let theme = cx.env.theme;
        // A line laid out unbounded tells the natural width; text that
        // doesn't fit is laid out again at the width it has.
        let key = text_key(&self.content, &spec, None);
        let natural = match cx.env.text.find(key) {
            Some(kept) => kept,
            None => {
                lay_out_ahead(cx, key, &self.content, &spec);
                cx.env.text.get(key, || text_builder(theme, &self.content, &spec, None).build())
            }
        };
        let nat = natural.size();
        let fits = nat.width <= bc.max.width + 0.01;
        let aligned = spec.align != Alignment::Start && spec.align != Alignment::Left;
        let (mut laid, mut s) = (natural, nat);
        if !fits || (aligned && bc.max.width.is_finite()) || (self.wrap && self.max_lines > 0) {
            let w = if aligned { bc.max.width.max(bc.min.width) } else { bc.max.width };
            if w.is_finite() {
                laid = cx.env.text.layout(theme, &self.content, &spec, Some(w.max(1.0)));
                s = laid.size();
            }
        }
        let height = if self.content.is_empty() { nat.height.max(spec.font.size * 1.2) } else { s.height };
        let width = if aligned { bc.max.width.min(s.width.max(bc.min.width)) } else { s.width.ceil() };
        self.key = Some(key);
        self.laid = Some(laid);
        bc.constrain(Size::new(width, height.ceil()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        if let Some(l) = &self.laid {
            // Centre the lines in what the parent gave beyond their height.
            let y = ((cx.size().height - l.size().height) / 2.0).max(0.0).floor();
            cx.canvas.draw_text(l, Point::new(0.0, y));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        // Once laid out, what its text and the way it's set were kept
        // under stands for both.
        match self.key {
            Some(key) => h.add(&key),
            None => h.str(&self.content),
        };
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Label, label: Some(self.content.to_string()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.content)
    }

    fn hit(&self) -> bool {
        self.selectable
    }
}

/// Text in runs of their own look: `rich_text([Span::new("Ln ").role(Role::Text3), Span::new("48").mono()])`.
pub struct RichText<Msg> {
    pub(crate) common: Common,
    spans: Vec<Span>,
    base: Option<FontSpec>,
    role: Role,
    wrap: bool,
    max_lines: usize,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

crate::builder!(RichText);

pub fn rich_text<Msg>(spans: impl IntoIterator<Item = Span>) -> RichText<Msg> {
    RichText {
        common: Common::default(),
        spans: spans.into_iter().collect(),
        base: None,
        role: Role::Text,
        wrap: false,
        max_lines: 0,
        _m: std::marker::PhantomData,
    }
}

impl<Msg> RichText<Msg> {
    pub fn font(mut self, font: FontSpec) -> Self {
        self.base = Some(font);
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    pub fn max_lines(mut self, n: usize) -> Self {
        self.wrap = true;
        self.max_lines = n;
        self
    }
}

impl<Msg> From<RichText<Msg>> for Element<Msg> {
    fn from(t: RichText<Msg>) -> Element<Msg> {
        let w =
            RichWidget { spans: t.spans, base: t.base, role: t.role, wrap: t.wrap, max_lines: t.max_lines, laid: None };
        let mut common = t.common;
        if common.width == Length::Shrink && t.wrap {
            common.width = Length::Fill;
        }
        Element::from_raw(RawElement::new(w, common, Vec::new()))
    }
}

struct RichWidget {
    spans: Vec<Span>,
    base: Option<FontSpec>,
    role: Role,
    wrap: bool,
    max_lines: usize,
    laid: Option<TextLayout>,
}

impl Widget for RichWidget {
    fn name(&self) -> &'static str {
        "rich-text"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let theme = cx.env.theme;
        let font = self.base.unwrap_or(FontSpec::ui(theme.metrics.body));
        let mut spec = TextSpec::new(font, theme.role(self.role));
        spec.wrap = if self.wrap && self.max_lines != 1 { Wrap::Words } else { Wrap::Truncate };
        spec.max_lines = self.max_lines;
        let width = bc.max.width.is_finite().then_some(bc.max.width.max(1.0));
        let laid = cx.env.text.layout_spans(theme, &self.spans, &spec, width);
        let s = laid.size();
        self.laid = Some(laid);
        bc.constrain(Size::new(s.width.ceil(), s.height.ceil()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        if let Some(l) = &self.laid {
            let y = ((cx.size().height - l.size().height) / 2.0).max(0.0).floor();
            cx.canvas.draw_text(l, Point::new(0.0, y));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for s in &self.spans {
            s.hash_into(h.0);
        }
        h.add(&(self.role, self.wrap, self.max_lines));
    }

    fn semantics(&self) -> Option<Semantics> {
        let label: String = self.spans.iter().map(|s| s.text.as_str()).collect();
        Some(Semantics { role: SemanticRole::Label, label: Some(label), ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Appearance, Theme};

    #[test]
    fn a_paragraph_cut_at_its_line_limit_ends_in_an_ellipsis() {
        let theme = Theme::new(Appearance::default());
        let mut cache = TextCache::default();
        let text =
            "Operations stay in points. Each window renders at its output's scale: the integer one wherever it can.";
        let mut spec = TextSpec::new(FontSpec::ui(13.0), theme.text());
        spec.wrap = Wrap::Words;
        spec.max_lines = 2;
        let laid = cache.layout(&theme, text, &spec, Some(120.0));
        let lines = laid.lines();
        assert_eq!(lines.len(), 2);
        // The last line holds the rest, cut short by the ellipsis.
        assert_eq!(lines[1].range.end, text.len(), "{lines:?}");
    }

    #[test]
    fn layouts_something_shows_stay_and_the_rest_are_bounded() {
        let theme = Theme::new(Appearance::default());
        let mut cache = TextCache::with_capacity(8);
        let spec = TextSpec::new(FontSpec::ui(13.0), theme.text());
        // Sixty labels a window shows, held as their widgets hold them:
        // far more than the cache keeps of layouts nothing shows.
        let shown: Vec<TextLayout> = (0..60).map(|i| cache.layout(&theme, &format!("row {i}"), &spec, None)).collect();
        assert_eq!(cache.len(), 60);
        // Text that comes and goes (a counter) leaves a bounded trail,
        // and takes none of the shown ones with it.
        for i in 0..500 {
            cache.layout(&theme, &format!("tick {i}"), &spec, None);
        }
        assert!(cache.len() <= 60 + 8, "{} kept", cache.len());
        let made = cache.misses;
        for i in 0..60 {
            cache.layout(&theme, &format!("row {i}"), &spec, None);
        }
        assert_eq!(cache.misses, made, "what shows was not laid out again");
        // The most recent of the rest are the ones kept.
        cache.layout(&theme, "tick 499", &spec, None);
        assert_eq!(cache.misses, made);
        // Let go of, the labels go as the rest do.
        drop(shown);
        for i in 500..600 {
            cache.layout(&theme, &format!("tick {i}"), &spec, None);
        }
        assert!(cache.len() <= 8, "{} kept", cache.len());
    }

    #[test]
    fn clipping_keeps_whole_characters_and_says_so() {
        assert!(matches!(clip("Kirby", NonZeroUsize::new(5).unwrap()), std::borrow::Cow::Borrowed("Kirby")));
        assert_eq!(clip(&"a".repeat(49), NonZeroUsize::new(48).unwrap()), format!("{}…", "a".repeat(47)));
        // A family is one character, and so is a letter with its accent.
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        assert_eq!(clip(&format!("ab{family}cd"), NonZeroUsize::new(4).unwrap()), format!("ab{family}…"));
        assert_eq!(clip(&"e\u{301}".repeat(5), NonZeroUsize::new(3).unwrap()), "e\u{301}e\u{301}…");
        assert_eq!(clip("🇯🇵🇫🇷🇩🇪", NonZeroUsize::new(2).unwrap()), "🇯🇵…");
        // No space before the ellipsis.
        assert_eq!(clip("one two three", NonZeroUsize::new(5).unwrap()), "one…");
    }

    #[test]
    fn clipping_to_one_grapheme_keeps_short_text_or_only_the_ellipsis() {
        let one = NonZeroUsize::MIN;
        assert!(matches!(clip("", one), std::borrow::Cow::Borrowed("")));
        assert!(matches!(clip("🇯🇵", one), std::borrow::Cow::Borrowed("🇯🇵")));
        assert_eq!(clip("🇯🇵🇫🇷", one), "…");
    }
}
