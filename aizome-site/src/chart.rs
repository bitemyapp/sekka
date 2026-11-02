//! Bar charts for the performance page, drawn when the site is built: SVG
//! in the page, coloured by the theme's classes (so Night, Day and High
//! Contrast each get their own), with no script. One measure per chart,
//! Aizome's bars in amber and the others in a quieter grey; every bar
//! carries its value at the tip and a `<title>` for the pointer, and a
//! table under the chart gives the same numbers for screen readers and for
//! copying.

use crate::html::esc;
use std::fmt::Write;

/// One bar: its label, its value, whether it is Aizome's, and a note for
/// its tooltip and the table (what exactly was measured, the spread).
pub struct Bar<'a> {
    pub label: &'a str,
    pub value: f64,
    pub ours: bool,
    pub note: &'a str,
}

pub const fn ours<'a>(label: &'a str, value: f64, note: &'a str) -> Bar<'a> {
    Bar { label, value, ours: true, note }
}

pub const fn other<'a>(label: &'a str, value: f64, note: &'a str) -> Bar<'a> {
    Bar { label, value, ours: false, note }
}

/// A chart of `bars` on one scale from zero.
pub struct Chart<'a> {
    /// Unique on the page; names the chart for its table.
    pub id: &'a str,
    /// What is plotted, with its unit and which way is better, e.g.
    /// "Time to the first frame, ms (lower is better)".
    pub title: &'a str,
    pub unit: &'a str,
    pub decimals: usize,
    pub bars: &'a [Bar<'a>],
    /// Under the chart: how it was measured, briefly.
    pub caption: &'a str,
    /// The value a full-width bar stands for; 0 scales to the largest bar.
    /// Small multiples share one so their bars compare.
    pub scale: f64,
    /// Whether to put the numbers in a table under the chart (small
    /// multiples leave it to one table for them all).
    pub table: bool,
    /// The label column's width; 0 sizes it to these bars' labels. Small
    /// multiples share one, so equal values draw equal bars.
    pub label_w: f64,
}

const WIDTH: f64 = 540.0;
/// The label column: wide enough for the longest label, within limits.
pub fn label_width(bars: &[Bar]) -> f64 {
    let longest = bars.iter().map(|b| b.label.chars().count()).max().unwrap_or(0) as f64;
    (longest * 8.2 + 14.0).clamp(110.0, 240.0)
}
const VALUE_W: f64 = 64.0;
const ROW: f64 = 30.0;
const BAR: f64 = 16.0;
const TOP: f64 = 6.0;

fn number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    // Thousands separators for the whole part.
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_owned(), format!(".{f}")),
        None => (s.clone(), String::new()),
    };
    let (sign, digits) = match int.strip_prefix('-') {
        Some(d) => ("-", d.to_owned()),
        None => ("", int),
    };
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped}{frac}")
}

impl Chart<'_> {
    /// Values of 100 and more are shown whole: three figures are enough.
    fn shown(&self, v: f64) -> String {
        number(v, if v.abs() >= 100.0 { 0 } else { self.decimals })
    }

    pub fn html(&self) -> String {
        let largest = self.bars.iter().map(|b| b.value).fold(0.0_f64, f64::max);
        let max = if self.scale > 0.0 { self.scale.max(largest) } else { largest }.max(f64::MIN_POSITIVE);
        let label_w = if self.label_w > 0.0 { self.label_w } else { label_width(self.bars) };
        let span = WIDTH - label_w - VALUE_W;
        let height = TOP * 2.0 + ROW * self.bars.len() as f64;
        let mut svg = String::new();
        let summary: Vec<String> = self
            .bars
            .iter()
            .map(|b| format!("{} {} {}", b.label, self.shown(b.value), self.unit))
            .collect();
        let _ = write!(
            svg,
            r#"<svg class="chart-svg" viewBox="0 0 {WIDTH} {height}" role="img" aria-label="{}: {}">"#,
            esc(self.title),
            esc(&summary.join("; ")),
        );
        let _ = write!(
            svg,
            r#"<line class="chart-base" x1="{label_w}" y1="{TOP}" x2="{label_w}" y2="{}"/>"#,
            height - TOP
        );
        for (i, b) in self.bars.iter().enumerate() {
            let y = TOP + ROW * i as f64 + (ROW - BAR) / 2.0;
            let w = (b.value / max * span).max(1.0);
            let class = if b.ours { "bar-ours" } else { "bar-other" };
            // Square at the baseline, a 4px rounded end at the value.
            let r = 4.0_f64.min(w / 2.0);
            let path = format!(
                "M{x0},{y}h{a}a{r},{r} 0 0 1 {r},{r}v{v}a{r},{r} 0 0 1 -{r},{r}h-{a}z",
                x0 = label_w,
                a = w - r,
                v = BAR - 2.0 * r,
            );
            let value = format!("{} {}", self.shown(b.value), self.unit);
            let tip = if b.note.is_empty() {
                format!("{}: {value}", b.label)
            } else {
                format!("{}: {value} ({})", b.label, b.note)
            };
            let _ = write!(
                svg,
                r#"<g class="chart-row"><title>{tip}</title><rect class="chart-hit" x="0" y="{hy}" width="{WIDTH}" height="{ROW}"/><text class="chart-label{lc}" x="{lx}" y="{ty}" text-anchor="end">{label}</text><path class="{class}" d="{path}"/><text class="chart-value" x="{vx}" y="{ty}">{v}</text></g>"#,
                tip = esc(&tip),
                hy = TOP + ROW * i as f64,
                lc = if b.ours { " chart-label-ours" } else { "" },
                lx = label_w - 10.0,
                ty = y + BAR / 2.0 + 4.5,
                label = esc(b.label),
                vx = label_w + w + 8.0,
                v = esc(&self.shown(b.value)),
            );
        }
        svg.push_str("</svg>");

        let mut rows = String::new();
        for b in self.bars {
            let _ = write!(
                rows,
                r#"<tr><th scope="row">{}</th><td>{}</td><td class="chart-note">{}</td></tr>"#,
                esc(b.label),
                self.shown(b.value),
                esc(b.note),
            );
        }
        let caption = if self.caption.is_empty() {
            String::new()
        } else {
            format!(r#"<p class="chart-caption">{}</p>"#, self.caption)
        };
        let table = if self.table {
            format!(
                r#"<details class="chart-table"><summary>The numbers</summary><div class="table-wrap"><table class="numbers"><thead><tr><th scope="col"></th><th scope="col">{unit}</th><th scope="col">Notes</th></tr></thead><tbody>{rows}</tbody></table></div></details>"#,
                unit = esc(self.unit),
            )
        } else {
            String::new()
        };
        format!(
            r#"<figure class="chart" id="{id}"><figcaption class="chart-title">{title}</figcaption><div class="chart-scroll">{svg}</div>{caption}{table}</figure>"#,
            id = self.id,
            title = esc(self.title),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_grouped() {
        assert_eq!(number(1234567.0, 0), "1,234,567");
        assert_eq!(number(999.0, 1), "999.0");
        assert_eq!(number(12.345, 2), "12.35");
        assert_eq!(number(-1500.0, 0), "-1,500");
    }

    #[test]
    fn bars_scale_to_the_largest() {
        let bars = [ours("Aizome", 10.0, ""), other("Other", 40.0, "")];
        let html = Chart { id: "t", title: "T", unit: "ms", decimals: 0, bars: &bars, caption: "", scale: 0.0, table: true, label_w: 0.0 }.html();
        assert!(html.contains("Aizome: 10 ms"));
        assert!(html.contains("<details"));
        // The largest bar fills the span; the other is a quarter of it.
        let span = WIDTH - label_width(&bars) - VALUE_W;
        assert!(html.contains(&format!("h{}", span - 4.0)));
        assert!(html.contains(&format!("h{}", span / 4.0 - 4.0)));
    }
}
