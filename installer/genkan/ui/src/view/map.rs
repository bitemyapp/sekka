// SPDX-License-Identifier: GPL-3.0-or-later
//! The clickable world map on the Location page. It draws the countries,
//! shades the selected zone's region and marks its city; a click or hover
//! resolves to a zone through `calamares_nixos::zonemap`.
use crate::app::{Genkan, Msg};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle, RoundedRect, Shape};
use aizome_kit::sidestep_ui::{Canvas, PointerButton, StrokeStyle};
use aizome_kit::widget::Event;
use calamares_nixos::zonemap::{self, Zones};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

type Ring = Vec<(f64, f64)>;

/// The atlas projected once, in unit coordinates: each country's and each
/// standard-time band's rings.
pub struct Shapes {
    countries: Vec<Vec<Ring>>,
    bands: Vec<Vec<Ring>>,
}

impl Shapes {
    pub fn of(zones: &Zones) -> Shapes {
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
        Shapes {
            countries: zones
                .atlas
                .countries
                .iter()
                .map(|c| project(&c.shape.rings))
                .collect(),
            bands: zones
                .atlas
                .bands
                .iter()
                .map(|b| project(&b.shape.rings))
                .collect(),
        }
    }
}

/// What the map shows: the chosen zone and the one under the pointer.
#[derive(Clone)]
struct State {
    zones: Rc<Zones>,
    shapes: Rc<Shapes>,
    selected: Option<usize>,
    hover: Option<usize>,
}

impl Hash for State {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.zones) as usize).hash(state);
        self.selected.hash(state);
        self.hover.hash(state);
    }
}

/// Map placement inside the canvas: origin and size.
#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Frame {
    fn new(width: f64, height: f64) -> Frame {
        let pad = 8.0;
        let fit = (width - 2.0 * pad)
            .min((height - 2.0 * pad) * zonemap::aspect())
            .max(1.0);
        let map_height = fit / zonemap::aspect();
        Frame {
            x: (width - fit) / 2.0,
            y: (height - map_height) / 2.0,
            width: fit,
            height: map_height,
        }
    }

    fn point(self, (u, v): (f64, f64)) -> (f64, f64) {
        (self.x + u * self.width, self.y + v * self.height)
    }

    fn place_at(self, zones: &Zones, x: f64, y: f64) -> Option<usize> {
        let (lon, lat) = zonemap::from_unit((x - self.x) / self.width, (y - self.y) / self.height)?;
        zones.locate(lon, lat).ok()
    }
}

fn trace(path: &mut BezPath, frame: Frame, rings: &[Ring]) {
    for ring in rings {
        let mut points = ring.iter().map(|&p| frame.point(p));
        if let Some(start) = points.next() {
            path.move_to(start);
            for point in points {
                path.line_to(point);
            }
            path.close_path();
        }
    }
}

/// The globe's outline at the cropped latitudes.
fn outline(frame: Frame) -> BezPath {
    let steps = 48;
    let latitude = |i: i32| {
        zonemap::SOUTH + (zonemap::NORTH - zonemap::SOUTH) * f64::from(i) / f64::from(steps)
    };
    let mut path = BezPath::new();
    for i in 0..=steps {
        let point = frame.point(zonemap::to_unit(-180.0, latitude(i)));
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    for i in (0..=steps).rev() {
        path.line_to(frame.point(zonemap::to_unit(180.0, latitude(i))));
    }
    path.close_path();
    path
}

fn graticule(frame: Frame) -> BezPath {
    let mut path = BezPath::new();
    for lon in (-150..=150).step_by(30) {
        for step in 0..=40 {
            let lat = zonemap::SOUTH + (zonemap::NORTH - zonemap::SOUTH) * f64::from(step) / 40.0;
            let point = frame.point(zonemap::to_unit(f64::from(lon), lat));
            if step == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }
    }
    for lat in [-30.0, 0.0, 30.0, 60.0] {
        path.move_to(frame.point(zonemap::to_unit(-180.0, lat)));
        path.line_to(frame.point(zonemap::to_unit(180.0, lat)));
    }
    path
}

/// Shade the country of zone `index`, within its standard-time bands.
fn shade(
    canvas: &mut Canvas,
    state: &State,
    frame: Frame,
    index: usize,
    fill: Color,
    edge: Option<Color>,
) {
    let territory = state.zones.territory(index);
    let Some(country) = territory.country else {
        return;
    };
    let mut region = BezPath::new();
    trace(&mut region, frame, &state.shapes.countries[country]);
    canvas.save();
    if !territory.bands.is_empty() {
        canvas.clip(&region);
        region = BezPath::new();
        for &band in &territory.bands {
            trace(&mut region, frame, &state.shapes.bands[band]);
        }
    }
    canvas.fill_even_odd(&region, fill);
    if let Some(edge) = edge {
        canvas.stroke(&region, &StrokeStyle::new(1.0), edge);
    }
    canvas.restore();
}

fn position(state: &State, index: Option<usize>) -> Option<(f64, f64)> {
    let (lon, lat) = state.zones.places.get(index?)?.position?;
    Some(zonemap::to_unit(
        lon,
        lat.clamp(zonemap::SOUTH, zonemap::NORTH),
    ))
}

fn draw(canvas: &mut Canvas, bounds: Rect, state: &State, theme: &Theme) {
    let frame = Frame::new(bounds.width(), bounds.height());
    let accent = theme.role(Role::Accent);
    let globe = outline(frame);
    canvas.fill(&globe, theme.sunk());
    canvas.save();
    canvas.clip(&globe);
    canvas.stroke(
        &graticule(frame),
        &StrokeStyle::new(1.0),
        theme.line().with_alpha(0.5),
    );
    canvas.restore();

    let mut land = BezPath::new();
    for rings in &state.shapes.countries {
        trace(&mut land, frame, rings);
    }
    canvas.fill_even_odd(&land, theme.raised());
    canvas.stroke(&land, &StrokeStyle::new(0.6), theme.line2());

    if let Some(hover) = state.hover.filter(|h| Some(*h) != state.selected) {
        shade(
            canvas,
            state,
            frame,
            hover,
            theme.focus().with_alpha(0.22),
            None,
        );
    }
    if let Some(selected) = state.selected {
        shade(
            canvas,
            state,
            frame,
            selected,
            accent.with_alpha(0.42),
            Some(accent),
        );
    }
    if let Some((x, y)) =
        position(state, state.hover.filter(|h| Some(*h) != state.selected)).map(|p| frame.point(p))
    {
        canvas.stroke(&Circle::new((x, y), 4.0), &StrokeStyle::new(1.5), accent);
    }
    let Some((x, y)) = position(state, state.selected).map(|p| frame.point(p)) else {
        return;
    };
    canvas.fill(&Circle::new((x, y), 13.0), accent.with_alpha(0.18));
    canvas.fill(
        &Circle::new((x, y + 1.0), 7.5),
        Color::rgba(0.0, 0.0, 0.0, 0.22),
    );
    canvas.fill(&Circle::new((x, y), 7.0), Color::rgb(1.0, 1.0, 1.0));
    canvas.fill(&Circle::new((x, y), 4.5), accent);
    // The city's name in a pill beside its pin.
    let Some(place) = state.selected.and_then(|i| state.zones.places.get(i)) else {
        return;
    };
    let mut spec = FontSpec::ui(12.0);
    spec.weight = 650.0;
    let style = theme.style(spec, theme.text());
    let measured = aizome_kit::sidestep_ui::measure(&place.city, &style);
    let (width, height) = (measured.width + 16.0, measured.height + 8.0);
    let mut left = x + 14.0;
    if left + width > bounds.width() - 4.0 {
        left = x - 14.0 - width;
    }
    let top = (y - height / 2.0).clamp(2.0, (bounds.height() - height - 2.0).max(2.0));
    let pill = RoundedRect::new(left, top, left + width, top + height, height / 2.0);
    canvas.fill(&pill.to_path(0.1), theme.raised());
    canvas.stroke(&pill, &StrokeStyle::new(1.0), theme.line2());
    canvas.draw_label(&place.city, &style, Point::new(left + 8.0, top + 4.0));
}

/// The map at `width` points, `width / aspect` tall.
pub fn view(app: &Genkan, height: f64) -> Element<Msg> {
    let (Some(zones), Some(shapes)) = (&app.zones, &app.map) else {
        return container(
            text("The time zone map is unavailable. Choose your time zone from the list.")
                .role(Role::Text3)
                .wrap(),
        )
        .padding(16.0)
        .width(Fill)
        .into();
    };
    let state = State {
        zones: zones.clone(),
        shapes: shapes.clone(),
        selected: app.zone,
        hover: app.zone_hover,
    };
    let tooltip = app
        .zone_hover
        .and_then(|i| zones.places.get(i))
        .map(|place| match place.name(false) {
            Some(name) => format!("{} · {name}", place.label()),
            None => place.label(),
        });
    let mut map = canvas(state, draw)
        .on_event(|event, size, state| {
            let frame = Frame::new(size.width, size.height);
            match event {
                Event::PointerMove { pos, .. } => {
                    let hover = frame.place_at(&state.zones, pos.x, pos.y);
                    (hover != state.hover).then_some(Msg::HoverZone(hover))
                }
                Event::PointerLeave => state.hover.is_some().then_some(Msg::HoverZone(None)),
                Event::PointerUp {
                    pos,
                    button: PointerButton::Left,
                    ..
                } => frame
                    .place_at(&state.zones, pos.x, pos.y)
                    .map(Msg::PickZone),
                _ => None,
            }
        })
        .width(Fill)
        .height(height)
        .id("zone-map")
        .label("World map. Click where you are to choose your time zone.");
    if let Some(tip) = tooltip {
        map = map.tooltip(tip);
    }
    map.into()
}
