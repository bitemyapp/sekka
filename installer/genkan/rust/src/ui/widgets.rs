// SPDX-License-Identifier: GPL-3.0-or-later
//! Small presentation helpers shared by the installer pages.
use adw::prelude::*;
use calamares_nixos::{Desktop, Filesystem, Firmware, disk::Layout};
use gtk::{Align, Label, Orientation, glib};
use std::{cell::RefCell, rc::Rc};

const KIB: f64 = 1024.0;

/// Human-readable binary size: "512 MiB", "31.4 GiB", "1.82 TiB".
pub fn size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes >= KIB.powi(4) {
        format!("{:.2} TiB", bytes / KIB.powi(4))
    } else if bytes >= KIB.powi(3) {
        format!("{:.1} GiB", bytes / KIB.powi(3))
    } else {
        format!("{:.0} MiB", bytes / KIB.powi(2))
    }
}

/// "0:07", "1:42", "1:02:05".
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "47 seconds", "1 minute 12 seconds".
pub fn duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (m, s) = (total / 60, total % 60);
    let unit = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if m == 0 {
        unit(s, "second")
    } else if s == 0 {
        unit(m, "minute")
    } else {
        format!("{} {}", unit(m, "minute"), unit(s, "second"))
    }
}

pub fn label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label
}

pub fn caption(text: &str) -> Label {
    let label = label(text);
    label.add_css_class("caption");
    label.add_css_class("dim-label");
    label
}

pub fn badge(text: &str, class: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("badge");
    label.add_css_class(class);
    label.set_valign(Align::Center);
    label
}

pub fn icon(name: &str) -> gtk::Image {
    gtk::Image::from_icon_name(name)
}

/// A boxed list row whose subtitle is the value: used on the review page.
pub fn property(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_css_class("property");
    row.set_use_markup(false);
    row.set_subtitle_selectable(true);
    row
}

pub fn clamp(child: &impl IsA<gtk::Widget>, width: i32) -> adw::Clamp {
    adw::Clamp::builder()
        .maximum_size(width)
        .tightening_threshold(width * 3 / 4)
        .child(child)
        .build()
}

pub fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build()
}

fn monogram_text(desktop: Desktop) -> &'static str {
    match desktop {
        Desktop::Plasma => "K",
        Desktop::Gnome => "G",
        Desktop::Xfce => "X",
        Desktop::Hyprland => "H",
        Desktop::Tatami => "T",
    }
}

/// A selectable card for one desktop environment.
pub fn desktop_card(desktop: Desktop) -> gtk::ToggleButton {
    let monogram = Label::new(Some(monogram_text(desktop)));
    monogram.add_css_class("monogram");
    monogram.add_css_class(&format!("monogram-{}", desktop.id()));
    monogram.set_valign(Align::Start);
    let title = Label::new(Some(desktop.label()));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_hexpand(true);
    let check = icon("object-select-symbolic");
    check.add_css_class("card-check");
    check.set_valign(Align::Start);
    let top = gtk::Box::new(Orientation::Horizontal, 12);
    top.append(&monogram);
    top.append(&title);
    top.append(&check);
    let description = label(desktop.description());
    description.add_css_class("card-description");
    // Bounded natural widths let the flow box place cards side by side.
    title.set_max_width_chars(16);
    description.set_max_width_chars(24);
    description.set_width_chars(18);
    let content = gtk::Box::new(Orientation::Vertical, 10);
    content.append(&top);
    content.append(&description);
    let card = gtk::ToggleButton::builder()
        .child(&content)
        .css_classes(["card", "desktop-card"])
        .build();
    card.update_property(&[gtk::accessible::Property::Label(desktop.label())]);
    card
}

/// The disk icon most likely to match the device type.
pub fn disk_icon(path: &str) -> &'static str {
    if path.starts_with("/dev/nvme") {
        "drive-harddisk-solidstate-symbolic"
    } else if path.starts_with("/dev/mmcblk") {
        "media-flash-symbolic"
    } else {
        "drive-harddisk-symbolic"
    }
}

const EFI_COLOR: (f64, f64, f64) = (0.898, 0.647, 0.039);
const BIOS_COLOR: (f64, f64, f64) = (0.6, 0.6, 0.6);
const ROOT_COLOR: (f64, f64, f64) = (0.322, 0.467, 0.765);
const SWAP_COLOR: (f64, f64, f64) = (0.569, 0.255, 0.675);

fn hex((r, g, b): (f64, f64, f64)) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8
    )
}

struct Segment {
    name: String,
    bytes: u64,
    color: (f64, f64, f64),
}

/// A proportional partition preview with a legend underneath.
pub struct LayoutBar {
    pub widget: gtk::Box,
    area: gtk::DrawingArea,
    legend: Label,
    segments: Rc<RefCell<Vec<Segment>>>,
}
impl LayoutBar {
    pub fn new() -> Self {
        let segments = Rc::new(RefCell::new(Vec::<Segment>::new()));
        let area = gtk::DrawingArea::builder()
            .content_height(30)
            .hexpand(true)
            .css_classes(["layout-bar"])
            .build();
        area.set_draw_func({
            let segments = segments.clone();
            move |_, cr, width, height| draw(cr, width, height, &segments.borrow())
        });
        let legend = Label::new(None);
        legend.add_css_class("legend");
        legend.set_xalign(0.0);
        legend.set_wrap(true);
        let widget = gtk::Box::new(Orientation::Vertical, 10);
        widget.set_margin_top(12);
        widget.set_margin_bottom(12);
        widget.set_margin_start(12);
        widget.set_margin_end(12);
        widget.append(&area);
        widget.append(&legend);
        Self {
            widget,
            area,
            legend,
            segments,
        }
    }
    pub fn show(&self, layout: &Layout, firmware: Firmware, filesystem: Filesystem) {
        let mib = 1024 * 1024;
        let mut segments = vec![
            if firmware == Firmware::Uefi {
                Segment {
                    name: "EFI system (FAT32)".into(),
                    bytes: (layout.boot.1 - layout.boot.0) * mib,
                    color: EFI_COLOR,
                }
            } else {
                Segment {
                    name: "BIOS boot".into(),
                    bytes: (layout.boot.1 - layout.boot.0) * mib,
                    color: BIOS_COLOR,
                }
            },
            Segment {
                name: format!("NixOS ({})", filesystem.name()),
                bytes: layout.root_bytes(),
                color: ROOT_COLOR,
            },
        ];
        if let Some(bytes) = layout.swap_bytes() {
            segments.push(Segment {
                name: "Swap + zswap".into(),
                bytes,
                color: SWAP_COLOR,
            });
        }
        self.legend.set_markup(
            &segments
                .iter()
                .map(|s| {
                    format!(
                        "<span foreground=\"{}\">●</span> {} · {}",
                        hex(s.color),
                        glib::markup_escape_text(&s.name),
                        size(s.bytes)
                    )
                })
                .collect::<Vec<_>>()
                .join("     "),
        );
        *self.segments.borrow_mut() = segments;
        self.area.set_visible(true);
        self.area.queue_draw();
    }
    pub fn clear(&self, message: &str) {
        self.segments.borrow_mut().clear();
        self.legend.set_text(message);
        self.area.set_visible(false);
    }
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    let r = r.min(h / 2.0).min(w / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn draw(cr: &gtk::cairo::Context, width: i32, height: i32, segments: &[Segment]) {
    let total: u64 = segments.iter().map(|s| s.bytes).sum();
    if total == 0 || width <= 0 {
        return;
    }
    let (width, height) = (f64::from(width), f64::from(height));
    // Tiny boot partitions stay visible; the rest is proportional.
    let minimum = (width * 0.035).max(10.0);
    let gap = 3.0;
    let available = width - gap * (segments.len() as f64 - 1.0);
    let raw: Vec<f64> = segments
        .iter()
        .map(|s| available * s.bytes as f64 / total as f64)
        .collect();
    let small: f64 = raw.iter().filter(|w| **w < minimum).count() as f64 * minimum;
    let large: f64 = raw.iter().filter(|w| **w >= minimum).sum();
    let scale = if large > 0.0 {
        (available - small) / large
    } else {
        1.0
    };
    let mut x = 0.0;
    for (segment, raw) in segments.iter().zip(raw) {
        let w = if raw < minimum { minimum } else { raw * scale };
        let (r, g, b) = segment.color;
        cr.set_source_rgb(r, g, b);
        rounded(cr, x, 0.0, w, height, 7.0);
        let _ = cr.fill();
        x += w + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_and_durations_read_naturally() {
        assert_eq!(size(512 * 1024 * 1024), "512 MiB");
        assert_eq!(size(32 * 1024u64.pow(3)), "32.0 GiB");
        assert_eq!(size(2 * 1024u64.pow(4)), "2.00 TiB");
        assert_eq!(clock(7), "0:07");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(duration(47.4), "47 seconds");
        assert_eq!(duration(60.0), "1 minute");
        assert_eq!(duration(72.0), "1 minute 12 seconds");
        assert_eq!(duration(1.0), "1 second");
    }
}
