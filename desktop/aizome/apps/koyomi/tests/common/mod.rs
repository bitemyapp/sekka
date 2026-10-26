//! A calendar folder with a believable October 2026 in it, and Koyomi
//! running headlessly over it at a fixed time.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use jiff::civil::datetime;
use koyomi::app::{Koyomi, Options};

pub const WORK: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Aizome//Koyomi tests//EN\r
X-WR-CALNAME:Work\r
X-AIZOME-COLOR:hanada\r
BEGIN:VEVENT\r
UID:standup@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261005T093000\r
DTEND;TZID=Europe/London:20261005T094500\r
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\r
EXDATE;TZID=Europe/London:20261012T093000\r
SUMMARY:Stand-up\r
LOCATION:Room 4.12\r
BEGIN:VALARM\r
ACTION:DISPLAY\r
TRIGGER:-PT10M\r
DESCRIPTION:Stand-up\r
END:VALARM\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:review@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261014T140000\r
DTEND;TZID=Europe/London:20261014T153000\r
SUMMARY:Design review: Koyomi\r
LOCATION:Studio\r
DESCRIPTION:Month, week and day views; drag to move.\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:one-to-one@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261015T110000\r
DTEND;TZID=Europe/London:20261015T113000\r
RRULE:FREQ=WEEKLY;INTERVAL=2\r
SUMMARY:1:1 with Sam\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:planning@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261013T100000\r
DTEND;TZID=Europe/London:20261013T113000\r
SUMMARY:Sprint planning\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:sync@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261013T110000\r
DTEND;TZID=Europe/London:20261013T120000\r
SUMMARY:Infra sync\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:offsite@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;VALUE=DATE:20261021\r
DTEND;VALUE=DATE:20261024\r
SUMMARY:Team offsite, Lisbon\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:release@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261016T163000\r
DTEND;TZID=Europe/London:20261016T173000\r
SUMMARY:Release 0.4\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:retro@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20260130T160000\r
DTEND;TZID=Europe/London:20260130T170000\r
RRULE:FREQ=MONTHLY;BYDAY=-1FR\r
SUMMARY:Retro\r
END:VEVENT\r
END:VCALENDAR\r
";

pub const HOME: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Aizome//Koyomi tests//EN\r
X-WR-CALNAME:Home\r
X-AIZOME-COLOR:kohaku\r
BEGIN:VEVENT\r
UID:climb@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261006T183000\r
DTEND;TZID=Europe/London:20261006T200000\r
RRULE:FREQ=WEEKLY;BYDAY=TU\r
SUMMARY:Climbing\r
LOCATION:The Arch\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:market@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261017T100000\r
DTEND;TZID=Europe/London:20261017T113000\r
SUMMARY:Farmers' market\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:dinner@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261017T193000\r
DTEND;TZID=Europe/London:20261017T220000\r
SUMMARY:Dinner at Hélène's\r
LOCATION:Bermondsey\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:bday@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;VALUE=DATE:19900309\r
DTEND;VALUE=DATE:19900310\r
RRULE:FREQ=YEARLY\r
SUMMARY:Sam's birthday\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:mum@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;VALUE=DATE:20261028\r
DTEND;VALUE=DATE:20261029\r
SUMMARY:Mum's birthday\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:dentist@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261019T083000\r
DTEND;TZID=Europe/London:20261019T090000\r
SUMMARY:Dentist\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:half@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;VALUE=DATE:20261026\r
DTEND;VALUE=DATE:20261031\r
SUMMARY:Half term\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:yoga@test\r
DTSTAMP:20261001T000000Z\r
DTSTART;TZID=Europe/London:20261003T090000\r
DTEND;TZID=Europe/London:20261003T100000\r
RRULE:FREQ=WEEKLY;BYDAY=SA\r
SUMMARY:Yoga\r
END:VEVENT\r
END:VCALENDAR\r
";

/// A fresh folder named `name` with the Work and Home calendars.
pub fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("koyomi-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("work.ics"), WORK).unwrap();
    std::fs::write(dir.join("home.ics"), HOME).unwrap();
    dir
}

/// Koyomi over `dir` at 14:20 on Saturday 17 October 2026, in London.
pub fn koyomi(dir: &Path) -> Koyomi {
    Koyomi::new(Options::test(dir, datetime(2026, 10, 17, 14, 20, 0, 0), "Europe/London"))
}

pub fn harness(name: &str, theme: ThemeChoice) -> Harness<Koyomi> {
    harness_with(name, theme, |_| {})
}

/// The same, with its options changed first.
pub fn harness_with(name: &str, theme: ThemeChoice, f: impl FnOnce(&mut Options)) -> Harness<Koyomi> {
    let dir = folder(name);
    let mut opts = Options::test(&dir, datetime(2026, 10, 17, 14, 20, 0, 0), "Europe/London");
    f(&mut opts);
    let mut h = Harness::with(Koyomi::new(opts), Appearance { theme, ..Appearance::default() }, false);
    let w = h.window();
    h.resize(w, aizome_kit::Size::new(1120.0, 740.0));
    h
}

pub fn shot(h: &mut Harness<Koyomi>, name: &str) {
    let w = h.window();
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}@2x.png")), 2.0);
}
