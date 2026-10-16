// SPDX-License-Identifier: MIT OR Apache-2.0
//! Graphs: a few minutes of readings, newest on the right, as filled lines.
//! Each series is drawn by a layer of its own whose CSS color it takes
//! (`.graph-series.cpu-user { color: … }`), so the desktop's stylesheet
//! colors them as it colors the rest. Series can stack (user and system
//! time) or overlap (reads and writes).
use gtk::{cairo, prelude::*};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

struct Data {
    series: Vec<VecDeque<f64>>,
    capacity: usize,
    /// A fixed top (100 for percentages), or the largest value, at least
    /// `least`.
    max: Option<f64>,
    least: f64,
    stacked: bool,
}

impl Data {
    fn top(&self) -> f64 {
        if let Some(max) = self.max {
            return max;
        }
        let len = self.series.iter().map(VecDeque::len).max().unwrap_or(0);
        let peak = (0..len)
            .map(|i| {
                let values = self.series.iter().map(|s| s.get(i).copied().unwrap_or(0.0));
                if self.stacked {
                    values.sum()
                } else {
                    values.fold(0.0, f64::max)
                }
            })
            .fold(0.0, f64::max);
        (peak * 1.15).max(self.least)
    }

    /// The bottom and top of series `layer` at reading `i`.
    fn span(&self, layer: usize, i: usize) -> (f64, f64) {
        let at = |s: usize| self.series[s].get(i).copied().unwrap_or(0.0).max(0.0);
        if self.stacked {
            let below: f64 = (0..layer).map(at).sum();
            (below, below + at(layer))
        } else {
            (0.0, at(layer))
        }
    }
}

#[derive(Clone)]
pub struct Graph {
    root: gtk::Overlay,
    layers: Vec<gtk::DrawingArea>,
    label: gtk::Label,
    data: Rc<RefCell<Data>>,
}

impl Graph {
    /// A graph of one series per CSS class.
    pub fn new(classes: &[&str], max: Option<f64>, stacked: bool, height: i32) -> Graph {
        let data = Rc::new(RefCell::new(Data {
            series: classes.iter().map(|_| VecDeque::new()).collect(),
            capacity: crate::entry::HISTORY,
            max,
            least: 1.0,
            stacked,
        }));
        let grid = gtk::DrawingArea::new();
        grid.add_css_class("graph-grid");
        grid.set_content_height(height);
        grid.set_hexpand(true);
        grid.set_draw_func(|area, cr, width, height| {
            let color = area.color();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64 * 0.12,
            );
            cr.set_line_width(1.0);
            for quarter in 1..4 {
                let y = (height as f64 * quarter as f64 / 4.0).round() + 0.5;
                cr.move_to(0.0, y);
                cr.line_to(width as f64, y);
            }
            let _ = cr.stroke();
        });
        let root = gtk::Overlay::new();
        root.add_css_class("graph");
        root.set_child(Some(&grid));
        root.set_overflow(gtk::Overflow::Hidden);
        let layers: Vec<gtk::DrawingArea> = classes
            .iter()
            .enumerate()
            .map(|(index, class)| {
                let layer = gtk::DrawingArea::new();
                layer.add_css_class("graph-series");
                layer.add_css_class(class);
                layer.set_can_target(false);
                let data = data.clone();
                layer.set_draw_func(move |area, cr, width, height| {
                    draw(
                        &data.borrow(),
                        index,
                        &area.color(),
                        cr,
                        width as f64,
                        height as f64,
                    );
                });
                root.add_overlay(&layer);
                layer
            })
            .collect();
        let label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .css_classes(["graph-label", "caption"])
            .build();
        label.set_can_target(false);
        root.add_overlay(&label);
        Graph {
            root,
            layers,
            label,
            data,
        }
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.root
    }

    /// The smallest top an auto-scaled graph shows.
    pub fn set_least(&self, least: f64) {
        self.data.borrow_mut().least = least;
    }

    pub fn set_label(&self, text: &str) {
        self.label.set_label(text);
        self.label.set_visible(!text.is_empty());
    }

    /// Add a reading: one value per series.
    pub fn push(&self, values: &[f64]) {
        {
            let mut data = self.data.borrow_mut();
            let capacity = data.capacity;
            for (series, value) in data.series.iter_mut().zip(values) {
                series.push_back(*value);
                while series.len() > capacity {
                    series.pop_front();
                }
            }
        }
        self.redraw();
    }

    /// Show this history instead: one list of values per series.
    pub fn set(&self, series: Vec<Vec<f64>>) {
        {
            let mut data = self.data.borrow_mut();
            for (to, from) in data.series.iter_mut().zip(series) {
                *to = from.into();
            }
        }
        self.redraw();
    }

    fn redraw(&self) {
        for layer in &self.layers {
            layer.queue_draw();
        }
    }
}

fn draw(
    data: &Data,
    layer: usize,
    color: &gtk::gdk::RGBA,
    cr: &cairo::Context,
    width: f64,
    height: f64,
) {
    let len = data.series[layer].len();
    if len < 2 {
        return;
    }
    let top = data.top().max(f64::EPSILON);
    let step = width / (data.capacity.max(2) - 1) as f64;
    // Newest at the right edge.
    let x = |i: usize| width - (len - 1 - i) as f64 * step;
    let y = |value: f64| height - (value / top).min(1.0) * (height - 2.0) - 1.0;
    let (r, g, b, a) = (
        color.red() as f64,
        color.green() as f64,
        color.blue() as f64,
        color.alpha() as f64,
    );
    // The area: along the tops, back along the bottoms.
    cr.move_to(x(0), y(data.span(layer, 0).1));
    for i in 1..len {
        cr.line_to(x(i), y(data.span(layer, i).1));
    }
    for i in (0..len).rev() {
        cr.line_to(x(i), y(data.span(layer, i).0));
    }
    cr.close_path();
    let fill = cairo::LinearGradient::new(0.0, 0.0, 0.0, height);
    fill.add_color_stop_rgba(0.0, r, g, b, a * 0.42);
    fill.add_color_stop_rgba(1.0, r, g, b, a * 0.10);
    let _ = cr.set_source(&fill);
    let _ = cr.fill();
    cr.move_to(x(0), y(data.span(layer, 0).1));
    for i in 1..len {
        cr.line_to(x(i), y(data.span(layer, i).1));
    }
    cr.set_source_rgba(r, g, b, a);
    cr.set_line_width(1.5);
    cr.set_line_join(cairo::LineJoin::Round);
    let _ = cr.stroke();
}

/// Each processor core's use as a bar, side by side.
#[derive(Clone)]
pub struct Bars {
    area: gtk::DrawingArea,
    values: Rc<RefCell<Vec<f64>>>,
}

impl Bars {
    pub fn new(height: i32) -> Bars {
        let area = gtk::DrawingArea::new();
        area.add_css_class("core-bars");
        area.set_content_height(height);
        let values: Rc<RefCell<Vec<f64>>> = Rc::default();
        let drawn = values.clone();
        area.set_draw_func(move |area, cr, width, height| {
            let values = drawn.borrow();
            if values.is_empty() {
                return;
            }
            let color = area.color();
            let (r, g, b, a) = (
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64,
            );
            let gap = 2.0;
            let n = values.len() as f64;
            let bar = ((width as f64 - gap * (n - 1.0)) / n).max(1.0);
            for (i, value) in values.iter().enumerate() {
                let x = i as f64 * (bar + gap);
                cr.set_source_rgba(r, g, b, a * 0.10);
                cr.rectangle(x, 0.0, bar, height as f64);
                let _ = cr.fill();
                let filled = (value / 100.0).clamp(0.0, 1.0) * height as f64;
                cr.set_source_rgba(r, g, b, a * 0.85);
                cr.rectangle(x, height as f64 - filled, bar, filled);
                let _ = cr.fill();
            }
        });
        Bars { area, values }
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn set(&self, values: &[f64]) {
        *self.values.borrow_mut() = values.to_vec();
        let width = (values.len() as i32 * 8).clamp(48, 160);
        self.area.set_content_width(width);
        self.area.queue_draw();
    }
}

/// A bar split into parts (memory by kind), each part's color from a CSS
/// class as for the graphs.
#[derive(Clone)]
pub struct Meter {
    root: gtk::Overlay,
    layers: Vec<gtk::DrawingArea>,
    parts: Rc<RefCell<Vec<f64>>>,
}

impl Meter {
    pub fn new(classes: &[&str]) -> Meter {
        let track = gtk::DrawingArea::new();
        track.add_css_class("meter-track");
        track.set_content_height(8);
        track.set_hexpand(true);
        track.set_draw_func(|area, cr, width, height| {
            let color = area.color();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64 * 0.10,
            );
            cr.rectangle(0.0, 0.0, width as f64, height as f64);
            let _ = cr.fill();
        });
        let root = gtk::Overlay::new();
        root.set_child(Some(&track));
        let parts: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(vec![0.0; classes.len()]));
        let layers = classes
            .iter()
            .enumerate()
            .map(|(index, class)| {
                let layer = gtk::DrawingArea::new();
                layer.add_css_class("meter-part");
                layer.add_css_class(class);
                let parts = parts.clone();
                layer.set_draw_func(move |area, cr, width, height| {
                    let parts = parts.borrow();
                    let start: f64 = parts[..index].iter().sum();
                    let color = area.color();
                    cr.set_source_rgba(
                        color.red() as f64,
                        color.green() as f64,
                        color.blue() as f64,
                        color.alpha() as f64 * 0.9,
                    );
                    let w = width as f64;
                    cr.rectangle(
                        start.min(1.0) * w,
                        0.0,
                        parts[index].clamp(0.0, 1.0 - start.min(1.0)) * w,
                        height as f64,
                    );
                    let _ = cr.fill();
                });
                root.add_overlay(&layer);
                layer
            })
            .collect();
        Meter {
            root,
            layers,
            parts,
        }
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.root
    }

    /// Each part's fraction of the whole bar.
    pub fn set(&self, fractions: &[f64]) {
        {
            let mut parts = self.parts.borrow_mut();
            for (part, value) in parts.iter_mut().zip(fractions) {
                *part = value.max(0.0);
            }
        }
        for layer in &self.layers {
            layer.queue_draw();
        }
    }
}
