// SPDX-License-Identifier: GPL-3.0-or-later
//! The clickable world map on the Location page. It draws the countries,
//! shades the selected zone's region and marks its city; a click or hover
//! resolves to a zone through `calamares_nixos::zonemap`.
use calamares_nixos::zonemap::{self, Zones};
use gtk::{cairo, gdk, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Ring = Vec<(f64, f64)>;
type Picked = Box<dyn Fn(usize)>;

/// Map colours for one colour scheme, as RGBA.
struct Palette {
    ocean: [f64; 4],
    grid: [f64; 4],
    land: [f64; 4],
    border: [f64; 4],
    hover: [f64; 4],
    region: [f64; 4],
    region_edge: [f64; 4],
    pin: [f64; 4],
    label: (gdk::RGBA, gdk::RGBA),
}

const LIGHT: Palette = Palette {
    ocean: [0.86, 0.92, 0.97, 1.0],
    grid: [0.32, 0.47, 0.76, 0.08],
    land: [0.99, 0.99, 1.0, 1.0],
    border: [0.62, 0.69, 0.79, 0.9],
    hover: [0.49, 0.73, 0.89, 0.35],
    region: [0.32, 0.47, 0.76, 0.42],
    region_edge: [0.25, 0.39, 0.68, 0.9],
    pin: [0.25, 0.39, 0.68, 1.0],
    label: (
        gdk::RGBA::new(1.0, 1.0, 1.0, 0.96),
        gdk::RGBA::new(0.13, 0.17, 0.25, 1.0),
    ),
};

const DARK: Palette = Palette {
    ocean: [0.11, 0.15, 0.2, 1.0],
    grid: [0.49, 0.73, 0.89, 0.07],
    land: [0.24, 0.28, 0.34, 1.0],
    border: [0.36, 0.42, 0.5, 1.0],
    hover: [0.49, 0.73, 0.89, 0.22],
    region: [0.49, 0.73, 0.89, 0.42],
    region_edge: [0.56, 0.75, 0.91, 0.9],
    pin: [0.56, 0.75, 0.91, 1.0],
    label: (
        gdk::RGBA::new(0.16, 0.19, 0.24, 0.96),
        gdk::RGBA::new(0.95, 0.96, 0.98, 1.0),
    ),
};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ZoneMap {
        pub(super) zones: RefCell<Option<Rc<Zones>>>,
        /// Projected rings in unit coordinates, per country and per band.
        pub(super) countries: RefCell<Vec<Vec<Ring>>>,
        pub(super) bands: RefCell<Vec<Vec<Ring>>>,
        pub(super) selected: Cell<Option<usize>>,
        pub(super) hover: Cell<Option<usize>>,
        pub(super) picked: RefCell<Option<Picked>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZoneMap {
        const NAME: &'static str = "CalamaresZoneMap";
        type Type = super::ZoneMap;
        type ParentType = gtk::Widget;

        fn class_init(class: &mut Self::Class) {
            class.set_css_name("zonemap");
            class.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for ZoneMap {}

    impl WidgetImpl for ZoneMap {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (280, 760, -1, -1),
                _ => {
                    let width = if for_size < 0 { 760 } else { for_size };
                    let height = (f64::from(width) / zonemap::aspect()).round() as i32;
                    (height, height, -1, -1)
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct ZoneMap(ObjectSubclass<imp::ZoneMap>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Map placement inside the widget: origin and size in pixels.
#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Frame {
    fn point(self, (u, v): (f64, f64)) -> (f64, f64) {
        (self.x + u * self.width, self.y + v * self.height)
    }
}

fn source(cr: &cairo::Context, [r, g, b, a]: [f64; 4]) {
    cr.set_source_rgba(r, g, b, a);
}

impl ZoneMap {
    pub fn new(zones: Option<Rc<Zones>>) -> Self {
        let map: Self = glib::Object::new();
        map.set_hexpand(true);
        map.set_cursor_from_name(Some("crosshair"));
        map.set_has_tooltip(true);
        map.update_property(&[gtk::accessible::Property::Label(
            "World map. Click where you are to choose your time zone.",
        )]);
        if let Some(zones) = zones {
            let project = |rings: &[Ring]| -> Vec<Ring> {
                rings
                    .iter()
                    .map(|ring| {
                        ring.iter()
                            .map(|&(lon, lat)| zonemap::to_unit(lon, lat))
                            .collect()
                    })
                    .collect()
            };
            let imp = map.imp();
            *imp.countries.borrow_mut() = zones
                .atlas
                .countries
                .iter()
                .map(|c| project(&c.shape.rings))
                .collect();
            *imp.bands.borrow_mut() = zones
                .atlas
                .bands
                .iter()
                .map(|b| project(&b.shape.rings))
                .collect();
            *imp.zones.borrow_mut() = Some(zones);
        }

        let click = gtk::GestureClick::new();
        click.connect_released({
            let map = map.downgrade();
            move |_, _, x, y| {
                if let Some(map) = map.upgrade()
                    && let Some(index) = map.place_at(x, y)
                {
                    map.select(Some(index));
                    if let Some(picked) = map.imp().picked.borrow().as_ref() {
                        picked(index);
                    }
                }
            }
        });
        map.add_controller(click);
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion({
            let map = map.downgrade();
            move |_, x, y| {
                if let Some(map) = map.upgrade() {
                    map.set_hover(map.place_at(x, y));
                }
            }
        });
        motion.connect_leave({
            let map = map.downgrade();
            move |_| {
                if let Some(map) = map.upgrade() {
                    map.set_hover(None);
                }
            }
        });
        map.add_controller(motion);
        map.connect_query_tooltip(|map, x, y, _, tooltip| {
            let Some(index) = map.place_at(f64::from(x), f64::from(y)) else {
                return false;
            };
            let zones = map.imp().zones.borrow();
            let Some(place) = zones.as_ref().and_then(|z| z.places.get(index)) else {
                return false;
            };
            tooltip.set_text(Some(&match place.name(false) {
                Some(name) => format!("{} · {name}", place.label()),
                None => place.label(),
            }));
            true
        });
        map
    }

    /// Called with the index of a zone the user clicked.
    pub fn connect_picked(&self, f: impl Fn(usize) + 'static) {
        *self.imp().picked.borrow_mut() = Some(Box::new(f));
    }

    pub fn select(&self, index: Option<usize>) {
        if self.imp().selected.replace(index) != index {
            self.queue_draw();
        }
    }

    fn set_hover(&self, index: Option<usize>) {
        if self.imp().hover.replace(index) != index {
            self.queue_draw();
        }
    }

    fn frame(&self) -> Frame {
        let (width, height) = (f64::from(self.width()), f64::from(self.height()));
        let pad = 8.0;
        let fit = (width - 2.0 * pad).min((height - 2.0 * pad) * zonemap::aspect());
        let map_height = fit / zonemap::aspect();
        Frame {
            x: (width - fit) / 2.0,
            y: (height - map_height) / 2.0,
            width: fit,
            height: map_height,
        }
    }

    fn place_at(&self, x: f64, y: f64) -> Option<usize> {
        let frame = self.frame();
        let (lon, lat) =
            zonemap::from_unit((x - frame.x) / frame.width, (y - frame.y) / frame.height)?;
        self.imp().zones.borrow().as_ref()?.locate(lon, lat).ok()
    }

    fn trace(cr: &cairo::Context, frame: Frame, rings: &[Ring]) {
        for ring in rings {
            let mut points = ring.iter().map(|&p| frame.point(p));
            if let Some((x, y)) = points.next() {
                cr.move_to(x, y);
                for (x, y) in points {
                    cr.line_to(x, y);
                }
                cr.close_path();
            }
        }
    }

    /// The globe's outline at the cropped latitudes.
    fn outline(cr: &cairo::Context, frame: Frame) {
        let steps = 48;
        let latitude = |i: i32| {
            zonemap::SOUTH + (zonemap::NORTH - zonemap::SOUTH) * f64::from(i) / f64::from(steps)
        };
        for i in 0..=steps {
            let (x, y) = frame.point(zonemap::to_unit(-180.0, latitude(i)));
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        for i in (0..=steps).rev() {
            let (x, y) = frame.point(zonemap::to_unit(180.0, latitude(i)));
            cr.line_to(x, y);
        }
        cr.close_path();
    }

    fn shade(
        &self,
        cr: &cairo::Context,
        frame: Frame,
        index: usize,
        fill: [f64; 4],
        edge: Option<[f64; 4]>,
    ) -> Result<(), cairo::Error> {
        let imp = self.imp();
        let zones = imp.zones.borrow();
        let Some(zones) = zones.as_ref() else {
            return Ok(());
        };
        let territory = zones.territory(index);
        let Some(country) = territory.country else {
            return Ok(());
        };
        let countries = imp.countries.borrow();
        let bands = imp.bands.borrow();
        cr.save()?;
        cr.new_path();
        Self::trace(cr, frame, &countries[country]);
        if !territory.bands.is_empty() {
            cr.clip();
            for &band in &territory.bands {
                Self::trace(cr, frame, &bands[band]);
            }
        }
        source(cr, fill);
        match edge {
            Some(edge) => {
                cr.fill_preserve()?;
                source(cr, edge);
                cr.set_line_width(1.0);
                cr.stroke()?;
            }
            None => cr.fill()?,
        }
        cr.restore()
    }

    fn paint(
        &self,
        cr: &cairo::Context,
        frame: Frame,
        palette: &Palette,
    ) -> Result<(), cairo::Error> {
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        cr.set_line_join(cairo::LineJoin::Round);
        Self::outline(cr, frame);
        source(cr, palette.ocean);
        cr.fill_preserve()?;
        cr.save()?;
        cr.clip();
        // Graticule every 30 degrees.
        source(cr, palette.grid);
        cr.set_line_width(1.0);
        for lon in (-150..=150).step_by(30) {
            for step in 0..=40 {
                let lat =
                    zonemap::SOUTH + (zonemap::NORTH - zonemap::SOUTH) * f64::from(step) / 40.0;
                let (x, y) = frame.point(zonemap::to_unit(f64::from(lon), lat));
                if step == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }
        }
        for lat in [-30.0, 0.0, 30.0, 60.0] {
            let (x0, y) = frame.point(zonemap::to_unit(-180.0, lat));
            let (x1, _) = frame.point(zonemap::to_unit(180.0, lat));
            cr.move_to(x0, y);
            cr.line_to(x1, y);
        }
        cr.stroke()?;
        cr.restore()?;

        let imp = self.imp();
        cr.new_path();
        for rings in imp.countries.borrow().iter() {
            Self::trace(cr, frame, rings);
        }
        source(cr, palette.land);
        cr.fill_preserve()?;
        source(cr, palette.border);
        cr.set_line_width(0.6);
        cr.stroke()?;

        let selected = imp.selected.get();
        if let Some(hover) = imp.hover.get().filter(|h| Some(*h) != selected) {
            self.shade(cr, frame, hover, palette.hover, None)?;
        }
        if let Some(selected) = selected {
            self.shade(
                cr,
                frame,
                selected,
                palette.region,
                Some(palette.region_edge),
            )?;
        }
        Ok(())
    }

    fn position(&self, index: Option<usize>) -> Option<(f64, f64)> {
        let zones = self.imp().zones.borrow();
        let (lon, lat) = zones.as_ref()?.places.get(index?)?.position?;
        Some(zonemap::to_unit(
            lon,
            lat.clamp(zonemap::SOUTH, zonemap::NORTH),
        ))
    }

    fn pin(cr: &cairo::Context, (x, y): (f64, f64), palette: &Palette) -> Result<(), cairo::Error> {
        let [r, g, b, _] = palette.pin;
        cr.new_path();
        cr.arc(x, y, 13.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(r, g, b, 0.18);
        cr.fill()?;
        cr.arc(x, y + 1.0, 7.5, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.22);
        cr.fill()?;
        cr.arc(x, y, 7.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.fill()?;
        cr.arc(x, y, 4.5, 0.0, std::f64::consts::TAU);
        cr.set_source_rgb(r, g, b);
        cr.fill()
    }

    /// The selected city's name in a pill beside its pin.
    fn label(&self, snapshot: &gtk::Snapshot, (x, y): (f64, f64), text: &str, palette: &Palette) {
        let layout = self.create_pango_layout(None);
        layout.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(text)));
        let attributes = pango::AttrList::new();
        attributes.insert(pango::AttrFloat::new_scale(0.9));
        layout.set_attributes(Some(&attributes));
        let (text_width, text_height) = layout.pixel_size();
        let (width, height) = (f64::from(text_width) + 16.0, f64::from(text_height) + 8.0);
        let mut left = x + 14.0;
        if left + width > f64::from(self.width()) - 4.0 {
            left = x - 14.0 - width;
        }
        let top = (y - height / 2.0).clamp(2.0, (f64::from(self.height()) - height - 2.0).max(2.0));
        let rect = graphene::Rect::new(left as f32, top as f32, width as f32, height as f32);
        let rounded = gsk::RoundedRect::from_rect(rect, (height / 2.0) as f32);
        snapshot.append_outset_shadow(
            &rounded,
            &gdk::RGBA::new(0.0, 0.0, 0.0, 0.18),
            0.0,
            1.0,
            0.0,
            4.0,
        );
        snapshot.push_rounded_clip(&rounded);
        snapshot.append_color(&palette.label.0, &rect);
        snapshot.pop();
        snapshot.save();
        snapshot.translate(&graphene::Point::new(
            (left + 8.0) as f32,
            (top + 4.0) as f32,
        ));
        snapshot.append_layout(&layout, &palette.label.1);
        snapshot.restore();
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let (width, height) = (self.width() as f32, self.height() as f32);
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let palette = if adw::StyleManager::default().is_dark() {
            &DARK
        } else {
            &LIGHT
        };
        let frame = self.frame();
        let imp = self.imp();
        let selected = imp.selected.get();
        let pin = self.position(selected).map(|p| frame.point(p));
        let hover = self
            .position(imp.hover.get().filter(|h| Some(*h) != selected))
            .map(|p| frame.point(p));
        {
            let cr = snapshot.append_cairo(&graphene::Rect::new(0.0, 0.0, width, height));
            let _ = self.paint(&cr, frame, palette).and_then(|()| {
                if let Some((x, y)) = hover {
                    cr.new_path();
                    cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
                    source(&cr, palette.pin);
                    cr.set_line_width(1.5);
                    cr.stroke()?;
                }
                match pin {
                    Some(point) => Self::pin(&cr, point, palette),
                    None => Ok(()),
                }
            });
        }
        if let Some(point) = pin {
            let zones = imp.zones.borrow();
            if let Some(place) = selected.and_then(|i| zones.as_ref()?.places.get(i)) {
                self.label(snapshot, point, &place.city, palette);
            }
        }
    }
}
