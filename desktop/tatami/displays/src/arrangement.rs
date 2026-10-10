// SPDX-License-Identifier: MIT OR Apache-2.0
//! The arrangement: the displays that are on, drawn to scale where they are
//! in the layout, as in macOS's Displays settings. Dragging one moves it;
//! where it is dropped is reported, and the window snaps it beside the
//! others. A click selects it. The main display has a menu bar.
use crate::model::Rect;
use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc};

/// A display to draw: its rectangle in the layout (logical pixels).
pub struct Tile {
    pub name: String,
    pub rect: Rect,
    pub title: String,
    pub subtitle: String,
    pub main: bool,
}

type Select = Rc<dyn Fn(&str)>;
type Moved = Rc<dyn Fn(&str, i32, i32)>;

#[derive(Default)]
struct Inner {
    tiles: Vec<(Tile, gtk::Box)>,
    selected: String,
    size: (f64, f64),
    /// Layout to widget: widget = offset + (layout - origin) * factor.
    factor: f64,
    offset: (f64, f64),
    origin: (i32, i32),
    drag_start: Option<(f64, f64)>,
    on_select: Option<Select>,
    on_move: Option<Moved>,
}

#[derive(Clone)]
pub struct Arrangement {
    root: gtk::Overlay,
    fixed: gtk::Fixed,
    inner: Rc<RefCell<Inner>>,
}

const PADDING: f64 = 28.0;

impl Arrangement {
    pub fn new() -> Self {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_content_height(230);
        let fixed = gtk::Fixed::new();
        let root = gtk::Overlay::new();
        root.set_child(Some(&area));
        root.add_overlay(&fixed);
        root.add_css_class("arrangement");
        root.set_overflow(gtk::Overflow::Hidden);
        let arrangement = Self {
            root,
            fixed,
            inner: Rc::default(),
        };
        let this = arrangement.clone();
        area.connect_resize(move |_, width, height| {
            this.inner.borrow_mut().size = (width as f64, height as f64);
            this.layout();
        });
        arrangement
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.root
    }

    pub fn connect_select(&self, select: impl Fn(&str) + 'static) {
        self.inner.borrow_mut().on_select = Some(Rc::new(select));
    }

    /// Called with where a display was dropped, in layout pixels.
    pub fn connect_moved(&self, moved: impl Fn(&str, i32, i32) + 'static) {
        self.inner.borrow_mut().on_move = Some(Rc::new(moved));
    }

    pub fn set_tiles(&self, tiles: Vec<Tile>, selected: &str) {
        for (_, widget) in self.inner.borrow_mut().tiles.drain(..) {
            self.fixed.remove(&widget);
        }
        let built: Vec<(Tile, gtk::Box)> = tiles
            .into_iter()
            .map(|tile| {
                let widget = self.tile_widget(&tile);
                self.fixed.put(&widget, 0.0, 0.0);
                (tile, widget)
            })
            .collect();
        {
            let mut inner = self.inner.borrow_mut();
            inner.tiles = built;
            inner.selected = selected.to_owned();
        }
        self.layout();
    }

    fn tile_widget(&self, tile: &Tile) -> gtk::Box {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("display-tile");
        widget.set_cursor_from_name(Some("grab"));
        widget.set_tooltip_text(Some(&format!("{}\n{}", tile.title, tile.subtitle)));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        bar.add_css_class("menu-bar");
        bar.set_visible(tile.main);
        widget.append(&bar);
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        labels.set_vexpand(true);
        labels.set_valign(gtk::Align::Center);
        let title = gtk::Label::new(Some(&tile.title));
        title.add_css_class("title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle = gtk::Label::new(Some(&tile.subtitle));
        subtitle.add_css_class("subtitle");
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        labels.append(&title);
        labels.append(&subtitle);
        widget.append(&labels);

        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        let name = tile.name.clone();
        let this = self.clone();
        let moving = widget.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let start = this.fixed.child_position(&moving);
            this.inner.borrow_mut().drag_start = Some(start);
            moving.set_cursor_from_name(Some("grabbing"));
        });
        let this = self.clone();
        let moving = widget.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let Some((x, y)) = this.inner.borrow().drag_start else {
                return;
            };
            let (width, height) = this.inner.borrow().size;
            let x = (x + dx).clamp(0.0, (width - moving.width() as f64).max(0.0));
            let y = (y + dy).clamp(0.0, (height - moving.height() as f64).max(0.0));
            this.fixed.move_(&moving, x, y);
        });
        let this = self.clone();
        let moving = widget.clone();
        drag.connect_drag_end(move |_, dx, dy| {
            moving.set_cursor_from_name(Some("grab"));
            let start = this.inner.borrow_mut().drag_start.take();
            if dx.hypot(dy) < 4.0 {
                this.select(&name);
                return;
            }
            let Some((x, y)) = start else {
                return;
            };
            let (x, y) = this.to_layout(x + dx, y + dy);
            // Called with no borrow held: the window redraws the tiles.
            let moved = this.inner.borrow().on_move.clone();
            if let Some(moved) = moved {
                moved(&name, x, y);
            }
        });
        widget.add_controller(drag);
        widget
    }

    fn select(&self, name: &str) {
        self.inner.borrow_mut().selected = name.to_owned();
        self.layout();
        let select = self.inner.borrow().on_select.clone();
        if let Some(select) = select {
            select(name);
        }
    }

    fn to_layout(&self, x: f64, y: f64) -> (i32, i32) {
        let inner = self.inner.borrow();
        let factor = inner.factor.max(1e-6);
        (
            ((x - inner.offset.0) / factor).round() as i32 + inner.origin.0,
            ((y - inner.offset.1) / factor).round() as i32 + inner.origin.1,
        )
    }

    /// Fit the arrangement in the area, centered, and place the tiles.
    fn layout(&self) {
        let mut inner = self.inner.borrow_mut();
        let (width, height) = inner.size;
        if inner.tiles.is_empty() || width <= 0.0 {
            return;
        }
        let rects: Vec<Rect> = inner.tiles.iter().map(|(tile, _)| tile.rect).collect();
        let left = rects.iter().map(|r| r.x).min().unwrap_or(0);
        let top = rects.iter().map(|r| r.y).min().unwrap_or(0);
        let right = rects.iter().map(|r| r.x + r.w).max().unwrap_or(1);
        let bottom = rects.iter().map(|r| r.y + r.h).max().unwrap_or(1);
        let (span_x, span_y) = ((right - left).max(1) as f64, (bottom - top).max(1) as f64);
        // Room around the displays to drag one beside the others.
        let factor = ((width - 2.0 * PADDING) / span_x)
            .min((height - 2.0 * PADDING) / span_y)
            .min(0.12)
            * 0.8;
        inner.factor = factor;
        inner.origin = (left, top);
        inner.offset = (
            (width - span_x * factor) / 2.0,
            (height - span_y * factor) / 2.0,
        );
        let selected = inner.selected.clone();
        for (tile, widget) in &inner.tiles {
            let x = inner.offset.0 + (tile.rect.x - left) as f64 * factor;
            let y = inner.offset.1 + (tile.rect.y - top) as f64 * factor;
            let w = (tile.rect.w as f64 * factor).round().max(24.0) as i32;
            let h = (tile.rect.h as f64 * factor).round().max(16.0) as i32;
            widget.set_size_request(w, h);
            self.fixed.move_(widget, x, y);
            if tile.name == selected {
                widget.add_css_class("selected");
            } else {
                widget.remove_css_class("selected");
            }
        }
    }
}
