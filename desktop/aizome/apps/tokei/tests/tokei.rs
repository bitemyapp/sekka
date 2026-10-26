//! Tokei headlessly, on a clock the test moves: world clocks, adding a
//! city, alarms scheduled through a recording scheduler (no timers are
//! made on this machine), the timer, the stopwatch, what a tick redraws
//! and costs, and golden images in Night and Day.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_tokei::alarms::{self, Alarm, Recorder};
use aizome_tokei::app::{Msg, Page, Place, Sheet, Tokei};
use aizome_tokei::clock::{self, Clock};
use aizome_tokei::sound::Silent;
use aizome_tokei::timing::TimerState;
use jiff::Timestamp;
use jiff::tz::TimeZone;

type Moved = Rc<Cell<(Timestamp, Duration)>>;

struct Rig {
    h: Harness<Tokei>,
    time: Moved,
    sched: Recorder,
    alarms_path: PathBuf,
}

/// Tokei at 21:42:07 on Saturday 17 October 2026 in Berlin, with its
/// alarms in a file of the test's own.
fn rig(name: &str, theme: ThemeChoice, places: &[(&str, &str)], alarms: &[Alarm]) -> Rig {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tokei-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let alarms_path = dir.join("alarms.json");
    alarms::save(&alarms_path, alarms).unwrap();
    let sched = Recorder::new();
    let mut app = Tokei::new(None, Some(alarms_path.clone()), Box::new(sched.clone()), Box::new(Silent::default()));
    let (c, time) = Clock::fake("2026-10-17T19:42:07.250Z".parse().unwrap());
    app.clock = c;
    app.now = app.clock.now();
    app.local = TimeZone::get("Europe/Berlin").unwrap();
    app.h24 = true;
    app.quiet = true;
    app.places = places.iter().map(|(n, z)| Place { name: (*n).into(), zone: (*z).into() }).collect();
    let h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
    Rig { h, time, sched, alarms_path }
}

impl Rig {
    /// Move the clock and the harness's timers on together.
    fn wait(&mut self, d: Duration) {
        let step = Duration::from_millis(10);
        let mut left = d;
        while !left.is_zero() {
            let s = step.min(left);
            clock::advance(&self.time, s);
            self.h.advance(s);
            left -= s;
        }
    }
}

fn texts(h: &Harness<Tokei>) -> String {
    h.texts(h.window()).join("")
}

fn alarm(id: &str, h: u8, m: u8, days: &[u8], label: &str) -> Alarm {
    Alarm { id: id.into(), hour: h, minute: m, days: days.to_vec(), label: label.into(), ..Alarm::default() }
}

const CITIES: [(&str, &str); 4] = [
    ("Tokyo", "Asia/Tokyo"),
    ("New York", "America/New_York"),
    ("Kolkata", "Asia/Kolkata"),
    ("Reykjavík", "Atlantic/Reykjavik"),
];

#[test]
fn world_clocks_show_time_day_and_offset() {
    let r = rig("world", ThemeChoice::Night, &CITIES, &[]);
    let all = texts(&r.h);
    assert!(all.contains("21:42:07"), "the local clock: {all}");
    assert!(all.contains("Saturday 17 October 2026 · Berlin"), "{all}");
    assert!(all.contains("04:42") && all.contains("Tomorrow · +7 h"), "Tokyo: {all}");
    assert!(all.contains("15:42") && all.contains("Today · −6 h"), "New York: {all}");
    assert!(all.contains("01:12") && all.contains("Tomorrow · +3 h 30 min"), "Kolkata: {all}");
    assert!(all.contains("Today · −2 h"), "Reykjavík: {all}");
    // Night in Tokyo at 04:42, day in New York at 15:42.
    let sem = r.h.semantics(r.h.window());
    let said: Vec<String> = sem.iter().filter_map(|(s, _)| s.label.clone()).collect();
    assert!(said.iter().any(|l| l == "night") && said.iter().any(|l| l == "day"), "{said:?}");
}

#[test]
fn a_tick_redraws_only_the_digits_that_changed() {
    let mut r = rig("tick", ThemeChoice::Night, &CITIES, &[]);
    let w = r.h.window();
    r.wait(Duration::from_millis(800));
    r.h.take_damage(w);
    // 21:42:08 → 21:42:09: the last digit.
    r.wait(Duration::from_secs(1));
    let damage = r.h.take_damage(w);
    assert!(texts(&r.h).contains("21:42:09"));
    let area: f64 = damage.iter().map(|d| d.area()).sum();
    assert_eq!(damage.len(), 1, "{damage:?}");
    assert!(damage[0].width() < 50.0 && area < 50.0 * 80.0, "one digit: {damage:?}");
    // 21:42:59 → 21:43:00: four digits of the big clock, and the minute
    // of every city.
    r.wait(Duration::from_secs(50));
    r.h.take_damage(w);
    r.wait(Duration::from_secs(1));
    let damage = r.h.take_damage(w);
    assert!(texts(&r.h).contains("21:43:00"));
    let size = r.h.size(w);
    let share = damage.iter().map(|d| d.area()).sum::<f64>() / (size.width * size.height);
    assert!(share < 0.08, "{:.1}% of the window: {damage:?}", share * 100.0);
}

#[test]
fn a_tick_fits_the_frame_budget() {
    let mut r = rig("budget", ThemeChoice::Night, &CITIES, &[]);
    let w = r.h.window();
    let _ = r.h.render(w, 2.0);
    r.wait(Duration::from_millis(800));
    let mut frames = Vec::new();
    for i in 0..120 {
        r.h.take_damage(w);
        clock::advance(&r.time, Duration::from_secs(1));
        let start = Instant::now();
        r.h.advance(Duration::from_secs(1));
        let damage = r.h.take_damage(w);
        for d in &damage {
            let _ = r.h.render_rect(w, *d, 2.0);
        }
        if i > 2 {
            frames.push(start.elapsed());
        }
    }
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "tokei world clock tick: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    if !cfg!(debug_assertions) {
        assert!(p(0.95) < Duration::from_millis(4));
    }
    // The stopwatch's frames, at 25 a second.
    r.h.send(Msg::Page(Page::Stopwatch));
    r.h.send(Msg::WatchToggle);
    let mut frames = Vec::new();
    for i in 0..100 {
        r.h.take_damage(w);
        clock::advance(&r.time, Duration::from_millis(40));
        let start = Instant::now();
        r.h.advance(Duration::from_millis(40));
        for d in r.h.take_damage(w) {
            let _ = r.h.render_rect(w, d, 2.0);
        }
        if i > 2 {
            frames.push(start.elapsed());
        }
    }
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "tokei stopwatch frame: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
}

#[test]
fn cities_are_found_added_and_removed() {
    let mut r = rig("cities", ThemeChoice::Night, &[], &[]);
    let w = r.h.window();
    assert!(texts(&r.h).contains("No cities yet"));
    r.h.command(w, 'n');
    assert!(matches!(r.h.program().sheet, Some(Sheet::AddCity { .. })));
    assert_eq!(r.h.focused_name(w).as_deref(), Some("city-search"));
    r.h.type_text(w, "osaka");
    assert!(r.h.texts(w).iter().any(|t| t == "Osaka"));
    r.h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(r.h.program().sheet.is_none());
    assert_eq!(r.h.program().places, [Place { name: "Osaka".into(), zone: "Asia/Tokyo".into() }]);
    // Another, picked from the list with a click.
    r.h.command(w, 'n');
    r.h.type_text(w, "rio");
    let row = r.h.find(w, "Rio de Janeiro").expect("a result");
    r.h.click(w, row.center());
    r.h.click_text(w, "Add");
    assert_eq!(r.h.program().places.len(), 2);
    // Escape clears the search, then closes the sheet without adding.
    r.h.command(w, 'n');
    r.h.type_text(w, "paris");
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(matches!(&r.h.program().sheet, Some(Sheet::AddCity { query, .. }) if query.is_empty()));
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(r.h.program().sheet.is_none());
    assert_eq!(r.h.program().places.len(), 2);
    // The selected city goes with ❖⌫; Delete works too.
    r.h.send(Msg::SelectPlace(vec![0]));
    r.h.key(w, aizome_kit::sidestep_ui::Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    assert_eq!(r.h.program().places.len(), 1);
    assert_eq!(r.h.program().places[0].name, "Rio de Janeiro");
}

#[test]
fn city_reorder_shortcuts_move_selection_and_respect_boundaries() {
    use aizome_kit::Program;
    use aizome_kit::menu::{ItemSpec, Shortcut, ShortcutKey};

    let mut r = rig("reorder-keys", ThemeChoice::Night, &CITIES, &[]);
    let w = r.h.window();
    r.h.click_text(w, "Kolkata");
    let names = |h: &Harness<Tokei>| h.program().places.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    let action = |h: &Harness<Tokei>, label: &str| {
        h.program()
            .menus()
            .menus()
            .iter()
            .find(|m| m.title == "Clock")
            .unwrap()
            .items
            .iter()
            .find_map(|i| match i {
                ItemSpec::Action(a) if a.label == label => Some(a.clone()),
                _ => None,
            })
            .unwrap()
    };
    let reorder = Modifiers::SUPER | Modifiers::SHIFT;
    r.h.named(w, NamedKey::ArrowUp, reorder);
    assert_eq!(names(&r.h), ["Tokyo", "Kolkata", "New York", "Reykjavík"]);
    assert_eq!(r.h.program().selected_place, Some(1));
    assert_eq!(action(&r.h, "Move Up").shortcut, Some(Shortcut::cmd_key(ShortcutKey::Up).shift()));
    assert_eq!(action(&r.h, "Move Down").shortcut, Some(Shortcut::cmd_key(ShortcutKey::Down).shift()));
    r.h.named(w, NamedKey::ArrowUp, reorder);
    let first = names(&r.h);
    assert_eq!(first[0], "Kolkata");
    assert!(!action(&r.h, "Move Up").enabled);
    r.h.named(w, NamedKey::ArrowUp, reorder);
    assert_eq!(names(&r.h), first);
    for _ in 0..3 {
        r.h.named(w, NamedKey::ArrowDown, reorder);
    }
    let last = names(&r.h);
    assert_eq!(last, ["Tokyo", "New York", "Reykjavík", "Kolkata"]);
    assert_eq!(r.h.program().selected_place, Some(3));
    assert!(!action(&r.h, "Move Down").enabled);
    r.h.named(w, NamedKey::ArrowDown, reorder);
    assert_eq!(names(&r.h), last);
    // These belong to the compositor (Fill / Minimise), not city ordering.
    r.h.named(w, NamedKey::ArrowUp, Modifiers::SUPER | Modifiers::ALT);
    r.h.named(w, NamedKey::ArrowDown, Modifiers::SUPER | Modifiers::ALT);
    assert_eq!(names(&r.h), last);
}

#[test]
fn alarms_are_made_scheduled_and_turned_off() {
    let mut r = rig("alarms", ThemeChoice::Night, &[], &[alarm("00000001", 6, 45, &[1, 2, 3, 4, 5], "Work")]);
    let w = r.h.window();
    // Launching scheduled the alarm there was.
    assert_eq!(r.sched.units(), ["aizome-alarm-00000001"]);
    r.h.command(w, '2');
    assert_eq!(r.h.program().page, Page::Alarms);
    assert!(texts(&r.h).contains("Work · Weekdays"));
    assert!(texts(&r.h).contains("Next: Monday 06:45 (Work), in 1 day 9 h"), "{}", texts(&r.h));
    // A new one: 7:30 tomorrow, labelled, waking the computer.
    r.h.command(w, 'n');
    assert_eq!(r.h.focused_name(w).as_deref(), Some("alarm-time"));
    r.h.send(Msg::DraftTime(String::new()));
    r.h.type_text(w, "7:30");
    r.h.send(Msg::DraftLabel("Swim".into()));
    r.h.send(Msg::DraftWake(true));
    r.h.click_text(w, "Save");
    let alarms = alarms::load(&r.alarms_path);
    assert_eq!(alarms.len(), 2);
    let swim = alarms.iter().find(|a| a.label == "Swim").expect("saved");
    assert_eq!((swim.hour, swim.minute, swim.wake, swim.enabled), (7, 30, true, true));
    let unit = format!("aizome-alarm-{}", swim.id);
    let calls = r.sched.calls();
    let run = calls.iter().rev().find(|c| c.contains(&unit) && c.starts_with("systemd-run")).expect("scheduled");
    assert!(run.contains("--on-calendar=2026-10-18 07:30:00"), "{run}");
    assert!(run.contains("--timer-property=WakeSystem=true"), "{run}");
    assert!(run.ends_with(&format!("--ring {}", swim.id)), "{run}");
    // A bad time can't be saved.
    r.h.command(w, 'n');
    r.h.send(Msg::DraftTime("25:00".into()));
    assert!(
        matches!(&r.h.program().sheet, Some(Sheet::Alarm { error: Some(e), .. }) if e == "A time such as 7:30 or 19:30")
    );
    r.h.click_text(w, "Save");
    assert!(r.h.program().sheet.is_some(), "not saved");
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    // Off: unscheduled.
    let i = r.h.program().alarms.iter().position(|a| a.label == "Swim").unwrap();
    r.h.send(Msg::ToggleAlarm(i, false));
    assert!(!r.sched.units().contains(&unit));
    assert!(!alarms::load(&r.alarms_path)[i].enabled);
    // Deleted, and back with ❖Z.
    r.h.send(Msg::SelectAlarm(vec![0]));
    r.h.key(w, aizome_kit::sidestep_ui::Key::Named(NamedKey::Delete), Modifiers::NONE);
    assert_eq!(alarms::load(&r.alarms_path).len(), 1);
    assert!(!r.sched.units().contains(&"aizome-alarm-00000001".to_owned()));
    r.h.command(w, 'z');
    assert_eq!(alarms::load(&r.alarms_path).len(), 2);
    assert!(r.sched.units().contains(&"aizome-alarm-00000001".to_owned()));
}

#[test]
fn the_timer_counts_down_and_says_when_its_done() {
    let mut r = rig("timer", ThemeChoice::Night, &[], &[]);
    let w = r.h.window();
    r.h.command(w, '3');
    r.h.click_text(w, "1 min");
    assert!(r.h.program().timer.running());
    assert!(texts(&r.h).contains("01:00"));
    assert!(texts(&r.h).contains("Ends 21:43"), "{}", texts(&r.h));
    r.wait(Duration::from_secs(20));
    assert!(texts(&r.h).contains("00:40"), "{}", texts(&r.h));
    // Space pauses (the window has the keys).
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character(" ".into()), Modifiers::NONE);
    assert!(matches!(r.h.program().timer.state, TimerState::Paused { .. }));
    r.wait(Duration::from_secs(30));
    assert!(texts(&r.h).contains("00:40"));
    r.h.click_text(w, "Resume");
    r.wait(Duration::from_secs(41));
    assert_eq!(r.h.program().timer.state, TimerState::Done);
    assert_eq!(r.h.program().notified, ["Timer done: 01:00"]);
    // A typed length.
    r.h.send(Msg::TimerCancel);
    r.h.send(Msg::TimerEntry("90s".into()));
    r.h.send(Msg::TimerSet);
    assert!(texts(&r.h).contains("01:30"));
}

#[test]
fn the_stopwatch_runs_and_takes_laps() {
    let mut r = rig("watch", ThemeChoice::Night, &[], &[]);
    let w = r.h.window();
    r.h.command(w, '4');
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character(" ".into()), Modifiers::NONE);
    // Frames come every 40 ms.
    r.wait(Duration::from_millis(1234));
    assert!(texts(&r.h).contains("00:01.20"), "{}", texts(&r.h));
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("l".into()), Modifiers::NONE);
    r.wait(Duration::from_millis(2000));
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("l".into()), Modifiers::NONE);
    r.wait(Duration::from_millis(500));
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character(" ".into()), Modifiers::NONE);
    let sw = &r.h.program().stopwatch;
    assert_eq!(sw.laps.len(), 2);
    assert!(!sw.running());
    let all = texts(&r.h);
    assert!(all.contains("Lap 3") && all.contains("Lap 1"), "{all}");
    assert!(all.contains("00:03.73"), "{all}");
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("r".into()), Modifiers::NONE);
    assert!(!r.h.program().stopwatch.started());
}

#[test]
fn ringing_a_once_alarm_turns_it_off() {
    let r = rig("ring", ThemeChoice::Night, &[], &[alarm("0000abcd", 9, 0, &[], "")]);
    let mut n = aizome_tokei::ring::Scripted {
        posted: Vec::new(),
        answer: Some(aizome_tokei::ring::Answer::Stop),
        closed: false,
    };
    let mut p = Silent::default();
    let mut s = Recorder::new();
    aizome_tokei::ring::ring_with(&r.alarms_path, "0000abcd", &mut n, &mut p, &mut s, aizome_tokei::ring::RING_FOR);
    assert_eq!(n.posted[0].0, "Alarm");
    assert!(!alarms::load(&r.alarms_path)[0].enabled);
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

fn pictures(r: &mut Rig, name: &str) {
    let w = r.h.window();
    r.h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tokei-{name}@2x.png")), 2.0);
    if !r.h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)");
        return;
    }
    assert_golden(&mut r.h, w, golden(name), 1.0);
}

#[test]
fn goldens() {
    let alarms_list = [
        alarm("00000001", 6, 45, &[1, 2, 3, 4, 5], "Work"),
        alarm("00000002", 9, 30, &[6, 7], "Long run"),
        Alarm { enabled: false, ..alarm("00000003", 14, 0, &[], "") },
    ];
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut r = rig(&format!("goldens-{t}"), theme, &CITIES, &alarms_list);
        r.h.send(Msg::SelectPlace(vec![1]));
        pictures(&mut r, &format!("world-{t}"));
        r.h.send(Msg::Page(Page::Alarms));
        pictures(&mut r, &format!("alarms-{t}"));
        r.h.send(Msg::EditAlarm(0));
        pictures(&mut r, &format!("alarm-sheet-{t}"));
        r.h.send(Msg::CloseSheet);
        r.h.send(Msg::Page(Page::Timer));
        r.h.send(Msg::TimerPreset(5));
        r.wait(Duration::from_secs(72));
        pictures(&mut r, &format!("timer-{t}"));
        r.h.send(Msg::Page(Page::Stopwatch));
        r.h.send(Msg::WatchToggle);
        for ms in [3_210, 2_870, 3_450] {
            r.wait(Duration::from_millis(ms));
            r.h.send(Msg::WatchLap);
        }
        r.wait(Duration::from_millis(1_120));
        pictures(&mut r, &format!("stopwatch-{t}"));
        r.h.send(Msg::Page(Page::World));
        r.h.send(Msg::AddCity);
        r.h.send(Msg::CityQuery("san".into()));
        pictures(&mut r, &format!("add-city-{t}"));
    }
}
