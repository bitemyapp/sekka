//! The palette as it draws (Strip's structure, Aizome's look): the query
//! row, the scope chips, results under mono heads beside a detail pane,
//! and the key hints along the bottom.

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect};
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Layoutable, column, row, stack};
use aizome_search::Scope;
use aizome_shell_proto as proto;
use aizome_theme::typeface;

use super::model::{Row, Section};
use crate::look::{self, Hue};
use crate::widgets::drawn::{around, drawn};
use crate::widgets::field::field;

pub const WIDTH: f64 = 720.0;
pub const HEIGHT: f64 = 480.0;
pub const SHADOW: f64 = 3.0;
const QUERY: f64 = 58.0;
const CHIPS: f64 = 45.0;
const FOOTER: f64 = 38.0;
const ROW: f64 = 38.0;
const HEAD: f64 = 31.0;
const DETAIL: f64 = 260.0;

/// What the palette's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum PaletteMsg {
    Query(String),
    Scope(Option<Scope>),
    /// A row clicked: select it, and run it on a second click.
    Click(usize),
    Run {
        keep_open: bool,
    },
    Close,
}

/// What the detail pane shows for the selected row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub caption: String,
    pub title: String,
    pub path: String,
    pub keys: Vec<String>,
    pub coach: Option<String>,
    pub actions: Vec<(String, String)>,
}

/// Everything the palette shows.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteView {
    pub query: String,
    pub scope: Option<Scope>,
    /// The scope chips after All: what the palette finds.
    pub scopes: Vec<Scope>,
    /// "in Editor · damage.rs".
    pub context: String,
    pub sections: Vec<Section>,
    pub selected: usize,
    pub detail: Option<Detail>,
}

/// The body's height for rows.
pub fn body_height() -> f64 {
    HEIGHT - QUERY - CHIPS - FOOTER - 2.0
}

/// Where row `index` (of all sections' rows) is in the body, before
/// scrolling; and the body's whole height.
pub fn row_tops(sections: &[Section]) -> (Vec<f64>, f64) {
    let mut y = 0.0;
    let mut tops = Vec::new();
    for s in sections {
        y += HEAD;
        for _ in &s.rows {
            tops.push(y);
            y += ROW;
        }
    }
    (tops, y + 8.0)
}

/// How far the results scroll so the selected row shows.
pub fn scroll_for(sections: &[Section], selected: usize, current: f64) -> f64 {
    let (tops, total) = row_tops(sections);
    let room = body_height();
    let Some(&top) = tops.get(selected) else { return 0.0 };
    let mut s = current;
    // Its section's head shows with the first row.
    let want_top = if tops.first() == Some(&top)
        || tops.iter().position(|t| *t == top).is_some_and(|i| i > 0 && tops[i - 1] + ROW < top)
    {
        top - HEAD
    } else {
        top
    };
    if want_top < s {
        s = want_top;
    }
    if top + ROW > s + room {
        s = top + ROW - room;
    }
    s.clamp(0.0, (total - room).max(0.0))
}

fn query_row(v: &PaletteView) -> Element<PaletteMsg> {
    let prompt = drawn(26.0, QUERY - 1.0, "prompt", |cx| {
        let t = cx.theme();
        let s = look::mono(20.0, typeface::WEIGHT_REGULAR, t.focus());
        cx.draw_text_in("❯", &s, cx.bounds(), Align::Start);
    });
    let q = field(&v.query, |t| look::ui(20.0 * t.metrics.factor, typeface::WEIGHT_MEDIUM, t.text()))
        .placeholder("Commands, windows, files, apps, settings…")
        .height(QUERY - 1.0)
        .on_change(PaletteMsg::Query)
        .on_submit(PaletteMsg::Run { keep_open: false });
    let ctx = v.context.clone();
    let context = drawn(0.0, QUERY - 1.0, ctx.clone(), move |cx| {
        let t = cx.theme();
        let s = look::mono(11.5, typeface::WEIGHT_REGULAR, t.text3());
        cx.draw_text_in(&ctx, &s, cx.bounds(), Align::End);
    })
    .fit({
        let ctx = v.context.clone();
        move |cx| {
            let t = cx.theme().clone();
            cx.measure(&ctx, &look::mono(11.5, typeface::WEIGHT_REGULAR, t.text3())).width.min(260.0)
        }
    });
    let r =
        row(vec![prompt.into(), Element::from(q).id("palette-query").width(aizome_kit::Length::Fill), context.into()])
            .spacing(12.0);
    around(r, [0.0, 20.0, 1.0, 20.0], aizome_kit::Size::new(WIDTH, QUERY), "query-row", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(Rect::new(0.0, b.y1 - 1.0, b.x1, b.y1), t.line());
    })
    .into()
}

fn chip(label: &'static str, scope: Option<Scope>, on: bool) -> Element<PaletteMsg> {
    drawn(0.0, 24.0, (label, on), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let fg = if on {
            cx.canvas.fill_rect(b, t.selection());
            t.on_selection()
        } else {
            if cx.is_hovered() {
                cx.canvas.fill_rect(b, t.hover(t.raised()));
            }
            aizome_kit::draw::border(cx.canvas, b, t.line(), 1.0);
            t.text3()
        };
        let s = look::ui(
            12.5 * t.metrics.factor,
            if on { typeface::WEIGHT_SEMIBOLD } else { typeface::WEIGHT_REGULAR },
            fg,
        );
        cx.draw_text_in(label, &s, b, Align::Center);
    })
    .fit(move |cx| {
        let t = cx.theme().clone();
        cx.measure(label, &look::ui(12.5 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, t.text3())).width + 20.0
    })
    .on_click(PaletteMsg::Scope(scope))
    .text(label, SemanticRole::Tab)
    .key(("chip", label))
    .into()
}

fn chips(v: &PaletteView) -> Element<PaletteMsg> {
    let mut items: Vec<Element<PaletteMsg>> = vec![chip("All", None, v.scope.is_none())];
    for &s in &v.scopes {
        items.push(chip(s.label(), Some(s), v.scope == Some(s)));
    }
    items.push(aizome_kit::spacer().into());
    items.push(
        drawn(90.0, 24.0, "next-scope", |cx| {
            let t = cx.theme();
            let s = look::mono(11.0, typeface::WEIGHT_REGULAR, t.faint());
            cx.draw_text_in("⇥ next scope", &s, cx.bounds(), Align::End);
        })
        .into(),
    );
    around(
        row(items).spacing(6.0).align(Align::Center),
        [10.0, 16.0, 11.0, 16.0],
        aizome_kit::Size::new(WIDTH, CHIPS),
        "chips",
        |cx| {
            let t = cx.theme();
            let b = cx.bounds();
            cx.canvas.fill_rect(Rect::new(0.0, b.y1 - 1.0, b.x1, b.y1), t.line());
        },
    )
    .into()
}

fn head(s: &Section) -> Element<PaletteMsg> {
    let (h, sub) = (s.head.clone(), s.sub.clone());
    drawn(0.0, HEAD, (h.clone(), sub.clone()), move |cx| {
        let t = cx.theme();
        let mut spec = look::mono(10.5, typeface::WEIGHT_BOLD, t.text3());
        spec.letter_spacing = 10.5 * 0.14;
        let r = Rect::new(16.0, 12.0, cx.bounds().x1, HEAD - 3.0);
        let w = cx.draw_text_in(&h, &spec, r, Align::Start);
        if let Some(sub) = &sub {
            let mut s2 = look::mono(10.5, typeface::WEIGHT_MEDIUM, t.faint());
            s2.letter_spacing = 10.5 * 0.06;
            cx.draw_text_in(sub, &s2, Rect::new(16.0 + w + 10.0, 12.0, cx.bounds().x1, HEAD - 3.0), Align::Start);
        }
    })
    .text(s.head.clone(), SemanticRole::Label)
    .into()
}

fn shortcut_label(s: &proto::Shortcut) -> String {
    s.label()
}

fn result(r: &Row, index: usize, on: bool) -> Element<PaletteMsg> {
    let item = r.item.clone();
    let ranges = r.ranges.clone();
    let key = (item.title.clone(), item.trail.clone(), on, ranges.clone());
    let label = item.title.clone();
    drawn(0.0, ROW, key, move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if on {
            cx.canvas.fill_rect(b, t.selection());
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), t.focus());
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.raised()));
        }
        let (fg, dim) = if on { (t.on_selection(), t.on_selection().with_alpha(0.78)) } else { (t.text(), t.text3()) };
        let glyph_c = if on { t.focus() } else { t.text3() };
        item.glyph.draw(cx.canvas, Point::new(16.0, ((ROW - 17.0) / 2.0).round()), 17.0, glyph_c);
        // The title with its matched characters in blue, underlined.
        let mut base = if item.mono {
            look::mono(13.0, typeface::WEIGHT_REGULAR, fg)
        } else {
            look::ui(14.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, fg)
        };
        base.wrap = aizome_kit::sidestep_ui::Wrap::None;
        let blue = if on { Hue::Focus.color(t) } else { Hue::Blue.color(t) };
        let mut spans = Vec::new();
        let mut at = 0;
        for rg in &ranges {
            if rg.start > at {
                spans.push(aizome_kit::Span::new(&item.title[at..rg.start]));
            }
            spans.push(
                aizome_kit::Span::new(&item.title[rg.start..rg.end])
                    .color(blue)
                    .underline()
                    .weight(typeface::WEIGHT_BOLD),
            );
            at = rg.end;
        }
        if at < item.title.len() {
            spans.push(aizome_kit::Span::new(&item.title[at..]));
        }
        let x = 45.0;
        let keys_w = item.shortcut.as_ref().map_or(0.0, |_| 64.0);
        let title_l = cx.text_cache().layout_spans(t, &spans, &base, None);
        let tw = title_l.size().width.min(b.x1 - x - keys_w - 40.0);
        let y = ((ROW - title_l.size().height) / 2.0).round();
        cx.canvas.save();
        cx.canvas.clip_rect(Rect::new(x, 0.0, x + tw, ROW));
        cx.canvas.draw_text(&title_l, Point::new(x, y));
        cx.canvas.restore();
        if !item.trail.is_empty() {
            let ts = if item.scope == Scope::File {
                look::mono(11.5, typeface::WEIGHT_REGULAR, dim)
            } else {
                look::ui(12.5 * t.metrics.factor, typeface::WEIGHT_REGULAR, dim)
            };
            cx.draw_text_in(&item.trail, &ts, Rect::new(x + tw + 10.0, 0.0, b.x1 - 16.0 - keys_w, ROW), Align::Start);
        }
        if let Some(s) = &item.shortcut {
            let ks = look::mono(12.0, typeface::WEIGHT_REGULAR, dim);
            cx.draw_text_in(
                &shortcut_label(s),
                &ks,
                Rect::new(b.x1 - 16.0 - keys_w, 0.0, b.x1 - 16.0, ROW),
                Align::End,
            );
        }
    })
    .on_click(PaletteMsg::Click(index))
    .text(label, SemanticRole::ListItem)
    .key(("row", index))
    .into()
}

fn results(v: &PaletteView, scroll: f64) -> Element<PaletteMsg> {
    let mut items: Vec<Element<PaletteMsg>> = Vec::new();
    let mut index = 0;
    let mut y = 0.0;
    let room = body_height();
    for s in &v.sections {
        let visible = |y: f64, h: f64| y + h > scroll && y < scroll + room;
        if visible(y, HEAD) {
            items.push(head(s));
        }
        y += HEAD;
        for r in &s.rows {
            if visible(y, ROW) {
                items.push(result(r, index, index == v.selected));
            }
            y += ROW;
            index += 1;
        }
    }
    if v.sections.is_empty() {
        items.push(
            drawn(0.0, 80.0, "nothing", |cx| {
                let t = cx.theme();
                let s = look::ui(13.0, typeface::WEIGHT_REGULAR, t.text3());
                cx.draw_text_in("Nothing matches.", &s, Rect::new(16.0, 0.0, cx.bounds().x1, 80.0), Align::Start);
            })
            .into(),
        );
    }
    // Only what shows is built; the first element starts where the
    // scrolled-to content does.
    let offset = {
        let mut first_y = 0.0;
        let mut y = 0.0;
        'outer: for s in &v.sections {
            if y + HEAD > scroll {
                first_y = y;
                break 'outer;
            }
            y += HEAD;
            for _ in &s.rows {
                if y + ROW > scroll {
                    first_y = y;
                    break 'outer;
                }
                y += ROW;
            }
        }
        first_y - scroll
    };
    let list = column(items);
    aizome_kit::container(list)
        .padding([offset, 0.0, 0.0, 0.0])
        .clip()
        .width(aizome_kit::Length::Fill)
        .height(room)
        .into()
}

fn detail(d: &Option<Detail>) -> Element<PaletteMsg> {
    let d = d.clone().unwrap_or_default();
    let label = d.title.clone();
    drawn(DETAIL, body_height(), format!("{d:?}"), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        aizome_kit::draw::frost(cx.canvas, t, b, t.sunk());
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, 1.0, b.y1), t.line());
        let (x0, x1) = (20.0, b.x1 - 20.0);
        let mut y = 22.0;
        let mut cap = look::mono(10.5, typeface::WEIGHT_BOLD, t.text3());
        cap.letter_spacing = 10.5 * 0.14;
        cx.draw_text_in(&d.caption, &cap, Rect::new(x0, y, x1, y + 14.0), Align::Start);
        y += 14.0 + 14.0;
        let mut title = look::ui(20.0 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, t.text());
        title.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        title.max_lines = 2;
        let tl = cx.layout_text(&d.title, &title, Some(x1 - x0));
        cx.canvas.draw_text(&tl, Point::new(x0, y));
        y += tl.size().height + 10.0;
        if !d.path.is_empty() {
            let ps = look::ui(13.0, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&d.path, &ps, Rect::new(x0, y, x1, y + 18.0), Align::Start);
            y += 18.0 + 14.0;
        }
        if !d.keys.is_empty() {
            let mut x = x0;
            let ks = look::mono(15.0, typeface::WEIGHT_REGULAR, t.text());
            for k in &d.keys {
                let w = (cx.layout_text(k, &ks, None).size().width + 16.0).max(30.0).ceil();
                let r = Rect::new(x, y, x + w, y + 30.0);
                look::keycap(cx.canvas, t, r);
                cx.draw_text_in(k, &ks, Rect::new(r.x0, r.y0, r.x1, r.y1 - 2.0), Align::Center);
                x += w + 6.0;
            }
            y += 30.0 + 14.0;
        }
        if let Some(c) = &d.coach {
            let mut cs = look::ui(13.0, typeface::WEIGHT_REGULAR, t.text2());
            cs.wrap = aizome_kit::sidestep_ui::Wrap::Words;
            cs.line_height = 1.5;
            let cl = cx.layout_text(c, &cs, Some(x1 - x0 - 24.0));
            let r = Rect::new(x0, y, x1, y + cl.size().height + 20.0);
            cx.canvas.fill_rect(r, t.raised());
            aizome_kit::draw::border(cx.canvas, r, t.line(), 1.0);
            cx.canvas.draw_text(&cl, Point::new(x0 + 12.0, y + 10.0));
        }
        // The actions, at the bottom.
        let n = d.actions.len() as f64;
        let mut ay = b.y1 - 22.0 - n * 22.0 + 8.0;
        for (label, keys) in &d.actions {
            let ls = look::ui(12.5, typeface::WEIGHT_REGULAR, t.text3());
            let ks = look::mono(12.5, typeface::WEIGHT_REGULAR, t.text2());
            cx.draw_text_in(label, &ls, Rect::new(x0, ay, x1, ay + 18.0), Align::Start);
            cx.draw_text_in(keys, &ks, Rect::new(x0, ay, x1, ay + 18.0), Align::End);
            ay += 22.0;
        }
    })
    .text(label, SemanticRole::Group)
    .into()
}

fn footer() -> Element<PaletteMsg> {
    drawn(WIDTH, FOOTER, "footer", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, b.x1, 1.0), t.line());
        let mut x = 18.0;
        for (k, label) in [("↑↓", "move"), ("↩", "run"), ("⇥", "scope"), ("❖K", "actions"), ("esc", "close")]
        {
            let ks = look::mono(12.0, typeface::WEIGHT_MEDIUM, t.text2());
            let ls = look::ui(12.0, typeface::WEIGHT_REGULAR, t.text3());
            let w = cx.draw_text_in(k, &ks, Rect::new(x, 1.0, b.x1, b.y1), Align::Start);
            x += w + 6.0;
            let w = cx.draw_text_in(label, &ls, Rect::new(x, 1.0, b.x1, b.y1), Align::Start);
            x += w + 18.0;
        }
        let ls = look::ui(12.0, typeface::WEIGHT_REGULAR, t.text3());
        cx.draw_text_in(
            "Searches every menu of the focused app",
            &ls,
            Rect::new(x, 1.0, b.x1 - 18.0, b.y1),
            Align::End,
        );
    })
    .into()
}

/// The palette, scrolled `scroll` points down its results.
pub fn view(v: &PaletteView, scroll: f64) -> Element<PaletteMsg> {
    let body = row(vec![results(v, scroll), detail(&v.detail)]).height(body_height());
    let content = column(vec![query_row(v), chips(v), body.into(), footer()]).width(WIDTH);
    let frame = around(content, [1.0, 1.0, 1.0, 1.0], aizome_kit::Size::new(WIDTH, HEIGHT), "palette-frame", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::panel(cx.canvas, t, b);
    });
    stack(vec![
        drawn(WIDTH + SHADOW, HEIGHT + SHADOW, "palette-shadow", |cx| {
            let t = cx.theme();
            look::shadow(cx.canvas, t, Rect::new(0.0, 0.0, WIDTH, HEIGHT));
        })
        .into(),
        frame.into(),
    ])
    .into()
}

/// The palette's surface size, with its shadow.
pub fn size() -> aizome_kit::Size {
    aizome_kit::Size::new(WIDTH + SHADOW, HEIGHT + SHADOW)
}

/// The rectangle of result `index` in the palette's window (to anchor its
/// actions' menu).
pub fn row_rect(sections: &[Section], index: usize, scroll: f64) -> Option<Rect> {
    let (tops, _) = row_tops(sections);
    let top = *tops.get(index)?;
    let y = 1.0 + QUERY + CHIPS + top - scroll;
    Some(Rect::new(1.0, y, WIDTH - DETAIL, y + ROW))
}

/// What the palette sets, in the sizes and weights it sets it in, to lay
/// out once before it first opens: the faces loaded and the shaping warm,
/// so its first frame isn't spent on them. `titles` are rows' titles and
/// paths to set too.
pub fn warm_texts(t: &aizome_kit::Theme, titles: &[String]) -> Vec<(String, aizome_kit::text::TextSpec)> {
    use aizome_theme::typeface as tf;
    let fg = t.text();
    let f = t.metrics.factor;
    let sample = "Commands, windows, files, apps, settings… ❯ ❖⇧⌥⌃ ↑↓ ↩ ⇥ esc › · 0123456789";
    let specs = [
        look::ui(20.0 * f, tf::WEIGHT_MEDIUM, fg),
        look::ui(20.0 * f, tf::WEIGHT_SEMIBOLD, fg),
        look::ui(14.0 * f, tf::WEIGHT_REGULAR, fg),
        look::ui(13.0, tf::WEIGHT_REGULAR, fg),
        look::ui(12.5 * f, tf::WEIGHT_REGULAR, fg),
        look::ui(12.5 * f, tf::WEIGHT_SEMIBOLD, fg),
        look::ui(12.0, tf::WEIGHT_REGULAR, fg),
        look::mono(20.0, tf::WEIGHT_REGULAR, fg),
        look::mono(15.0, tf::WEIGHT_REGULAR, fg),
        look::mono(13.0, tf::WEIGHT_REGULAR, fg),
        look::mono(12.5, tf::WEIGHT_REGULAR, fg),
        look::mono(12.0, tf::WEIGHT_REGULAR, fg),
        look::mono(12.0, tf::WEIGHT_MEDIUM, fg),
        look::mono(11.5, tf::WEIGHT_REGULAR, fg),
        look::mono(11.0, tf::WEIGHT_REGULAR, fg),
        look::mono(10.5, tf::WEIGHT_BOLD, fg),
        look::mono(10.5, tf::WEIGHT_MEDIUM, fg),
    ];
    let mut out: Vec<(String, aizome_kit::text::TextSpec)> =
        specs.iter().map(|s| (sample.to_owned(), s.clone())).collect();
    let row = look::ui(14.0 * f, tf::WEIGHT_REGULAR, fg);
    let trail = look::mono(12.0, tf::WEIGHT_REGULAR, fg);
    for title in titles {
        out.push((title.clone(), row.clone()));
        out.push((title.clone(), trail.clone()));
    }
    out
}
