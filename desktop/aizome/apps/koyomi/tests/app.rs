//! Koyomi headlessly: navigation by keys and menus, making, moving,
//! resizing and deleting events by pointer, the editor, undo, series,
//! the calendars, import, files changed by others, and golden images of
//! the views in Night and Day.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use aizome_kit::{Point, Rect, Vec2};
use common::*;
use jiff::civil::{date, datetime};
use koyomi::app::{Koyomi, Msg, View};
use koyomi::model::Calendar;

/// Where the occurrences of `title` start, as `YYYY-MM-DDTHH:MM`.
fn starts(h: &Harness<Koyomi>, title: &str) -> Vec<String> {
    let k = h.program();
    k.occ
        .iter()
        .filter(|o| k.store.calendars[o.cal].events[o.event].summary == title)
        .map(|o| o.start.strftime("%Y-%m-%dT%H:%M").to_string())
        .collect()
}

fn ends(h: &Harness<Koyomi>, title: &str) -> Vec<String> {
    let k = h.program();
    k.occ
        .iter()
        .filter(|o| k.store.calendars[o.cal].events[o.event].summary == title)
        .map(|o| o.end.strftime("%H:%M").to_string())
        .collect()
}

/// The calendar file `id` as it is on disk now.
fn on_disk(dir: &Path, id: &str) -> Calendar {
    Calendar::parse(id, &std::fs::read_to_string(dir.join(format!("{id}.ics"))).unwrap()).0
}

fn dir_of(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("koyomi-{name}"))
}

fn union(damage: &[Rect]) -> Option<Rect> {
    damage.iter().copied().reduce(|a, b| a.union(b))
}

#[test]
fn keys_and_menus_move_through_time() {
    let mut h = harness("keys", ThemeChoice::Night);
    let w = h.window();
    assert_eq!(h.program().view, View::Month);
    assert_eq!(h.focused_name(w).as_deref(), Some("calendar"));
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().anchor, date(2026, 11, 17));
    assert!(h.texts(w).iter().any(|t| t == "November 2026"));
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.program().anchor, date(2026, 9, 17));
    h.key(w, Key::Character("t".into()), Modifiers::NONE);
    assert_eq!(h.program().anchor, date(2026, 10, 17));
    // ❖1–4 switch views.
    h.command(w, '2');
    assert_eq!(h.program().view, View::Week);
    assert_eq!(h.program().range(), (date(2026, 10, 12), date(2026, 10, 18)));
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().range(), (date(2026, 10, 19), date(2026, 10, 25)));
    h.command(w, '1');
    assert_eq!(h.program().range(), (date(2026, 10, 24), date(2026, 10, 24)));
    h.command(w, '4');
    assert_eq!(h.program().view, View::Year);
    h.command(w, 't');
    h.command(w, '3');
    assert_eq!(h.program().anchor, date(2026, 10, 17));
    // The mini-month browses on its own, and a day picked there is shown.
    let mini = h.find_id(w, "mini-month").expect("the mini-month");
    h.click(w, Point::new(mini.x1 - 14.0, mini.y0 + 14.0));
    assert_eq!(h.program().mini, (2026, 11));
    assert_eq!(h.program().anchor, date(2026, 10, 17));
    // Row 3, Monday: 16 November.
    let cw = mini.width() / 7.0;
    h.click(w, Point::new(mini.x0 + cw * 0.5, mini.y0 + 28.0 + 18.0 + 24.0 * 3.5));
    assert_eq!(h.program().anchor, date(2026, 11, 16));
}

#[test]
fn the_locale_and_settings_order_the_week() {
    let dir = folder("weeks");
    let mut opts = koyomi::app::Options::test(&dir, datetime(2026, 10, 17, 14, 20, 0, 0), "America/New_York");
    opts.weeks = Some(koyomi::locale::Weeks::for_locale("en_US.UTF-8"));
    let mut h = Harness::new(Koyomi::new(opts));
    h.send(Msg::SetView(View::Week));
    assert_eq!(h.program().range(), (date(2026, 10, 11), date(2026, 10, 17)));
    h.send(Msg::FirstDay(koyomi::settings::FirstDay::Monday));
    assert_eq!(h.program().range(), (date(2026, 10, 12), date(2026, 10, 18)));
    // London's 09:30 stand-up is 04:30 in New York.
    assert!(starts(&h, "Stand-up").contains(&"2026-10-13T04:30".to_owned()));
}

#[test]
fn dragging_in_the_week_moves_resizes_and_makes_events() {
    let mut h = harness("drag", ThemeChoice::Night);
    let w = h.window();
    h.send(Msg::SetView(View::Week));
    let review = h.find(w, "Design review: Koyomi").expect("the review's block");
    let grid = h.find_id(w, "time-grid").expect("the grid");
    let col = (grid.width() - 56.0) / 7.0;
    // A press selects it.
    h.click(w, review.center());
    assert_eq!(h.program().selected.as_ref().map(|s| s.uid.as_str()), Some("review@test"));
    // Drag it a day on and an hour later: while dragging, nothing but the
    // ghost is redrawn and the program hears nothing.
    let from = review.center();
    h.press(w, from, PointerButton::Left, 1);
    let mut largest = 0.0f64;
    for i in 1..=10 {
        let p = from + Vec2::new(col * i as f64 / 10.0, 48.0 * i as f64 / 10.0);
        h.take_damage(w);
        h.move_to(w, p);
        if let Some(d) = union(&h.take_damage(w)) {
            largest = largest.max(d.area());
        }
    }
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-14T14:00"], "the event moves only when dropped");
    assert!(largest < col * 48.0 * 4.0, "a drag step redraws the ghost's ground only ({largest}pt²)");
    h.release(w, from + Vec2::new(col, 48.0), PointerButton::Left);
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-15T15:00"]);
    assert_eq!(ends(&h, "Design review: Koyomi"), ["16:30"]);
    let dir = dir_of("drag");
    let saved = on_disk(&dir, "work");
    let e = saved.events.iter().find(|e| e.uid == "review@test").unwrap();
    assert_eq!(e.start.civil(), datetime(2026, 10, 15, 15, 0, 0, 0));
    // Undo puts it back, on disk too.
    h.command(w, 'z');
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-14T14:00"]);
    assert_eq!(
        on_disk(&dir, "work").events.iter().find(|e| e.uid == "review@test").unwrap().start.civil(),
        datetime(2026, 10, 14, 14, 0, 0, 0)
    );
    h.key(w, Key::Character("z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-15T15:00"]);
    h.command(w, 'z');

    // Resize Release 0.4 by its bottom edge, from 17:30 to 18:15.
    let rel = h.find(w, "Release 0.4").expect("the release");
    let edge = Point::new(rel.center().x, rel.y1 - 2.0);
    h.drag(w, edge, edge + Vec2::new(0.0, 48.0 * 0.75), 6);
    assert_eq!(ends(&h, "Release 0.4"), ["18:15"]);

    // Drag across Monday's empty afternoon: the editor opens for it.
    let monday_x = grid.x0 + 56.0 + col * 0.5;
    let top = h.find(w, "Design review: Koyomi").unwrap().y0 - 1.0;
    // 14:00 is the review's top.
    let y_of = |hh: f64| top + (hh - 14.0) * 48.0;
    h.drag(w, Point::new(monday_x, y_of(13.0) + 2.0), Point::new(monday_x, y_of(14.5) - 2.0), 5);
    let ed = h.program().editor.as_ref().expect("the editor");
    assert_eq!(ed.times().unwrap(), (datetime(2026, 10, 12, 13, 0, 0, 0), datetime(2026, 10, 12, 14, 30, 0, 0)));
    assert_eq!(h.focused_name(w).as_deref(), Some("edit-title"));
    h.type_text(w, "Lunch with Ana");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().editor.is_none());
    assert_eq!(starts(&h, "Lunch with Ana"), ["2026-10-12T13:00"]);
    // New events go in the default calendar with the default alert.
    let k = h.program();
    let (cal, lunch) = k
        .store
        .calendars
        .iter()
        .flat_map(|c| c.events.iter().map(move |e| (c, e)))
        .find(|(_, e)| e.summary == "Lunch with Ana")
        .unwrap();
    assert_eq!(cal.id, "home");
    assert_eq!(lunch.alarms[0].minutes_before(), Some(10));

    // A double-click on empty time makes an hour.
    let tue = Point::new(grid.x0 + 56.0 + col * 1.5, y_of(16.25));
    h.press(w, tue, PointerButton::Left, 2);
    h.release(w, tue, PointerButton::Left);
    let ed = h.program().editor.as_ref().expect("the editor");
    assert_eq!(ed.times().unwrap(), (datetime(2026, 10, 13, 16, 0, 0, 0), datetime(2026, 10, 13, 17, 0, 0, 0)));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().editor.is_none());
}

#[test]
fn a_series_asks_before_it_changes() {
    let mut h = harness("series", ThemeChoice::Night);
    let w = h.window();
    h.send(Msg::SetView(View::Week));
    // A stand-up block (the sidebar's Up Next has the title too).
    let one = h
        .semantics(w)
        .into_iter()
        .find(|(s, _)| s.label.as_deref().is_some_and(|l| l.starts_with("Stand-up, ")))
        .map(|(_, r)| r)
        .expect("a stand-up");
    let grid = h.find_id(w, "time-grid").unwrap();
    let col = (grid.width() - 56.0) / 7.0;
    let at = |day: f64| Point::new(grid.x0 + 56.0 + col * (day + 0.5), one.center().y);
    // Thursday's, an hour later: only it.
    h.drag(w, at(3.0), at(3.0) + Vec2::new(0.0, 48.0), 4);
    let texts = h.texts(w);
    assert!(
        h.click_text(w, "Only This Event"),
        "an alert asks: {:?} {:?}",
        texts.iter().filter(|t| t.contains("Event") || t.contains("Stand")).collect::<Vec<_>>(),
        h.program().selected
    );
    let s = starts(&h, "Stand-up");
    assert!(s.contains(&"2026-10-15T10:30".to_owned()) && !s.contains(&"2026-10-15T09:30".to_owned()), "{s:?}");
    assert!(s.contains(&"2026-10-16T09:30".to_owned()));
    let work = on_disk(&dir_of("series"), "work");
    assert!(work.events.iter().any(|e| e.uid == "standup@test" && e.recurrence_id.is_some()), "an override is written");
    // Delete Friday's: only it.
    h.click(w, at(4.0));
    h.named(w, NamedKey::Backspace, Modifiers::NONE);
    assert!(h.click_text(w, "Only This Event"));
    let s = starts(&h, "Stand-up");
    assert!(!s.contains(&"2026-10-16T09:30".to_owned()) && s.contains(&"2026-10-14T09:30".to_owned()), "{s:?}");
    // Moving all of them moves the rule, and the exceptions with it (a
    // changed instance keeps its own time).
    h.drag(w, at(2.0), at(2.0) + Vec2::new(0.0, 24.0), 4);
    assert!(h.click_text(w, "All Events"));
    let s = starts(&h, "Stand-up");
    assert!(s.contains(&"2026-10-13T10:00".to_owned()), "{s:?}");
    assert!(s.contains(&"2026-10-15T10:30".to_owned()) && !s.iter().any(|x| x.starts_with("2026-10-16")), "{s:?}");
    h.command(w, 'z');
    assert!(starts(&h, "Stand-up").contains(&"2026-10-13T09:30".to_owned()));
}

#[test]
fn the_month_moves_events_between_days_and_deletes_with_undo() {
    let mut h = harness("month", ThemeChoice::Night);
    let w = h.window();
    let cell = |h: &Harness<Koyomi>, label: &str| -> Rect {
        h.semantics(w)
            .into_iter()
            .find(|(s, _)| s.label.as_deref().is_some_and(|l| l.starts_with(label)))
            .map(|(_, r)| r)
            .unwrap_or_else(|| panic!("{label}"))
    };
    // Wednesday 14: the stand-up (lane 0) and the review (lane 1).
    let wed = cell(&h, "Wednesday 14 October 2026");
    let review = Point::new(wed.x0 + 60.0, wed.y0 + 24.0 + 18.0 + 8.0);
    h.click(w, review);
    assert_eq!(h.program().selected.as_ref().map(|s| s.uid.as_str()), Some("review@test"));
    let fri = cell(&h, "Friday 16 October 2026");
    h.drag(w, review, fri.center(), 8);
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-16T14:00"]);
    // Delete it; undo brings it back.
    h.named(w, NamedKey::Delete, Modifiers::NONE);
    assert!(starts(&h, "Design review: Koyomi").is_empty());
    assert!(!on_disk(&dir_of("month"), "work").events.iter().any(|e| e.uid == "review@test"));
    h.command(w, 'z');
    assert_eq!(starts(&h, "Design review: Koyomi"), ["2026-10-16T14:00"]);
    // A double-click on a day makes an all-day event there.
    let sun = cell(&h, "Sunday 18 October 2026");
    let p = Point::new(sun.center().x, sun.y1 - 10.0);
    h.press(w, p, PointerButton::Left, 2);
    h.release(w, p, PointerButton::Left);
    let ed = h.program().editor.as_ref().expect("the editor");
    assert!(ed.all_day);
    h.type_text(w, "Read");
    h.click_text(w, "Add Event");
    let k = h.program();
    let o = k.occ.iter().find(|o| k.store.calendars[o.cal].events[o.event].summary == "Read").expect("the new event");
    assert!(o.all_day && o.start.date() == date(2026, 10, 18));
    // Clicking another month's day selects it without leaving the month.
    let nov = cell(&h, "Sunday 1 November 2026");
    h.click(w, Point::new(nov.center().x, nov.y1 - 6.0));
    assert_eq!(h.program().cursor, date(2026, 11, 1));
    assert_eq!(h.program().anchor, date(2026, 10, 17));
}

#[test]
fn the_editor_edits_everything() {
    let mut h = harness("editor", ThemeChoice::Night);
    let w = h.window();
    h.send(Msg::SetView(View::Week));
    let review = h.find(w, "Design review: Koyomi").unwrap();
    h.press(w, review.center(), PointerButton::Left, 2);
    h.release(w, review.center(), PointerButton::Left);
    assert!(h.program().editor.is_some());
    use koyomi::ui::editor::Field;
    h.send(Msg::EditField(Field::Title("Design review".into())));
    h.send(Msg::EditField(Field::Location("Room 2".into())));
    h.send(Msg::EditField(Field::StartTime("15:00".into())));
    assert_eq!(h.program().editor.as_ref().unwrap().end_time, "16:30", "the end keeps the length");
    h.send(Msg::EditField(Field::Freq(2)));
    h.send(Msg::EditField(Field::Weekday(jiff::civil::Weekday::Friday)));
    h.send(Msg::EditField(Field::EndKind(1)));
    h.send(Msg::EditField(Field::Count(4.0)));
    h.send(Msg::EditField(Field::Alert(4)));
    h.send(Msg::EditField(Field::Calendar(0)));
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-night@2x.png"), 2.0);
    h.send(Msg::EditSave);
    assert!(h.program().editor.is_none());
    let home = on_disk(&dir_of("editor"), "home");
    let e = home.events.iter().find(|e| e.uid == "review@test").expect("moved to Home");
    assert_eq!(e.summary, "Design review");
    assert_eq!(e.location, "Room 2");
    assert_eq!(e.rrule.as_ref().unwrap().to_ical(), "FREQ=WEEKLY;COUNT=4;BYDAY=WE,FR");
    assert_eq!(e.alarms[0].minutes_before(), Some(15));
    assert!(!on_disk(&dir_of("editor"), "work").events.iter().any(|e| e.uid == "review@test"));
    assert_eq!(starts(&h, "Design review"), ["2026-10-14T15:00", "2026-10-16T15:00"]);
    // An end before the start is refused.
    h.send(Msg::NewEvent);
    h.send(Msg::EditField(Field::EndTime("01:00".into())));
    h.send(Msg::EditSave);
    assert!(h.program().editor.as_ref().is_some_and(|e| e.error.is_some()));
}

#[test]
fn calendars_hide_rename_recolour_and_go() {
    let mut h = harness("calendars", ThemeChoice::Night);
    let w = h.window();
    let all = h.program().occ.len();
    h.click_text(w, "Work");
    assert!(h.program().occ.len() < all);
    assert!(h.program().occ.iter().all(|o| h.program().store.calendars[o.cal].id == "home"));
    h.click_text(w, "Work");
    assert_eq!(h.program().occ.len(), all);
    // A new calendar, through its sheet.
    h.click_text(w, "New Calendar…");
    h.type_text(w, "Climbing Club\n");
    assert!(dir_of("calendars").join("climbing-club.ics").exists());
    assert!(h.texts(w).iter().any(|t| t == "Climbing Club"));
    h.send(Msg::SetColor("climbing-club".into(), koyomi::model::Dye::Green));
    assert_eq!(on_disk(&dir_of("calendars"), "climbing-club").color, koyomi::model::Dye::Green);
    h.send(Msg::RenameCalendar("climbing-club".into()));
    h.key(w, Key::Character("a".into()), Modifiers::SUPER);
    h.type_text(w, "Bouldering\n");
    assert_eq!(on_disk(&dir_of("calendars"), "climbing-club").name, "Bouldering");
    h.send(Msg::DeleteCalendarConfirmed("climbing-club".into()));
    assert!(!dir_of("calendars").join("climbing-club.ics").exists());
    h.command(w, 'z');
    assert!(dir_of("calendars").join("climbing-club.ics").exists());
    // Export writes the calendar where asked.
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("koyomi-export.ics");
    h.send(Msg::ExportTo("work".into(), Some(out.clone())));
    assert_eq!(Calendar::parse("x", &std::fs::read_to_string(&out).unwrap()).0.events.len(), 8);
}

#[test]
fn import_adds_events_to_a_calendar_or_a_new_one() {
    let mut h = harness("import", ThemeChoice::Night);
    let w = h.window();
    let google = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/google.ics");
    // As if opened from Files.
    h.send(Msg::Instance(vec![google.to_string_lossy().into_owned()]));
    assert!(h.program().import.is_some());
    assert!(h.texts(w).iter().any(|t| t.starts_with("7 events")));
    let n = h.program().store.calendars.len();
    h.send(Msg::ImportTarget(n));
    h.click_text(w, "Import");
    assert!(h.program().import.is_none());
    let i = h.program().store.index_of("alex-example").expect("a new calendar named as the file's");
    assert_eq!(h.program().store.calendars[i].events.len(), 7);
    assert!(dir_of("import").join("alex-example.ics").exists());
    // Into an existing one, again: nothing new.
    h.send(Msg::Instance(vec![google.to_string_lossy().into_owned()]));
    h.send(Msg::ImportTarget(i));
    h.send(Msg::ImportDone);
    assert!(h.program().note.as_ref().unwrap().0.contains("0 added"));
    // `--show DATE` from a later launch.
    h.send(Msg::Instance(vec!["--show".into(), "2027-03-14".into()]));
    assert_eq!(h.program().anchor, date(2027, 3, 14));
}

#[test]
fn files_changed_elsewhere_are_read_again() {
    let mut h = harness("watch", ThemeChoice::Night);
    let w = h.window();
    let dir = dir_of("watch");
    std::fs::write(
        dir.join("shared.ics"),
        "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Shared\r\nBEGIN:VEVENT\r\nUID:s1\r\nDTSTART:20261017T170000Z\r\nDTEND:20261017T180000Z\r\nSUMMARY:Shared thing\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    )
    .unwrap();
    h.advance(Duration::from_secs(4));
    assert_eq!(starts(&h, "Shared thing"), ["2026-10-17T18:00"]);
    assert!(h.texts(w).iter().any(|t| t == "Shared"));
}

/// The views, Night and Day, against their goldens (1×), with 2× copies
/// to look at in the target's tmp folder.
#[test]
fn goldens() {
    let golden = |name: &str| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"));
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut h = harness(&format!("golden-{t}"), theme);
        let w = h.window();
        assert_golden(&mut h, w, golden(&format!("month-{t}")), 1.0);
        shot(&mut h, &format!("month-{t}"));
        h.send(Msg::SetView(View::Week));
        let review = h.find(w, "Design review: Koyomi").unwrap();
        h.click(w, review.center());
        assert_golden(&mut h, w, golden(&format!("week-{t}")), 1.0);
        shot(&mut h, &format!("week-{t}"));
        h.send(Msg::GoTo(date(2026, 10, 14)));
        h.send(Msg::SetView(View::Day));
        assert_golden(&mut h, w, golden(&format!("day-{t}")), 1.0);
        shot(&mut h, &format!("day-{t}"));
        h.send(Msg::SetView(View::Year));
        assert_golden(&mut h, w, golden(&format!("year-{t}")), 1.0);
        shot(&mut h, &format!("year-{t}"));
    }
}

#[test]
fn the_all_day_strip_and_the_year_take_the_pointer() {
    let mut h = harness("strip", ThemeChoice::Night);
    let w = h.window();
    h.send(Msg::GoTo(date(2026, 10, 21)));
    h.send(Msg::SetView(View::Week));
    let strip = h.find_id(w, "all-day").expect("the all-day strip");
    let col = (strip.width() - 56.0) / 7.0;
    // The offsite runs Wednesday to Friday; drag it a day on.
    let wed = Point::new(strip.x0 + 56.0 + col * 2.5, strip.y0 + 12.0);
    h.drag(w, wed, wed + Vec2::new(col, 0.0), 4);
    let k = h.program();
    let o = k.occ.iter().find(|o| k.store.calendars[o.cal].events[o.event].summary == "Team offsite, Lisbon").unwrap();
    assert_eq!((o.first_day(), o.last_day()), (date(2026, 10, 22), date(2026, 10, 24)));
    // A double-click on the strip's empty space makes an all-day event.
    let mon = Point::new(strip.x0 + 56.0 + col * 0.5, strip.y0 + 12.0);
    h.press(w, mon, PointerButton::Left, 2);
    h.release(w, mon, PointerButton::Left);
    assert!(h.program().editor.as_ref().is_some_and(|e| e.all_day));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    // The year: a day opens the day view, a month's name the month.
    h.send(Msg::SetView(View::Year));
    h.click_text(w, "Year");
    let year = h.find_id(w, "year").unwrap();
    let _ = year;
    h.send(Msg::Grid(koyomi::ui::week::GridAction::ShowDay(date(2026, 3, 14))));
    assert_eq!((h.program().view, h.program().anchor), (View::Day, date(2026, 3, 14)));
    h.send(Msg::SetView(View::Year));
    h.send(Msg::Grid(koyomi::ui::week::GridAction::ShowMonth(date(2026, 3, 1))));
    assert_eq!(h.program().view, View::Month);
}

#[test]
fn the_sheets_look_finished() {
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut h = harness(&format!("sheets-{t}"), theme);
        let w = h.window();
        h.send(Msg::ShowSettings);
        h.save_png(w, PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("settings-{t}@2x.png")), 2.0);
        h.send(Msg::SettingsDone);
        let google = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/google.ics");
        h.send(Msg::Instance(vec![google.to_string_lossy().into_owned()]));
        h.save_png(w, PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("import-{t}@2x.png")), 2.0);
        h.send(Msg::ImportCancel);
        h.send(Msg::SetView(View::Week));
        let review = h.find(w, "Design review: Koyomi").unwrap();
        h.press(w, review.center(), PointerButton::Left, 2);
        h.release(w, review.center(), PointerButton::Left);
        h.save_png(w, PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-{t}@2x.png")), 2.0);
        assert!(h.program().editor.is_some());
    }
}
