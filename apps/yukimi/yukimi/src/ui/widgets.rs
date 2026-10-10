// SPDX-License-Identifier: MIT OR Apache-2.0
//! Small pieces used across pages: falling snow, badges, size bars and
//! friendly labels.
use std::cell::RefCell;
use std::f64::consts::TAU;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

/// One snowflake.
struct Flake {
    x: f64,
    y: f64,
    radius: f64,
    /// Fraction of the height fallen per second.
    speed: f64,
    drift: f64,
    phase: f64,
    /// Drawn with six arms rather than as a dot.
    crystal: bool,
}

/// A tiny deterministic random source, so the snow is the same every start.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f64 / 10_000.0
    }
}

/// Gently falling snow, drawn over whatever is behind it. Stands still when
/// the desktop asks for less animation, and while the window is in the
/// background; otherwise moves thirty times a second, which is plenty for
/// snow and half the work of every frame.
pub fn snowfall(count: usize) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_can_target(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    let mut seed = Seed(0x9e37_79b9_7f4a_7c15);
    let flakes: Vec<Flake> = (0..count)
        .map(|i| {
            let crystal = i % 7 == 0;
            Flake {
                x: seed.next(),
                y: seed.next(),
                radius: if crystal { 3.5 + seed.next() * 3.0 } else { 0.8 + seed.next() * 2.2 },
                speed: 0.025 + seed.next() * 0.05,
                drift: 4.0 + seed.next() * 14.0,
                phase: seed.next() * TAU,
                crystal,
            }
        })
        .collect();
    let state = Rc::new(RefCell::new((flakes, 0.0f64)));
    let draw_state = state.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let (flakes, time) = &*draw_state.borrow();
        let (w, h) = (width as f64, height as f64);
        for flake in flakes {
            let x = flake.x * w + (time * 0.6 + flake.phase).sin() * flake.drift;
            let y = flake.y * h;
            let alpha = 0.35 + 0.5 * (flake.radius / 6.5).min(1.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
            if flake.crystal {
                cr.set_line_width(1.1);
                for arm in 0..6 {
                    let angle = arm as f64 * TAU / 6.0 + flake.phase + time * 0.15;
                    let (dx, dy) = (angle.cos() * flake.radius, angle.sin() * flake.radius);
                    cr.move_to(x - dx, y - dy);
                    cr.line_to(x + dx, y + dy);
                    // A small branch near each tip.
                    let (bx, by) = (x + dx * 0.6, y + dy * 0.6);
                    for side in [-1.0, 1.0] {
                        let a = angle + side * 0.6;
                        cr.move_to(bx, by);
                        cr.line_to(bx + a.cos() * flake.radius * 0.35, by + a.sin() * flake.radius * 0.35);
                    }
                }
                let _ = cr.stroke();
            } else {
                cr.arc(x, y, flake.radius, 0.0, TAU);
                let _ = cr.fill();
            }
        }
    });
    let start = Rc::new(RefCell::new(None::<i64>));
    area.add_tick_callback(move |widget, clock| {
        let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        let active = widget.root().and_downcast::<gtk::Window>().is_none_or(|w| w.is_active());
        let now = clock.frame_time();
        let mut last = start.borrow_mut();
        if !animate || !active {
            *last = None;
            return glib::ControlFlow::Continue;
        }
        if last.is_some_and(|t| now - t < 33_000) {
            return glib::ControlFlow::Continue;
        }
        let dt = last.map_or(0.0, |t| ((now - t) as f64 / 1e6).min(0.1));
        *last = Some(now);
        let (flakes, time) = &mut *state.borrow_mut();
        *time += dt;
        for flake in flakes.iter_mut() {
            flake.y += flake.speed * dt;
            if flake.y > 1.05 {
                flake.y -= 1.1;
            }
        }
        widget.queue_draw();
        glib::ControlFlow::Continue
    });
    area
}

/// A small rounded label: `unfree`, `current`, `installed`.
pub fn badge(text: &str, kind: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("badge");
    label.add_css_class(kind);
    label.set_valign(gtk::Align::Center);
    label
}

/// A horizontal bar showing a share of some total.
pub fn share_bar(fraction: f64) -> gtk::LevelBar {
    let bar = gtk::LevelBar::for_interval(0.0, 1.0);
    bar.set_value(fraction.clamp(0.0, 1.0));
    bar.set_width_request(160);
    bar.set_valign(gtk::Align::Center);
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
    bar.add_css_class("share");
    bar
}

/// A label for numbers and names that should line up: tabular figures.
pub fn dim(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("dim-label");
    label.add_css_class("numeric");
    label.set_valign(gtk::Align::Center);
    label
}

/// A store path as Pango markup: the hash faded, the name clear.
pub fn store_path_markup(path: &str) -> String {
    match yukimi_store::StorePath::parse(path) {
        Some(p) => format!(
            "<span alpha='45%'>/nix/store/{}-</span><b>{}</b>",
            p.hash(),
            glib::markup_escape_text(p.full_name())
        ),
        None => glib::markup_escape_text(path).to_string(),
    }
}

/// A coloured circle with initials, the same colour every time for a name.
pub fn monogram(name: &str, size: i32) -> adw::Avatar {
    let pretty = name.replace(['-', '_', '.'], " ");
    adw::Avatar::new(size, Some(&pretty), true)
}

/// Text that wraps, for descriptions in rows and cards.
pub fn wrapping(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// A heading with a short explanation under it, for sections.
pub fn heading(title: &str, explanation: &str) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("title-3");
    label.set_xalign(0.0);
    column.append(&label);
    if !explanation.is_empty() {
        column.append(&wrapping(explanation, &["dim-label"]));
    }
    column
}

/// A scrolling page of width-limited content.
pub fn page(content: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder().maximum_size(960).tightening_threshold(720).child(content).build();
    gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&clamp).build()
}

/// Add many rows a batch at a time, between frames, so a long list never
/// holds up the window: `add` puts each made row in place. Stops when
/// `anchor` is no longer in a window (its page was built again).
pub fn fill_later<T: 'static, W: IsA<gtk::Widget>>(
    anchor: &impl IsA<gtk::Widget>,
    items: Vec<T>,
    make: impl Fn(&T) -> W + 'static,
    add: impl Fn(&W) + 'static,
) {
    const BATCH: usize = 40;
    let _busy = crate::stalls::doing("adding rows");
    let mut items = items.into_iter();
    // The first batch at once, so the list doesn't start empty.
    for item in items.by_ref().take(BATCH) {
        add(&make(&item));
    }
    let anchor = anchor.as_ref().downgrade();
    glib::idle_add_local(move || {
        if anchor.upgrade().is_none_or(|a| a.root().is_none()) {
            return glib::ControlFlow::Break;
        }
        let _busy = crate::stalls::doing("adding rows");
        let mut added = 0;
        for item in items.by_ref().take(BATCH) {
            add(&make(&item));
            added += 1;
        }
        if added < BATCH { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
    });
}

/// A row saying something is still being read.
pub fn pending_row(text: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(text);
    let spinner = adw::Spinner::new();
    row.add_prefix(&spinner);
    row.add_css_class("dim-row");
    row
}

/// Remove every child of a box.
pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_paths_fade_their_hash() {
        let markup = store_path_markup("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-firefox-142.0");
        assert!(markup.contains("<b>firefox-142.0</b>"));
        assert!(markup.starts_with("<span alpha='45%'>/nix/store/0123456789abcdfghijklmnpqrsvwxyz-</span>"));
    }
}
