//! The program: the places, their forecasts, searching, the location, and
//! how messages change them. The views are in [`crate::ui`].

use std::collections::{HashMap, HashSet};
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;

use crate::forecast::{self, Forecast};
use crate::location::{Cancellation, LocateError, Located};
use crate::places::{self, CURRENT_ID, Found, Origin, Place};
use crate::services::{FetchError, Services};
use crate::store::{Cached, Consent, Settings, State, Store};
use crate::time::Clock;
use crate::units::Units;

/// A forecast older than this is fetched again when its place shows.
pub const STALE: i64 = 20 * 60;
/// The places not showing are fetched again when older than this.
pub const HOURLY: i64 = 60 * 60;
/// A failed fetch is tried again no sooner than this.
pub const RETRY: i64 = 60;
/// How long typing waits before searching.
pub const SEARCH_DELAY: Duration = Duration::from_millis(250);

/// A place's weather, as far as Tenki knows it.
#[derive(Clone, Debug, Default)]
pub struct Weather {
    pub forecast: Option<Forecast>,
    /// When the forecast came (Unix time).
    pub fetched_at: i64,
    pub loading: bool,
    /// What went wrong the last time, if it did.
    pub error: Option<FetchError>,
    /// When it was last asked for.
    pub tried_at: i64,
}

/// The search for places to add.
#[derive(Clone, Debug, Default)]
pub struct Search {
    pub text: String,
    /// Bumped by each change, so an old answer is dropped.
    pub generation: u64,
    /// A request for the current generation is running. Older requests may
    /// still finish, but cannot hold this query busy or supply its results.
    pub busy: bool,
    /// What the last answered search found, and for what text.
    pub results: Vec<Found>,
    pub answered: Option<String>,
    pub error: Option<String>,
    /// Add the first place found once the answer comes (Return typed
    /// before it did).
    pub add_first: bool,
}

/// Finding where the machine is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Locating {
    #[default]
    Idle,
    Busy,
    Failed(LocateError),
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Show the place in row `n` of the sidebar.
    Select(usize),
    SelectNext,
    SelectPrevious,
    /// Move the place in row `from` to row `to`.
    Move(usize, usize),
    MoveSelected(isize),
    /// Forget the place in row `n`.
    Remove(usize),
    RemoveSelected,
    Search(String),
    RunSearch(u64),
    Searched(u64, Result<Vec<Found>, String>),
    /// Keep result `n` of the search.
    Add(usize),
    /// Keep the first result (Return in the search field).
    AddFirst,
    ClearSearch,
    FocusSearch,
    Refresh,
    /// A forecast came for the position keyed so, asked for at a time.
    Fetched(String, i64, Result<String, FetchError>),
    /// A minute went by.
    Minute,
    SetUnits(Units),
    AllowLocation,
    DeclineLocation,
    ToggleLocation,
    Locate,
    SearchInstead,
    Located(u64, Result<Located, LocateError>),
    /// The time zone's city was searched for.
    TimezoneFound(String, Result<Vec<Found>, String>),
    /// Another launch asked for the window.
    Activate,
    ShowSettings,
    CloseSettings,
    /// Units in Settings: 0 the locale's, 1 metric, 2 imperial.
    UnitsChoice(usize),
    /// Background work that ended with nothing to say.
    Idle,
}

pub struct Tenki {
    services: Arc<dyn Services>,
    store: Option<Store>,
    pub settings: Settings,
    /// The places kept, in the user's order.
    pub places: Vec<Place>,
    /// The current location, once known.
    pub current: Option<Place>,
    pub locating: Locating,
    location_generation: u64,
    location_cancel: Cancellation,
    /// The id of the place showing.
    pub selected: Option<String>,
    /// Forecasts by position (`Place::cache_key`).
    pub weather: HashMap<String, Weather>,
    pub search: Search,
    pub clock: Clock,
    /// The window.
    pub window: WindowKey,
    /// The one-instance socket, served once the app runs.
    pub instance: Option<UnixListener>,
    /// The Settings sheet, while it shows.
    pub settings_sheet: Option<WindowKey>,
    /// The time zone's city is being looked for.
    timezone_busy: bool,
    timezone_tried: i64,
}

impl Tenki {
    /// Tenki asking `services`, keeping its files in `store` (none: keep
    /// nothing, as tests may).
    pub fn new(services: Arc<dyn Services>, store: Option<Store>) -> Tenki {
        let settings = store.as_ref().map(Store::load_settings).unwrap_or_default();
        let places = store.as_ref().and_then(Store::load_places).unwrap_or_default();
        let state = store.as_ref().map(Store::load_state).unwrap_or_default();
        let current = state.located.map(|(lat, lon)| current_place(lat, lon, None));
        let mut t = Tenki {
            services,
            store,
            settings,
            places,
            current,
            locating: Locating::Idle,
            location_generation: 0,
            location_cancel: Cancellation::default(),
            selected: state.selected,
            weather: HashMap::new(),
            search: Search::default(),
            clock: Clock::H24,
            window: WindowKey::NONE,
            instance: None,
            settings_sheet: None,
            timezone_busy: false,
            timezone_tried: 0,
        };
        for key in t.rows().iter().map(Place::cache_key).collect::<Vec<_>>() {
            t.load_cached(&key);
        }
        t.fix_selection();
        t
    }

    /// The units showing: the user's choice, else the locale's.
    pub fn units(&self) -> Units {
        self.settings.units.unwrap_or_else(Units::from_locale)
    }

    pub fn now(&self) -> i64 {
        self.services.now()
    }

    /// The sidebar's rows: the current location (when allowed), then the
    /// places kept.
    pub fn rows(&self) -> Vec<Place> {
        let mut rows = Vec::with_capacity(self.places.len() + 1);
        if self.settings.location == Consent::Allowed {
            rows.push(self.current.clone().unwrap_or_else(|| Place {
                id: CURRENT_ID.into(),
                name: "Current Location".into(),
                region: String::new(),
                latitude: f64::NAN,
                longitude: f64::NAN,
                timezone: None,
                origin: Origin::Current,
            }));
        }
        rows.extend(self.places.iter().cloned());
        rows
    }

    /// How many rows at the top stay put (the current location).
    pub fn pinned(&self) -> usize {
        usize::from(self.settings.location == Consent::Allowed)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.rows().iter().position(|p| p.id == id)
    }

    pub fn selected_place(&self) -> Option<Place> {
        let id = self.selected.as_deref()?;
        self.rows().into_iter().find(|p| p.id == id)
    }

    /// The weather of a place with a position.
    pub fn weather_of(&self, place: &Place) -> Option<&Weather> {
        has_position(place).then(|| self.weather.get(&place.cache_key())).flatten()
    }

    fn fix_selection(&mut self) {
        let rows = self.rows();
        if self.selected.as_deref().is_some_and(|id| rows.iter().any(|p| p.id == id)) {
            return;
        }
        self.selected = rows.first().map(|p| p.id.clone());
    }

    fn load_cached(&mut self, key: &str) {
        if self.weather.get(key).is_some_and(|w| w.forecast.is_some()) {
            return;
        }
        let Some(cached) = self.store.as_ref().and_then(|s| s.load_forecast(key)) else { return };
        match forecast::parse(&cached.body) {
            Ok(f) => {
                let w = self.weather.entry(key.to_owned()).or_default();
                w.forecast = Some(f);
                w.fetched_at = cached.fetched_at;
            }
            Err(e) => log::warn!("the cached forecast {key} didn't read: {e}"),
        }
    }

    fn save_places(&self) {
        if let Some(s) = &self.store {
            s.save_places(&self.places);
            let keep: Vec<String> = self.rows().iter().filter(|p| has_position(p)).map(Place::cache_key).collect();
            s.prune_forecasts(&keep);
        }
    }

    fn save_state(&self) {
        if let Some(s) = &self.store {
            let located = self.current.as_ref().map(|p| (p.latitude, p.longitude));
            s.save_state(&State { selected: self.selected.clone(), located });
        }
    }

    fn save_settings(&self) {
        if let Some(s) = &self.store {
            s.save_settings(&self.settings);
        }
    }

    /// Run `work` off the program's thread (or on it, in tests), its
    /// answer coming back as a message.
    fn run(&self, cx: &mut Cx<Msg>, work: impl FnOnce() -> Msg + Send + 'static) {
        if self.services.inline() {
            cx.send(work());
        } else {
            cx.spawn(work);
        }
    }

    /// Fetch the forecasts at the positions of `places` that are older
    /// than `stale` seconds (and not being fetched, nor failed just now),
    /// one after another.
    fn refresh(&mut self, places: Vec<Place>, stale: i64, cx: &mut Cx<Msg>) {
        let now = self.now();
        let mut jobs = Vec::new();
        let mut seen = HashSet::new();
        for p in places.iter().filter(|p| has_position(p)) {
            let key = p.cache_key();
            if !seen.insert(key.clone()) {
                continue;
            }
            let w = self.weather.entry(key.clone()).or_default();
            let fresh = w.forecast.is_some() && now - w.fetched_at < stale;
            if w.loading || fresh || (w.error.is_some() && now - w.tried_at < RETRY && stale > 0) {
                continue;
            }
            w.loading = true;
            w.tried_at = now;
            jobs.push((key, p.latitude, p.longitude));
        }
        if jobs.is_empty() {
            return;
        }
        let services = self.services.clone();
        if services.inline() {
            for (key, lat, lon) in jobs {
                let r = services.forecast(lat, lon);
                cx.send(Msg::Fetched(key, services.now(), r));
            }
            return;
        }
        let tx = cx.stream();
        cx.spawn(move || {
            for (i, (key, lat, lon)) in jobs.into_iter().enumerate() {
                if i > 0 {
                    // One at a time, a little apart: Open-Meteo is free.
                    std::thread::sleep(Duration::from_millis(150));
                }
                let r = services.forecast(lat, lon);
                if !tx.send(Msg::Fetched(key, services.now(), r)) {
                    break;
                }
            }
            Msg::Idle
        });
    }

    fn refresh_selected(&mut self, stale: i64, cx: &mut Cx<Msg>) {
        if let Some(p) = self.selected_place() {
            self.refresh(vec![p], stale, cx);
        }
    }

    /// With nothing to show and no location, look for the time zone's
    /// city and keep it.
    fn ensure_timezone_city(&mut self, cx: &mut Cx<Msg>) {
        if !self.places.is_empty() || self.settings.location == Consent::Allowed || self.timezone_busy {
            return;
        }
        let now = self.now();
        if self.timezone_tried != 0 && now - self.timezone_tried < RETRY {
            return;
        }
        let Some(tz) = self.services.timezone() else { return };
        let Some(city) = places::timezone_city(&tz) else { return };
        self.timezone_busy = true;
        self.timezone_tried = now;
        let services = self.services.clone();
        self.run(cx, move || {
            let r = services.search(&city).map_err(|e| e.message()).and_then(|b| places::parse_search(&b));
            Msg::TimezoneFound(tz, r)
        });
    }

    fn select(&mut self, id: String, cx: &mut Cx<Msg>) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        self.selected = Some(id);
        self.save_state();
        self.refresh_selected(STALE, cx);
    }

    fn select_row(&mut self, n: usize, cx: &mut Cx<Msg>) {
        if let Some(p) = self.rows().get(n) {
            self.select(p.id.clone(), cx);
        }
    }

    fn locate(&mut self, cx: &mut Cx<Msg>) {
        if self.settings.location != Consent::Allowed || self.locating == Locating::Busy {
            return;
        }
        self.cancel_location();
        self.location_cancel = Cancellation::default();
        self.locating = Locating::Busy;
        let cancel = self.location_cancel.clone();
        let generation = self.location_generation;
        let services = self.services.clone();
        self.run(cx, move || Msg::Located(generation, services.locate(&cancel)));
    }

    fn cancel_location(&mut self) {
        self.location_cancel.cancel();
        self.location_generation += 1;
        self.locating = Locating::Idle;
    }

    fn decline_location(&mut self, cx: &mut Cx<Msg>) {
        self.cancel_location();
        self.settings.location = Consent::Declined;
        self.save_settings();
        self.fix_selection();
        self.ensure_timezone_city(cx);
    }

    fn move_row(&mut self, from: usize, to: usize) {
        let pinned = self.pinned();
        if from < pinned || to < pinned {
            return;
        }
        let (from, to) = (from - pinned, to - pinned);
        if from >= self.places.len() || from == to {
            return;
        }
        let p = self.places.remove(from);
        self.places.insert(to.min(self.places.len()), p);
        self.save_places();
    }

    fn remove_row(&mut self, n: usize, cx: &mut Cx<Msg>) {
        let rows = self.rows();
        let Some(p) = rows.get(n) else { return };
        let was_selected = self.selected.as_deref() == Some(p.id.as_str());
        if p.origin == Origin::Current {
            self.cancel_location();
            self.settings.location = Consent::Declined;
            self.save_settings();
        } else {
            self.places.retain(|q| q.id != p.id);
            self.save_places();
        }
        if was_selected {
            let rows = self.rows();
            self.selected = rows.get(n.min(rows.len().saturating_sub(1))).map(|p| p.id.clone());
            self.save_state();
            self.refresh_selected(STALE, cx);
        }
        self.ensure_timezone_city(cx);
    }

    fn add(&mut self, found: &Found, cx: &mut Cx<Msg>) {
        let place = found.place(Origin::Search);
        let id = place.id.clone();
        if !self.places.iter().any(|p| p.id == id) {
            self.load_cached(&place.cache_key());
            self.places.push(place);
            self.save_places();
        }
        self.search = Search { generation: self.search.generation + 1, ..Search::default() };
        self.selected = None;
        self.select(id, cx);
    }

    fn run_search(&mut self, cx: &mut Cx<Msg>) {
        let text = self.search.text.trim().to_owned();
        if text.chars().count() < 2 {
            return;
        }
        self.search.busy = true;
        let generation = self.search.generation;
        let services = self.services.clone();
        self.run(cx, move || {
            let r = services.search(&text).map_err(|e| e.message()).and_then(|b| places::parse_search(&b));
            Msg::Searched(generation, r)
        });
    }
}

/// Whether a place has a position yet (the current location may not).
pub fn has_position(p: &Place) -> bool {
    p.latitude.is_finite() && p.longitude.is_finite()
}

/// The current location's row at a position.
pub fn current_place(lat: f64, lon: f64, description: Option<String>) -> Place {
    let (lat, lon) = (forecast::round2(lat), forecast::round2(lon));
    Place {
        id: CURRENT_ID.into(),
        name: "Current Location".into(),
        region: description.unwrap_or_else(|| places::coordinates(lat, lon)),
        latitude: lat,
        longitude: lon,
        timezone: None,
        origin: Origin::Current,
    }
}

impl Drop for Tenki {
    fn drop(&mut self) {
        self.location_cancel.cancel();
    }
}

impl Program for Tenki {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(crate::APP_ID, "Tenki", "Weather").icon(Icon::CloudSun).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        if !self.services.inline() {
            self.clock = Clock::from_desktop();
        }
        self.window = cx.open_window(WindowSpec::new("Tenki").size(980.0, 720.0).min_size(640.0, 460.0));
        if let Some(listener) = self.instance.take() {
            let tx = cx.stream();
            crate::instance::serve(listener, tx, || Msg::Activate);
        }
        cx.every(Duration::from_secs(60), Msg::Minute);
        if self.settings.location == Consent::Allowed {
            self.locate(cx);
        }
        self.ensure_timezone_city(cx);
        // The one showing first, then the rest.
        self.refresh_selected(STALE, cx);
        let rows = self.rows();
        self.refresh(rows, STALE, cx);
        if let Some(p) = self.selected_place() {
            cx.set_title(self.window, p.name);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Select(n) => self.select_row(n, cx),
            Msg::SelectNext | Msg::SelectPrevious => {
                let n = self.rows().len();
                if n == 0 {
                    return;
                }
                let i = self.selected_index().unwrap_or(0);
                let j = if matches!(msg, Msg::SelectNext) { (i + 1).min(n - 1) } else { i.saturating_sub(1) };
                self.select_row(j, cx);
            }
            Msg::Move(from, to) => self.move_row(from, to),
            Msg::MoveSelected(by) => {
                if let Some(i) = self.selected_index() {
                    let to = i as isize + by;
                    if to >= self.pinned() as isize && (to as usize) < self.rows().len() {
                        self.move_row(i, to as usize);
                    }
                }
            }
            Msg::Remove(n) => self.remove_row(n, cx),
            Msg::RemoveSelected => {
                if let Some(i) = self.selected_index() {
                    self.remove_row(i, cx);
                }
            }
            Msg::Search(text) => {
                if text == self.search.text {
                    return;
                }
                self.search.text = text;
                self.search.generation += 1;
                self.search.add_first = false;
                // All transient state belongs to the query just replaced.
                // In particular, a stale reply is discarded below, so it must
                // not be the only thing that could release this query's busy flag.
                self.search.busy = false;
                self.search.results.clear();
                self.search.answered = None;
                self.search.error = None;
                if self.search.text.trim().chars().count() < 2 {
                    return;
                }
                cx.after(SEARCH_DELAY, Msg::RunSearch(self.search.generation));
            }
            Msg::RunSearch(g) => {
                if g == self.search.generation && !self.search.busy {
                    self.run_search(cx);
                }
            }
            Msg::Searched(g, r) => {
                if g != self.search.generation {
                    return;
                }
                self.search.busy = false;
                self.search.answered = Some(self.search.text.trim().to_owned());
                match r {
                    Ok(found) => {
                        self.search.results = found;
                        self.search.error = None;
                    }
                    Err(e) => {
                        self.search.results.clear();
                        self.search.error = Some(e);
                    }
                }
                if std::mem::take(&mut self.search.add_first) {
                    cx.send(Msg::AddFirst);
                }
            }
            Msg::Add(i) => {
                if let Some(f) = self.search.results.get(i).cloned() {
                    self.add(&f, cx);
                }
            }
            Msg::AddFirst => {
                let answered = self.search.answered.as_deref() == Some(self.search.text.trim());
                if answered && !self.search.busy {
                    if let Some(f) = self.search.results.first().cloned() {
                        self.add(&f, cx);
                    }
                } else if self.search.text.trim().chars().count() >= 2 {
                    self.search.add_first = true;
                    if !self.search.busy {
                        self.run_search(cx);
                    }
                }
            }
            Msg::ClearSearch => {
                self.search = Search { generation: self.search.generation + 1, ..Search::default() };
            }
            Msg::FocusSearch => cx.focus("search"),
            Msg::Refresh => {
                let rows = self.rows();
                // Everything, now (but not twice at once).
                self.refresh(rows, 0, cx);
                if self.settings.location == Consent::Allowed && self.current.is_none() {
                    self.locate(cx);
                }
                self.ensure_timezone_city(cx);
            }
            Msg::Fetched(key, at, r) => {
                let w = self.weather.entry(key.clone()).or_default();
                w.loading = false;
                let parsed = r.and_then(|body| match forecast::parse(&body) {
                    Ok(f) => Ok((f, body)),
                    Err(e) => Err(FetchError::Bad(e)),
                });
                match parsed {
                    Ok((f, body)) => {
                        w.forecast = Some(f);
                        w.fetched_at = at;
                        w.error = None;
                        if let Some(s) = &self.store {
                            s.save_forecast(&key, &Cached { fetched_at: at, body });
                        }
                    }
                    Err(e) => {
                        log::info!("the forecast at {key}: {e:?}");
                        w.error = Some(e);
                    }
                }
            }
            Msg::Minute => {
                self.refresh_selected(STALE, cx);
                // The others, hourly, for the sidebar.
                let rows = self.rows();
                self.refresh(rows, HOURLY, cx);
                self.ensure_timezone_city(cx);
            }
            Msg::SetUnits(u) => {
                self.settings.units = Some(u);
                self.save_settings();
            }
            Msg::AllowLocation => {
                self.settings.location = Consent::Allowed;
                self.save_settings();
                self.selected = Some(CURRENT_ID.into());
                self.save_state();
                if let Some(p) = self.current.clone() {
                    self.load_cached(&p.cache_key());
                    self.refresh(vec![p], STALE, cx);
                }
                self.locate(cx);
            }
            Msg::DeclineLocation => self.decline_location(cx),
            Msg::ToggleLocation => {
                if self.settings.location == Consent::Allowed {
                    self.decline_location(cx);
                } else {
                    cx.send(Msg::AllowLocation);
                }
            }
            Msg::Locate => self.locate(cx),
            Msg::SearchInstead => {
                self.decline_location(cx);
                cx.send(Msg::FocusSearch);
            }
            Msg::Located(generation, r) => {
                if generation != self.location_generation || self.settings.location != Consent::Allowed {
                    return;
                }
                match r {
                    Ok(l) => {
                        self.locating = Locating::Idle;
                        let p = current_place(l.latitude, l.longitude, l.description);
                        self.load_cached(&p.cache_key());
                        self.current = Some(p.clone());
                        self.save_state();
                        if self.settings.location == Consent::Allowed {
                            self.refresh(vec![p], STALE, cx);
                        }
                    }
                    Err(LocateError::Cancelled) => self.locating = Locating::Idle,
                    Err(e) => {
                        log::info!("no location: {e:?}");
                        self.locating = Locating::Failed(e);
                    }
                }
            }
            Msg::TimezoneFound(tz, r) => {
                self.timezone_busy = false;
                let Ok(found) = r else { return };
                let pick =
                    places::in_timezone(&found, &tz).or_else(|| found.iter().max_by_key(|f| f.population.unwrap_or(0)));
                let Some(f) = pick else { return };
                if self.places.is_empty() {
                    let p = f.place(Origin::TimeZone);
                    self.load_cached(&p.cache_key());
                    self.places.push(p.clone());
                    self.save_places();
                    self.fix_selection();
                    self.save_state();
                    self.refresh(vec![p], STALE, cx);
                }
            }
            Msg::Activate => cx.activate(self.window),
            Msg::ShowSettings => {
                if self.settings_sheet.is_none() {
                    let sheet = cx.show_sheet(self.window);
                    self.settings_sheet = (sheet != WindowKey::NONE).then_some(sheet);
                }
            }
            Msg::CloseSettings => {
                if let Some(s) = self.settings_sheet.take() {
                    cx.close_window(s);
                }
            }
            Msg::UnitsChoice(i) => {
                self.settings.units = match i {
                    1 => Some(Units::Metric),
                    2 => Some(Units::Imperial),
                    _ => None,
                };
                self.save_settings();
            }
            Msg::Idle => {}
        }
        // The window is named for the place it shows (its tab, the Deskbar).
        let title = self.selected_place().map_or_else(|| "Tenki".to_owned(), |p| p.name);
        cx.set_title(self.window, title);
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.settings_sheet {
            return crate::ui::settings(self);
        }
        crate::ui::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let has_selection = self.selected_index().is_some();
        let movable = self.selected_index().is_some_and(|i| i >= self.pinned());
        let places = Menu::new("Places")
            .item(MenuItem::new("Add Place…", Msg::FocusSearch).shortcut(Shortcut::cmd('N')))
            .item(MenuItem::new("Remove Place", Msg::RemoveSelected).enabled(has_selection))
            .separator()
            .item(MenuItem::new("Previous Place", Msg::SelectPrevious).shortcut(Shortcut::cmd('[')))
            .item(MenuItem::new("Next Place", Msg::SelectNext).shortcut(Shortcut::cmd(']')))
            .item(MenuItem::new("Move Up", Msg::MoveSelected(-1)).enabled(movable))
            .item(MenuItem::new("Move Down", Msg::MoveSelected(1)).enabled(movable))
            .separator()
            .item(
                MenuItem::new("Show Current Location", Msg::ToggleLocation)
                    .checked(self.settings.location == Consent::Allowed)
                    .icon(Icon::LocateFixed),
            );
        let units = self.units();
        let view = Menu::new("View")
            .item(MenuItem::new("Refresh", Msg::Refresh).shortcut(Shortcut::cmd('R')).icon(Icon::RefreshCw))
            .separator()
            .header("Units")
            .item(MenuItem::new(Units::Metric.label(), Msg::SetUnits(Units::Metric)).checked(units == Units::Metric))
            .item(
                MenuItem::new(Units::Imperial.label(), Msg::SetUnits(Units::Imperial))
                    .checked(units == Units::Imperial),
            );
        // The app menu as standard, its Settings… opening Tenki's.
        let role = |label: &str, role: MenuRole| MenuItem::role_item(label, role);
        let app = Menu::new("Tenki")
            .item(role("About Tenki", MenuRole::About))
            .separator()
            .item(MenuItem::new("Settings…", Msg::ShowSettings).shortcut(Shortcut::cmd(',')))
            .separator()
            .submenu(Menu::new("Services").item(role("No Services Apply", MenuRole::Services).enabled(false)))
            .separator()
            .item(role("Hide Tenki", MenuRole::Hide).shortcut(Shortcut::cmd('H')))
            .item(role("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
            .separator()
            .item(role("Quit Tenki", MenuRole::Quit).shortcut(Shortcut::cmd('Q')));
        MenuBar::standard(&self.info()).replace("Tenki", app).insert(1, places).with_menu(view)
    }
}
