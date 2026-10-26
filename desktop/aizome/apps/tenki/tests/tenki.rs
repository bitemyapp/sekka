//! Tenki headlessly, on answers recorded from Open-Meteo
//! (`tests/fixtures`, re-recorded with `aizome-tenki --record-fixtures`):
//! the first launch (the time zone's city, the question about location),
//! searching and adding, dragging to reorder, the keyboard, units, the
//! current location, the cache offline, golden images in Night and Day,
//! and what an interaction costs.
//!
//! Golden images need Aizome's fonts: run with
//! `AIZOME_FONT_DIR=~/work/.aizome-fonts` (they're skipped without
//! Archivo).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{Point, Size};
use aizome_tenki::app::{Locating, STALE};
use aizome_tenki::forecast;
use aizome_tenki::location::{LocateError, Located};
use aizome_tenki::places::{CURRENT_ID, Found, Origin, Place, parse_search};
use aizome_tenki::services::{Recorded, Services};
use aizome_tenki::store::{Consent, Settings, State, Store};
use aizome_tenki::ui::widgets::PLACE_ROW;
use aizome_tenki::units::Units;
use aizome_tenki::{Msg, Tenki};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn recorded() -> Arc<Recorded> {
    let r = Recorded::new(fixtures());
    r.set_timezone(Some("America/Chicago"));
    Arc::new(r)
}

fn store(name: &str) -> Store {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tenki").join(name);
    let _ = std::fs::remove_dir_all(&root);
    Store::at(&root)
}

fn found(search: &str) -> Found {
    let body = std::fs::read_to_string(fixtures().join(format!("search-{}.json", search.to_lowercase()))).unwrap();
    parse_search(&body).unwrap().remove(0)
}

fn reykjavik() -> Place {
    Place {
        id: "geonames:3413829".into(),
        name: "Reykjavík".into(),
        region: "Capital Region, Iceland".into(),
        latitude: 64.13548,
        longitude: -21.89541,
        timezone: Some("Atlantic/Reykjavik".into()),
        origin: Origin::Search,
    }
}

/// A store with three places kept, Tokyo showing, location declined.
fn three_places(name: &str) -> Store {
    let s = store(name);
    let tokyo = found("Tokyo").place(Origin::Search);
    s.save_places(&[tokyo.clone(), found("Chicago").place(Origin::Search), reykjavik()]);
    s.save_state(&State { selected: Some(tokyo.id), located: None });
    s.save_settings(&Settings { units: Some(Units::Metric), location: Consent::Declined });
    s
}

fn launch(services: &Arc<Recorded>, store: Option<Store>, theme: ThemeChoice) -> Harness<Tenki> {
    let mut t = Tenki::new(services.clone() as Arc<dyn Services>, store);
    t.settings.units.get_or_insert(Units::Metric);
    Harness::with(t, Appearance { theme, ..Appearance::default() }, false)
}

fn names(h: &Harness<Tenki>) -> Vec<String> {
    h.program().places.iter().map(|p| p.name.clone()).collect()
}

fn texts_has(h: &Harness<Tenki>, needle: &str) -> bool {
    let w = h.window();
    h.texts(w).iter().any(|t| t.contains(needle))
}

#[test]
fn fixtures_read_whole() {
    for f in ["forecast-35.69_139.69.json", "forecast-41.85_-87.65.json", "forecast-64.14_-21.90.json"] {
        let body = std::fs::read_to_string(fixtures().join(f)).unwrap();
        let fc = forecast::parse(&body).unwrap();
        assert_eq!(fc.hourly.len(), forecast::HOURS, "{f}");
        assert_eq!(fc.daily.len(), forecast::DAYS, "{f}");
        assert!(fc.current.temperature.is_some() && fc.current.code.is_some(), "{f}");
        assert!(fc.daily.iter().all(|d| d.max.is_some() && d.min.is_some()), "{f}");
    }
    assert_eq!(found("Chicago").timezone.as_deref(), Some("America/Chicago"));
}

#[test]
fn the_first_launch_keeps_the_time_zone_city_and_asks_about_location() {
    let r = recorded();
    let s = store("first");
    let mut h = launch(&r, Some(s.clone()), ThemeChoice::Night);
    let t = h.program();
    assert_eq!(names(&h), ["Chicago"]);
    assert_eq!(t.places[0].origin, Origin::TimeZone);
    assert_eq!(t.selected.as_deref(), Some(t.places[0].id.as_str()));
    assert_eq!(r.asked(), ["search chicago", "forecast 41.85_-87.65"]);
    assert!(texts_has(&h, "See the weather where you are?"));
    assert!(texts_has(&h, "Illinois, United States · your time zone"));
    assert_eq!(aizome_tenki::ui::status(h.program()), "Updated just now");
    // Kept on disk: the place, and the forecast in the cache.
    assert_eq!(s.load_places().map(|p| p.len()), Some(1));
    assert!(s.load_forecast("41.85_-87.65").is_some());
    // At the narrowest the window goes, the question's words keep their
    // width (the buttons go under them).
    let w = h.window();
    h.resize(w, Size::new(640.0, 460.0));
    let words = h.find(w, "See the weather where you are?").expect("the question");
    assert!(words.width() > 200.0, "the question is squeezed: {words:?}");
    h.resize(w, Size::new(980.0, 720.0));
    // Not now: the question goes, and stays gone.
    assert!(h.click_text(w, "Not Now"));
    assert_eq!(h.program().settings.location, Consent::Declined);
    assert!(!texts_has(&h, "See the weather where you are?"));
    assert_eq!(s.load_settings().location, Consent::Declined);
}

#[test]
fn searching_adds_a_place() {
    let r = recorded();
    let mut h = launch(&r, Some(three_places("search")), ThemeChoice::Night);
    let w = h.window();
    let field = h.find_id(w, "search").expect("the search field");
    h.click(w, field.center());
    h.type_text(w, "Kyo");
    // Nothing is asked until typing pauses.
    assert!(!r.asked().iter().any(|a| a.starts_with("search")));
    h.type_text(w, "to");
    h.advance(Duration::from_millis(300));
    assert_eq!(r.asked().iter().filter(|a| a.starts_with("search")).count(), 1, "one search for the word typed");
    assert!(texts_has(&h, "Kyoto"));
    let first = h.find_id(w, "result-0").expect("a result");
    h.click(w, first.center());
    let t = h.program();
    assert_eq!(names(&h), ["Tokyo", "Chicago", "Reykjavík", "Kyoto"]);
    assert_eq!(t.selected_place().map(|p| p.name), Some("Kyoto".into()));
    assert!(t.search.text.is_empty(), "the search clears");
    // Kyoto's forecast wasn't recorded: a gentle note, not a crash.
    assert!(texts_has(&h, "No forecast for Kyoto yet"));
    // Return before the answer comes adds the first place found.
    h.click(w, field.center());
    h.type_text(w, "Tokyo\n");
    h.advance(Duration::from_millis(300));
    assert_eq!(names(&h).len(), 4, "Tokyo is kept once");
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Tokyo".into()));
    // And Return once the answer is there; ❖] and ❖[ then go between places,
    // even from the search field.
    h.click(w, field.center());
    h.type_text(w, "Chicago");
    h.advance(Duration::from_millis(300));
    assert!(h.find_id(w, "result-0").is_some());
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Chicago".into()));
    h.click(w, field.center());
    h.command(w, '[');
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Tokyo".into()), "❖[ from the field");
    h.command(w, ']');
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Chicago".into()));
}

#[test]
fn dragging_reorders_places() {
    let r = recorded();
    let s = three_places("drag");
    let mut h = launch(&r, Some(s.clone()), ThemeChoice::Night);
    let w = h.window();
    let from = h.find(w, "Reykjavík").expect("Reykjavík's row");
    let to = h.find(w, "Tokyo").expect("Tokyo's row");
    h.drag(w, from.center(), Point::new(from.center().x, to.center().y - 10.0), 8);
    assert_eq!(names(&h), ["Reykjavík", "Tokyo", "Chicago"]);
    assert_eq!(s.load_places().unwrap()[0].name, "Reykjavík", "the order is kept");
    // A click without moving only chooses.
    let chicago = h.find(w, "Chicago").unwrap();
    h.click(w, chicago.center());
    assert_eq!(names(&h), ["Reykjavík", "Tokyo", "Chicago"]);
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Chicago".into()));
    assert_eq!(h.title(w), "Chicago", "the window is named for its place");
}

#[test]
fn the_keyboard_chooses_moves_and_removes() {
    let r = recorded();
    let mut h = launch(&r, Some(three_places("keys")), ThemeChoice::Night);
    let w = h.window();
    let tokyo = h.find(w, "Tokyo").unwrap();
    h.click(w, tokyo.center());
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Chicago".into()));
    h.named(w, NamedKey::ArrowDown, Modifiers::ALT);
    assert_eq!(names(&h), ["Tokyo", "Reykjavík", "Chicago"]);
    h.named(w, NamedKey::Backspace, Modifiers::NONE);
    assert_eq!(names(&h), ["Tokyo", "Reykjavík"]);
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Reykjavík".into()), "the next one shows");
    // ❖[ from anywhere.
    h.command(w, '[');
    assert_eq!(h.program().selected_place().map(|p| p.name), Some("Tokyo".into()));
}

#[test]
fn units_switch_without_fetching() {
    let r = recorded();
    let mut h = launch(&r, Some(three_places("units")), ThemeChoice::Night);
    let asked = r.asked().len();
    let t = h.program();
    let f = t.weather_of(&t.selected_place().unwrap()).unwrap().forecast.clone().unwrap();
    let c = f.current.temperature.unwrap();
    assert!(texts_has(&h, &Units::Metric.degrees(c)));
    h.send(Msg::SetUnits(Units::Imperial));
    assert!(texts_has(&h, &Units::Imperial.degrees(c)));
    assert!(texts_has(&h, "mph"));
    assert_eq!(r.asked().len(), asked);
    let w = h.window();
    let units = h.find_id(w, "units").expect("the toolbar's switch");
    h.click(w, Point::new(units.x0 + units.width() * 0.25, units.center().y));
    assert_eq!(h.program().units(), Units::Metric);
    // Settings (❖,): the locale's units again, then Done.
    h.command(w, ',');
    let sheet = h.program().settings_sheet.expect("the Settings sheet");
    assert!(h.texts(sheet).iter().any(|t| t == "Tenki Settings"));
    h.send(Msg::UnitsChoice(0));
    assert_eq!(h.program().settings.units, None);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().settings_sheet.is_none(), "Return is Done");
}

#[test]
fn the_current_location_comes_first_once_allowed() {
    let r = recorded();
    r.set_located(Ok(Located { latitude: 35.6901, longitude: 139.6922, accuracy: 4000.0, description: None }));
    let mut h = launch(&r, Some(three_places("located")), ThemeChoice::Night);
    h.send(Msg::AllowLocation);
    let t = h.program();
    let rows = t.rows();
    assert_eq!(rows[0].id, CURRENT_ID);
    assert_eq!(rows[0].region, "35.69° N, 139.69° E");
    assert_eq!(t.selected.as_deref(), Some(CURRENT_ID));
    assert!(t.weather_of(&rows[0]).is_some_and(|w| w.forecast.is_some()));
    // The current location stays on top.
    h.send(Msg::Move(2, 0));
    assert_eq!(h.program().rows()[0].id, CURRENT_ID);
    // A refusal says so, and offers a way out.
    let r2 = recorded();
    r2.set_located(Err(LocateError::Denied));
    let mut h2 = launch(&r2, Some(three_places("denied")), ThemeChoice::Night);
    h2.send(Msg::AllowLocation);
    assert_eq!(h2.program().locating, Locating::Failed(LocateError::Denied));
    assert!(texts_has(&h2, "Location unavailable"));
    let w2 = h2.window();
    assert!(h2.click_text(w2, "Stop Using Location"));
    assert_eq!(h2.program().rows().len(), 3);
}

#[test]
fn the_cache_opens_at_once_and_offline() {
    let r = recorded();
    let s = three_places("cache");
    let at = r.recorded_at();
    r.set_now(at);
    drop(launch(&r, Some(s.clone()), ThemeChoice::Night));
    // Twelve minutes later, with no network: the cache shows at once.
    let r = recorded();
    r.set_now(at + 12 * 60);
    r.set_offline(true);
    let mut h = launch(&r, Some(s.clone()), ThemeChoice::Night);
    assert!(r.asked().is_empty(), "fresh enough not to ask: {:?}", r.asked());
    assert_eq!(aizome_tenki::ui::status(h.program()), "Updated 12 min ago");
    // Once it's stale, Tenki tries, fails, and says so gently.
    r.set_now(at + STALE + 60);
    h.advance(Duration::from_secs(60));
    assert_eq!(r.asked().len(), 1);
    assert_eq!(aizome_tenki::ui::status(h.program()), "Offline · updated 21 min ago");
    assert!(texts_has(&h, "Tokyo"));
    // A place never fetched shows why there's nothing.
    let s2 = store("never");
    s2.save_places(&[found("Tokyo").place(Origin::Search)]);
    let r2 = recorded();
    r2.set_offline(true);
    let h2 = launch(&r2, Some(s2), ThemeChoice::Night);
    assert!(texts_has(&h2, "No forecast for Tokyo yet"));
    assert!(texts_has(&h2, "Tenki can't reach Open-Meteo."));
}

#[test]
fn the_window_menus_hold_every_command() {
    let r = recorded();
    let h = launch(&r, Some(three_places("menus")), ThemeChoice::Night);
    let bar = aizome_kit::Program::menus(h.program());
    let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Tenki", "Places", "Edit", "View", "Window", "Help"]);
    let labels: Vec<String> =
        bar.menus().iter().flat_map(|m| m.actions().into_iter().map(|a| a.label.clone()).collect::<Vec<_>>()).collect();
    for l in [
        "Add Place…",
        "Remove Place",
        "Show Current Location",
        "Refresh",
        "Metric (°C, km/h, mm)",
        "Imperial (°F, mph, in)",
    ] {
        assert!(labels.iter().any(|x| x == l), "{l} in {labels:?}");
    }
}

// Pictures.

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

/// A golden at 1× (compared) and a 2× picture to look at.
fn pictures(h: &mut Harness<Tenki>, name: &str) {
    let w = h.window();
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tenki-{name}@2x.png")), 2.0);
    if !h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR)");
        return;
    }
    assert_golden(h, w, golden(name), 1.0);
}

/// Tokyo, Chicago and Reykjavík with the clock twelve minutes after the
/// answers were recorded, writing its files from the writer thread as the
/// app does.
fn scene(name: &str, theme: ThemeChoice) -> (Harness<Tenki>, Arc<Recorded>) {
    let r = recorded();
    let at = r.recorded_at();
    r.set_now(at);
    let mut h = launch(&r, Some(three_places(name).deferred(true)), theme);
    r.set_now(at + 12 * 60);
    h.advance(Duration::from_secs(60));
    (h, r)
}

#[test]
fn goldens() {
    for (theme, tname) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let (mut h, _r) = scene(&format!("golden-{tname}"), theme);
        let w = h.window();
        assert!(texts_has(&h, "Updated 12 min ago"));
        pictures(&mut h, &format!("tokyo-{tname}"));
        // The whole of it, tall, and wide enough for two columns.
        h.resize(w, Size::new(980.0, 1240.0));
        pictures(&mut h, &format!("tokyo-full-{tname}"));
        h.resize(w, Size::new(1320.0, 940.0));
        let reykjavik = h.find(w, "Reykjavík").unwrap();
        h.click(w, reykjavik.center());
        pictures(&mut h, &format!("reykjavik-wide-{tname}"));
        h.resize(w, Size::new(980.0, 720.0));
        // Searching.
        let field = h.find_id(w, "search").unwrap();
        h.click(w, field.center());
        h.type_text(w, "Chicago");
        h.advance(Duration::from_millis(300));
        pictures(&mut h, &format!("search-{tname}"));
        // A place being dragged up the list.
        h.named(w, NamedKey::Escape, Modifiers::NONE);
        let from = h.find(w, "Reykjavík").unwrap().center();
        h.press(w, from, aizome_kit::sidestep_ui::PointerButton::Left, 1);
        for dy in [10.0, 30.0, 52.0, 64.0] {
            h.move_to(w, Point::new(from.x, from.y - dy));
        }
        pictures(&mut h, &format!("drag-{tname}"));
        h.release(w, Point::new(from.x, from.y - 64.0), aizome_kit::sidestep_ui::PointerButton::Left);
        assert_eq!(names(&h), ["Tokyo", "Reykjavík", "Chicago"]);
        // Settings, over the window.
        h.named(w, NamedKey::Escape, Modifiers::NONE);
        h.command(w, ',');
        pictures(&mut h, &format!("settings-{tname}"));
    }
    // The first launch: the time zone's city, and the question.
    for (theme, tname) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let r = recorded();
        r.set_now(r.recorded_at());
        let mut h = launch(&r, Some(store(&format!("first-{tname}"))), theme);
        pictures(&mut h, &format!("first-launch-{tname}"));
    }
    // The current location, allowed, on top of the places kept.
    let r = recorded();
    let at = r.recorded_at();
    r.set_now(at);
    r.set_located(Ok(Located { latitude: 35.6901, longitude: 139.6922, accuracy: 4000.0, description: None }));
    let s = three_places("current");
    s.save_settings(&Settings { units: Some(Units::Metric), location: Consent::Allowed });
    let mut h = launch(&r, Some(s), ThemeChoice::Day);
    h.send(Msg::Select(0));
    r.set_now(at + 12 * 60);
    h.advance(Duration::from_secs(60));
    assert_eq!(h.program().selected.as_deref(), Some(CURRENT_ID));
    pictures(&mut h, "current-location-day");
    // Offline with nothing cached.
    let r = recorded();
    r.set_offline(true);
    let s = store("offline");
    s.save_places(&[found("Tokyo").place(Origin::Search)]);
    s.save_settings(&Settings { units: Some(Units::Metric), location: Consent::Declined });
    let mut h = launch(&r, Some(s), ThemeChoice::Night);
    pictures(&mut h, "offline-night");
}

/// What a typical interaction costs, from the input to its pixels: the
/// program's update, the view, layout and diff, and rasterising what
/// changed at 2×. Prints percentiles for docs/performance.md and holds
/// them to the 4 ms budget in an optimised build.
#[test]
fn interactions_fit_the_frame_budget() {
    let (mut h, _r) = scene("budget", ThemeChoice::Night);
    let w = h.window();
    let _ = h.render(w, 2.0);
    let rows: Vec<_> = ["Tokyo", "Chicago", "Reykjavík"].iter().map(|n| h.find(w, n).unwrap().center()).collect();
    let window_area = h.size(w).width * h.size(w).height;
    let drawn = std::cell::Cell::new(0.0f64);
    let frame = |h: &mut Harness<Tenki>, act: &mut dyn FnMut(&mut Harness<Tenki>)| {
        h.take_damage(w);
        let start = Instant::now();
        act(h);
        let mut bounds: Option<aizome_kit::Rect> = None;
        for d in h.take_damage(w) {
            bounds = Some(bounds.map_or(d, |b| b.union(d)));
        }
        if let Some(b) = bounds {
            drawn.set(drawn.get() + b.area() / window_area);
            let _ = h.render_rect(w, b, 2.0);
        }
        start.elapsed()
    };
    let report = |name: &str, mut v: Vec<Duration>| {
        v.sort();
        let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
        let share = drawn.replace(0.0) / v.len() as f64 * 100.0;
        eprintln!(
            "tenki {name}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x, {share:.0}% drawn)",
            p(0.5),
            p(0.95),
            v.last().unwrap(),
            v.len()
        );
        p(0.95)
    };
    // Choosing another place: the whole main view changes.
    let mut select = Vec::new();
    for i in 0..30 {
        let at = rows[i % 3];
        select.push(frame(&mut h, &mut |h| h.click(w, at)));
    }
    let p_select = report("choose a place", select);
    // Hovering the list.
    let mut hover = Vec::new();
    for i in 0..60 {
        let at = Point::new(rows[i % 3].x, rows[i % 3].y + (i % 7) as f64);
        hover.push(frame(&mut h, &mut |h| h.move_to(w, at)));
    }
    let p_hover = report("hover the places", hover);
    // Dragging a place down the list and back: the rows make room.
    let mut drags = Vec::new();
    h.press(w, rows[0], aizome_kit::sidestep_ui::PointerButton::Left, 1);
    for i in 0..60 {
        let y = rows[0].y + 2.0 * PLACE_ROW * ((i as f64 / 30.0) * std::f64::consts::PI).sin().abs();
        let at = Point::new(rows[0].x, y);
        drags.push(frame(&mut h, &mut |h| h.move_to(w, at)));
    }
    h.release(w, rows[0], aizome_kit::sidestep_ui::PointerButton::Left);
    let p_drag = report("drag a place", drags);
    // Scrolling the hours sideways.
    let hours = h.find(w, "Now").expect("the hourly strip").center();
    let mut scrolls = Vec::new();
    for i in 0..40 {
        let d = if (i / 10) % 2 == 0 { 40.0 } else { -40.0 };
        scrolls.push(frame(&mut h, &mut |h| h.scroll(w, hours, aizome_kit::Vec2::new(d, 0.0))));
    }
    let p_scroll = report("scroll the hours", scrolls);
    // The minute tick ("Updated 13 min ago").
    let mut ticks = Vec::new();
    for _ in 0..10 {
        ticks.push(frame(&mut h, &mut |h| h.advance(Duration::from_secs(60))));
    }
    let p_tick = report("the minute", ticks);
    if !cfg!(debug_assertions) {
        for (name, p) in [("hover", p_hover), ("drag", p_drag), ("scroll", p_scroll), ("tick", p_tick)] {
            assert!(p < Duration::from_millis(4), "{name} p95 {p:?}");
        }
        // Choosing a place redraws the whole main view: see docs/performance.md.
        assert!(p_select < Duration::from_millis(8), "choose p95 {p_select:?}");
    }
}
