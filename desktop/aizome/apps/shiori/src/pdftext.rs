//! A page's text, as hayro shows its glyphs: each glyph's characters (its
//! `ToUnicode`, else its name) and box, in the order the page draws them,
//! with the spaces and line ends a reader infers where it leaves a gap or
//! starts another line, and `/ActualText` in place of the glyphs it covers.
//! Boxes are a line high, from about the font's descenders to its
//! ascenders, as readers select. Finding text searches this.

use std::collections::HashMap;

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::hayro_cmap::{BfString, WritingMode};
use hayro::hayro_interpret::hayro_syntax::object::Dict;
use hayro::hayro_interpret::hayro_syntax::page::Page;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache,
    InterpreterSettings, RenderContent, SoftMask, interpret_page,
};
use hayro::kurbo::{Affine, BezPath, Point, Rect, Vec2};

use crate::document::{shown, text_string};
use crate::protocol::{PageRect, PageText};

/// Where a glyph's box reaches above and below its baseline, in
/// thousandths of its font size (glyph space).
const ASCENT: f64 = 880.0;
const DESCENT: f64 = -220.0;
/// A glyph's advance when its font gives none (Type 3 fonts don't).
const DEFAULT_ADVANCE: f64 = 500.0;
/// A gap wider than this many ems between two glyphs on a line is a space.
const SPACE_GAP: f64 = 0.15;
/// The most hits a page gives (a page of a million isn't searched to the
/// end).
const MOST_HITS: usize = 5000;

/// A glyph as it shows: its text, the four corners of its box (its start
/// edge's top and bottom, then its end edge's), and the line it's on.
#[derive(Clone, Debug)]
struct Shown {
    text: Vec<char>,
    start: (Point, Point),
    end: (Point, Point),
    /// Where it starts and ends along the way the text runs (the baseline,
    /// or a vertical line's middle).
    from: Point,
    to: Point,
    /// The way the text runs, and across it, as unit vectors.
    flow: Vec2,
    across: Vec2,
    /// The font's size on the page.
    em: f64,
}

impl Shown {
    fn new(text: Vec<char>, m: Affine, advance: f64, vertical: bool) -> Option<Shown> {
        let p = |x: f64, y: f64| m * Point::new(x, y);
        let em = (p(0.0, 1000.0) - p(0.0, 0.0)).hypot();
        let x_axis = p(1.0, 0.0) - p(0.0, 0.0);
        let y_axis = p(0.0, 1.0) - p(0.0, 0.0);
        if !(em.is_finite() && em > 1e-3) || x_axis.hypot() < 1e-9 || y_axis.hypot() < 1e-9 {
            return None;
        }
        let (from, to, flow, across) = if vertical {
            let (from, to) = (p(advance / 2.0, ASCENT), p(advance / 2.0, DESCENT));
            (from, to, (to - from).normalize(), x_axis.normalize())
        } else {
            (p(0.0, 0.0), p(advance, 0.0), x_axis.normalize(), y_axis.normalize())
        };
        Some(Shown {
            text,
            start: (p(0.0, ASCENT), p(0.0, DESCENT)),
            end: (p(advance, ASCENT), p(advance, DESCENT)),
            from,
            to,
            flow,
            across,
            em,
        })
    }

    /// The box of the `i`th of `n` equal parts of the glyph along its line.
    fn part(&self, i: usize, n: usize) -> PageRect {
        let at = |t: f64| (self.start.0.lerp(self.end.0, t), self.start.1.lerp(self.end.1, t));
        let (a, b) = (at(i as f64 / n as f64), at((i + 1) as f64 / n as f64));
        let r = Rect::from_points(a.0, a.1).union_pt(b.0).union_pt(b.1);
        [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32]
    }

    fn is_space(&self) -> bool {
        self.text.iter().all(|c| c.is_whitespace())
    }
}

/// How the next glyph follows the one before it.
#[derive(Debug, PartialEq)]
enum Follows {
    Directly,
    AfterSpace,
    OnNewLine,
}

fn follows(prev: &Shown, next: &Shown) -> Follows {
    let em = prev.em.max(next.em);
    let across = (next.from - prev.from).dot(prev.across);
    let along = (next.from - prev.to).dot(prev.flow);
    if prev.flow.dot(next.flow) < 0.95 || across.abs() > 0.5 * prev.em || along < -2.0 * em {
        Follows::OnNewLine
    } else if along > SPACE_GAP * em && !prev.is_space() && !next.is_space() {
        Follows::AfterSpace
    } else {
        Follows::Directly
    }
}

/// The device the page is interpreted into: it keeps its glyphs.
struct Collector {
    glyphs: Vec<Shown>,
    /// The `/ActualText` of each marked-content sequence open, and where
    /// its glyphs start.
    actual: Vec<Option<(String, usize)>>,
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let vertical = run.writing_mode() == WritingMode::Vertical;
        for g in run.glyphs() {
            let text = match g.as_unicode() {
                Some(BfString::Char(c)) => vec![c],
                Some(BfString::String(s)) => s.chars().collect(),
                None => vec!['\u{fffd}'],
            };
            let advance = match &**g {
                hayro::hayro_interpret::font::Glyph::Outline(o) => o.advance_width().map(f64::from),
                hayro::hayro_interpret::font::Glyph::Type3(_) => None,
            }
            .filter(|a| a.is_finite() && *a > 0.0)
            .unwrap_or(DEFAULT_ADVANCE);
            if let Some(s) = Shown::new(text, props.transform * g.transform(), advance, vertical) {
                self.glyphs.push(s);
            }
        }
    }

    fn begin_marked_content(&mut self, _: &[u8], properties: Option<&Dict<'_>>) {
        let actual = properties
            .and_then(|p| p.get::<hayro::hayro_interpret::hayro_syntax::object::String<'_>>(b"ActualText"))
            .map(|t| text_string(&t));
        self.actual.push(actual.map(|t| (t, self.glyphs.len())));
    }

    fn end_marked_content(&mut self) {
        let Some(Some((text, at))) = self.actual.pop() else { return };
        // Nested sequences' text is the outer one's.
        if self.actual.iter().any(Option::is_some) || at >= self.glyphs.len() {
            return;
        }
        let (first, last) = (&self.glyphs[at], &self.glyphs[self.glyphs.len() - 1]);
        // Glyphs that turn can't share one box: they're kept as they are.
        if first.flow.dot(last.flow) < 0.95 {
            return;
        }
        // An empty `/ActualText` says the glyphs aren't text (a hyphen
        // only there for the line's end, say).
        let actual = (!text.is_empty()).then(|| Shown {
            text: text.chars().collect(),
            end: last.end,
            to: last.to,
            ..first.clone()
        });
        self.glyphs.truncate(at);
        self.glyphs.extend(actual);
    }
}

/// The text of `page` (index `n` among the document's), interpreted with
/// the document's cache.
pub fn page_text<'a>(page: &Page<'a>, cache: &InterpreterCache<'a>, settings: &InterpreterSettings) -> PageText {
    let (w, h) = page.render_dimensions();
    let settings = InterpreterSettings { render_content: RenderContent::PAGE, ..settings.clone() };
    let mut context = Context::new(shown(page), Rect::new(0.0, 0.0, w as f64, h as f64), cache, page.xref(), settings);
    let mut collector = Collector { glyphs: Vec::new(), actual: Vec::new() };
    interpret_page(page, &mut context, &mut collector);
    lay_out(&collector.glyphs)
}

/// The glyphs' characters in order, with the spaces and line ends between
/// them, and their boxes (none for those inferred).
fn lay_out(glyphs: &[Shown]) -> PageText {
    let mut out = PageText::default();
    // Glyphs drawn twice in nearly the same place (bold made by
    // overprinting, or a shadow) count once: those already shown, by their
    // first character and where they are to the nearest few points.
    let mut seen: HashMap<(char, i64, i64), Vec<Point>> = HashMap::new();
    let cell = |p: Point| ((p.x / 4.0).floor() as i64, (p.y / 4.0).floor() as i64);
    let mut prev: Option<&Shown> = None;
    for g in glyphs {
        let Some(&first) = g.text.first() else { continue };
        let (cx, cy) = cell(g.from);
        let near = 0.1 * g.em;
        let repeated = (cx - 1..=cx + 1).any(|x| {
            (cy - 1..=cy + 1)
                .any(|y| seen.get(&(first, x, y)).is_some_and(|ps| ps.iter().any(|p| (*p - g.from).hypot() < near)))
        });
        if repeated && !first.is_whitespace() {
            continue;
        }
        seen.entry((first, cx, cy)).or_default().push(g.from);
        if let Some(p) = prev {
            match follows(p, g) {
                Follows::OnNewLine if out.chars.last() != Some(&'\n') => {
                    out.chars.push('\n');
                    out.boxes.push([0.0; 4]);
                }
                Follows::AfterSpace => {
                    out.chars.push(' ');
                    out.boxes.push([0.0; 4]);
                }
                _ => {}
            }
        }
        for (i, c) in g.text.iter().enumerate() {
            out.chars.push(*c);
            out.boxes.push(g.part(i, g.text.len()));
        }
        prev = Some(g);
    }
    out
}

/// The hits of `query` in `text`: each the rectangles it covers, a line
/// each. Runs of white space match one another, ligatures match their
/// letters, soft hyphens are skipped, and case is ignored unless `case`.
pub fn find(text: &PageText, query: &str, case: bool) -> Vec<Vec<PageRect>> {
    let (needle, _) = normalize(&query.chars().collect::<Vec<_>>(), case);
    let needle: Vec<char> = {
        let mut n = needle;
        while n.first() == Some(&' ') {
            n.remove(0);
        }
        while n.last() == Some(&' ') {
            n.pop();
        }
        n
    };
    if needle.is_empty() {
        return Vec::new();
    }
    let (hay, from) = normalize(&text.chars, case);
    let mut hits = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() && hits.len() < MOST_HITS {
        if hay[i..i + needle.len()] == needle[..] {
            let rects = covered(text, from[i], from[i + needle.len() - 1]);
            if !rects.is_empty() {
                hits.push(rects);
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    hits
}

/// The characters to search, and the index in `chars` each came from.
fn normalize(chars: &[char], case: bool) -> (Vec<char>, Vec<usize>) {
    let mut out = Vec::with_capacity(chars.len());
    let mut from = Vec::with_capacity(chars.len());
    let mut space = false;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                from.push(i);
            }
            space = true;
            continue;
        }
        space = false;
        let expanded: &[char] = match c {
            '\u{ad}' => &[],
            'ﬀ' => &['f', 'f'],
            'ﬁ' => &['f', 'i'],
            'ﬂ' => &['f', 'l'],
            'ﬃ' => &['f', 'f', 'i'],
            'ﬄ' => &['f', 'f', 'l'],
            'ﬅ' | 'ﬆ' => &['s', 't'],
            _ => std::slice::from_ref(&c),
        };
        for &e in expanded {
            if case {
                out.push(e);
                from.push(i);
            } else {
                for l in e.to_lowercase() {
                    out.push(l);
                    from.push(i);
                }
            }
        }
    }
    (out, from)
}

/// The rectangles characters `first` to `last` cover: one for each line
/// (the lines the layout ended), joining their boxes.
fn covered(text: &PageText, first: usize, last: usize) -> Vec<PageRect> {
    let mut rects: Vec<PageRect> = Vec::new();
    let mut open = false;
    for i in first..=last {
        if text.chars[i] == '\n' {
            open = false;
            continue;
        }
        let b = text.boxes[i];
        if b == [0.0; 4] {
            continue;
        }
        match rects.last_mut() {
            Some(r) if open => *r = [r[0].min(b[0]), r[1].min(b[1]), r[2].max(b[2]), r[3].max(b[3])],
            _ => {
                rects.push(b);
                open = true;
            }
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glyph of `size` points at (x, y) on an upright page, `advance`
    /// thousandths wide.
    fn at(text: &str, x: f64, y: f64, size: f64, advance: f64) -> Shown {
        // Glyph space (thousandths of an em, y up) to a page's points, y
        // down.
        let m = Affine::new([size / 1000.0, 0.0, 0.0, -size / 1000.0, x, y]);
        Shown::new(text.chars().collect(), m, advance, false).unwrap()
    }

    fn word(s: &str, x: f64, y: f64) -> Vec<Shown> {
        s.chars().enumerate().map(|(i, c)| at(&c.to_string(), x + 6.0 * i as f64, y, 10.0, 600.0)).collect()
    }

    fn joined(t: &PageText) -> String {
        t.chars.iter().collect()
    }

    #[test]
    fn spaces_and_lines_are_inferred() {
        let mut g = word("Hello", 10.0, 100.0);
        // A word space's gap (a third of an em) after it.
        g.extend(word("world", 10.0 + 30.0 + 3.3, 100.0));
        g.extend(word("Next", 10.0, 114.0));
        let t = lay_out(&g);
        assert_eq!(joined(&t), "Hello world\nNext");
        // Inferred characters have no box; the others are a line high.
        assert_eq!(t.boxes[5], [0.0; 4]);
        let h = t.boxes[0];
        assert!((h[0] - 10.0).abs() < 1e-4 && (h[2] - 16.0).abs() < 1e-4, "{h:?}");
        assert!((h[1] - (100.0 - 8.8)).abs() < 1e-4 && (h[3] - (100.0 + 2.2)).abs() < 1e-4, "{h:?}");
    }

    #[test]
    fn overprinted_glyphs_count_once() {
        let mut g = word("Bold", 10.0, 100.0);
        g.extend(word("Bold", 10.3, 100.0));
        assert_eq!(joined(&lay_out(&g)), "Bold");
    }

    #[test]
    fn ligatures_split_their_box() {
        let t = lay_out(&[at("fi", 0.0, 50.0, 10.0, 600.0)]);
        assert_eq!(joined(&t), "fi");
        assert!((t.boxes[0][2] - 3.0).abs() < 1e-4 && (t.boxes[1][0] - 3.0).abs() < 1e-4);
    }

    #[test]
    fn actual_text_takes_the_place_of_its_glyphs() {
        let mut c = Collector { glyphs: word("x", 0.0, 100.0), actual: Vec::new() };
        c.actual.push(Some(("ﬁ".into(), 1)));
        c.glyphs.extend(word("fi", 6.0, 100.0));
        c.end_marked_content();
        assert_eq!(joined(&lay_out(&c.glyphs)), "xﬁ");
        // Empty: what it covers isn't text.
        c.actual.push(Some((String::new(), 2)));
        c.glyphs.extend(word("-", 18.0, 100.0));
        c.end_marked_content();
        assert_eq!(joined(&lay_out(&c.glyphs)), "xﬁ");
        // Glyphs that turn on the way keep their own text.
        c.actual.push(Some(("ab".into(), 2)));
        c.glyphs.push(at("a", 20.0, 100.0, 10.0, 600.0));
        let turned = Affine::new([0.0, -0.01, -0.01, 0.0, 30.0, 100.0]);
        c.glyphs.push(Shown::new(vec!['b'], turned, 600.0, false).unwrap());
        c.end_marked_content();
        assert_eq!(c.glyphs.len(), 4);
        assert_eq!(c.glyphs[3].text, vec!['b']);
    }

    #[test]
    fn finding_ignores_case_spacing_and_ligatures() {
        let mut g = word("A", 0.0, 20.0);
        g.push(at("ﬁ", 6.0, 20.0, 10.0, 600.0));
        g.extend(word("nd", 12.0, 20.0));
        g.extend(word("the", 0.0, 34.0));
        let t = lay_out(&g);
        assert_eq!(joined(&t), "Aﬁnd\nthe");
        assert_eq!(find(&t, "find", false).len(), 1);
        assert!(find(&t, "FIND", true).is_empty());
        // Across the line end: a rectangle on each line.
        let hits = find(&t, "nd  the", false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].len(), 2);
        assert!(find(&t, "  ", false).is_empty());
    }
}
