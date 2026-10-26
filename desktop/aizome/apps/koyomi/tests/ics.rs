//! Real producers' exports: Google Calendar, Apple Calendar, Outlook
//! (Exchange), and a file of quirks. Each is read, its occurrences in
//! October 2026 checked where a person would see them, its alarms timed,
//! and it's written back and read again unchanged.

use std::path::Path;

use jiff::civil::{DateTime, datetime};
use jiff::tz::TimeZone;
use koyomi::model::{Calendar, Dye, Trigger, When};
use koyomi::occur::{self, Index, Occurrence};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).expect("fixture")
}

fn october(cal: &Calendar, zone: &str) -> Vec<(String, String)> {
    let tz = TimeZone::get(zone).unwrap();
    let cals = vec![cal.clone()];
    let ix = Index::new(&cals, &tz);
    let v: Vec<Occurrence> = occur::occurrences(
        &cals,
        &ix,
        &[true],
        &tz,
        datetime(2026, 10, 1, 0, 0, 0, 0),
        datetime(2026, 11, 1, 0, 0, 0, 0),
    );
    v.iter().map(|o| (o.start.to_string(), cals[0].events[o.event].summary.clone())).collect()
}

fn round_trips(cal: &Calendar) {
    let text = cal.to_ics();
    for line in text.split("\r\n") {
        assert!(line.len() <= 75, "a line over 75 octets: {line:?}");
    }
    let (again, dropped) = Calendar::parse(&cal.id, &text);
    assert_eq!(dropped, 0);
    assert_eq!(again.name, cal.name);
    assert_eq!(again.color, cal.color);
    assert_eq!(again.events, cal.events, "events change in a round trip");
    assert_eq!(again.other, cal.other);
    // And a second write is the same text.
    assert_eq!(again.to_ics(), text);
}

fn at(o: &[(String, String)], start: DateTime) -> Option<&str> {
    o.iter().find(|(s, _)| *s == start.to_string()).map(|(_, t)| t.as_str())
}

#[test]
fn google_calendar_export() {
    let (cal, dropped) = Calendar::parse("google", &fixture("google.ics"));
    assert_eq!(dropped, 0);
    assert_eq!(cal.name, "Alex Example");
    assert_eq!(cal.events.len(), 7);
    let stand = &cal.events[0];
    assert_eq!(stand.description.lines().next(), Some("Daily stand-up."));
    assert!(stand.description.contains("https://support.google.com/a/users/answer/9282720"));
    assert_eq!(stand.extra.iter().filter(|p| p.name == "ATTENDEE").count(), 2);
    assert_eq!(stand.extra.iter().find(|p| p.name == "ATTENDEE").unwrap().param("CN"), Some("Alex Example"));
    assert_eq!(stand.alarms[0].minutes_before(), Some(10));

    let o = october(&cal, "Europe/London");
    let standups: Vec<&(String, String)> = o.iter().filter(|(_, t)| t.starts_with("Stand-up")).collect();
    // 22 weekdays in October 2026, less the 12th (an exception) and the
    // 16th (cancelled); the 14th moved to 11:00.
    assert_eq!(standups.len(), 20);
    assert_eq!(at(&o, datetime(2026, 10, 14, 11, 0, 0, 0)), Some("Stand-up (moved)"));
    assert_eq!(at(&o, datetime(2026, 10, 14, 9, 30, 0, 0)), None);
    assert_eq!(at(&o, datetime(2026, 10, 12, 9, 30, 0, 0)), None);
    assert_eq!(at(&o, datetime(2026, 10, 16, 9, 30, 0, 0)), None);
    // After the clocks go back (25 October) it's still 09:30 in London.
    assert_eq!(at(&o, datetime(2026, 10, 26, 9, 30, 0, 0)), Some("Stand-up"));
    assert_eq!(at(&o, datetime(2026, 10, 30, 16, 0, 0, 0)), Some("Retro"));
    assert_eq!(at(&o, datetime(2026, 10, 20, 18, 0, 0, 0)), Some("Climbing"));
    assert_eq!(at(&o, datetime(2026, 10, 23, 0, 0, 0, 0)), Some("Lisbon trip"));
    assert!(!o.iter().any(|(_, t)| t == "Sam's birthday"));
    // Seen from Tokyo the stand-up is in the evening.
    let tokyo = october(&cal, "Asia/Tokyo");
    assert_eq!(at(&tokyo, datetime(2026, 10, 1, 17, 30, 0, 0)), Some("Stand-up"));
    // The birthday comes every 14 March.
    let tz = TimeZone::get("Europe/London").unwrap();
    let cals = vec![cal.clone()];
    let ix = Index::new(&cals, &tz);
    let march = occur::occurrences(
        &cals,
        &ix,
        &[true],
        &tz,
        datetime(2027, 3, 1, 0, 0, 0, 0),
        datetime(2027, 4, 1, 0, 0, 0, 0),
    );
    assert!(march.iter().any(|m| m.all_day && m.start == datetime(2027, 3, 14, 0, 0, 0, 0)));
    // Climbing's display alarm an hour before; the e-mail one is kept.
    let climbing = cal.events.iter().find(|e| e.summary == "Climbing").unwrap();
    assert_eq!(climbing.alarms.len(), 2);
    assert_eq!(climbing.alarms[0].action, "EMAIL");
    round_trips(&cal);
}

#[test]
fn apple_calendar_export() {
    let (cal, dropped) = Calendar::parse("apple", &fixture("apple.ics"));
    assert_eq!(dropped, 0);
    assert_eq!(cal.name, "Home");
    assert_eq!(cal.color, Dye::Cyan);
    let run = &cal.events[0];
    assert_eq!(run.location, "Golden Gate Park\nSan Francisco, CA, United States");
    let structured = run.extra.iter().find(|p| p.name == "X-APPLE-STRUCTURED-LOCATION").unwrap();
    assert_eq!(structured.param("X-TITLE"), Some("Golden Gate Park"));
    assert_eq!(structured.value, "geo:37.769421,-122.486214");
    // Apple's "no alarm" placeholder isn't an alarm.
    assert!(run.alarms[0].is_active());
    assert!(!run.alarms[1].is_active());
    assert!(matches!(run.alarms[1].trigger, Trigger::Absolute(_)));

    let o = october(&cal, "America/Los_Angeles");
    assert_eq!(at(&o, datetime(2026, 10, 19, 18, 0, 0, 0)), Some("Run club"));
    assert_eq!(at(&o, datetime(2026, 10, 24, 19, 30, 0, 0)), Some("Dîner chez Hélène 🍷"));
    assert_eq!(at(&o, datetime(2026, 10, 31, 0, 0, 0, 0)), Some("Halloween party"));
    // Floating: 08:00 wherever one is, ten days from the 22nd.
    let vit: Vec<_> = o.iter().filter(|(_, t)| t == "Take vitamins").collect();
    assert_eq!(vit.len(), 10);
    let berlin = october(&cal, "Europe/Berlin");
    assert_eq!(at(&berlin, datetime(2026, 10, 22, 8, 0, 0, 0)), Some("Take vitamins"));
    // Every other Monday until the end of the year: 19 Oct, 2, 16, 30 Nov, 14, 28 Dec.
    let tz = TimeZone::get("America/Los_Angeles").unwrap();
    let cals = vec![cal.clone()];
    let ix = Index::new(&cals, &tz);
    let rest = occur::occurrences(
        &cals,
        &ix,
        &[true],
        &tz,
        datetime(2026, 10, 1, 0, 0, 0, 0),
        datetime(2027, 6, 1, 0, 0, 0, 0),
    );
    assert_eq!(rest.iter().filter(|r| cals[0].events[r.event].summary == "Run club").count(), 6);
    // The all-day alarm goes off at 09:00 the day before (wall clock).
    let alarms = occur::alarms(
        &cals,
        &ix,
        &tz,
        datetime(2026, 10, 30, 0, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp(),
        datetime(2026, 10, 31, 0, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp(),
    );
    assert_eq!(alarms.len(), 1);
    assert_eq!(alarms[0].0.to_zoned(tz.clone()).datetime(), datetime(2026, 10, 30, 9, 0, 0, 0));
    round_trips(&cal);
}

#[test]
fn outlook_export() {
    let (cal, dropped) = Calendar::parse("outlook", &fixture("outlook.ics"));
    assert_eq!(dropped, 0);
    let budget = &cal.events[0];
    // A Windows zone name resolves to its IANA zone.
    match &budget.start {
        When::Zoned(_, tz) => assert_eq!(&*tz.id, "Europe/Berlin"),
        other => panic!("{other:?}"),
    }
    assert!(
        budget.uid.starts_with("040000008200E00074C5B7101A82E008")
            && budget.uid.ends_with("0B0F67DD1D1F1A54FAE8A3C4A7B0F0E2A")
    );
    let o = october(&cal, "Europe/Berlin");
    assert_eq!(at(&o, datetime(2026, 10, 13, 10, 0, 0, 0)), Some("Budget review"));
    // A "Customized Time Zone" with the US rules: 09:00 EDT is 15:00 in Berlin.
    assert_eq!(at(&o, datetime(2026, 10, 21, 15, 0, 0, 0)), Some("NY sync"));
    // The second Tuesday of each month, to June 2027.
    let tz = TimeZone::get("Europe/Berlin").unwrap();
    let cals = vec![cal.clone()];
    let ix = Index::new(&cals, &tz);
    let all = occur::occurrences(
        &cals,
        &ix,
        &[true],
        &tz,
        datetime(2026, 10, 1, 0, 0, 0, 0),
        datetime(2028, 1, 1, 0, 0, 0, 0),
    );
    let budgets: Vec<String> = all.iter().filter(|r| r.event == 0).map(|r| r.start.date().to_string()).collect();
    assert_eq!(budgets.first().map(String::as_str), Some("2026-10-13"));
    assert_eq!(budgets[1], "2026-11-10");
    assert_eq!(budgets.last().map(String::as_str), Some("2027-05-11"));
    assert_eq!(budgets.len(), 8);
    assert!(all.iter().any(|r| r.all_day && r.start.date().to_string() == "2026-11-02"));
    round_trips(&cal);
}

#[test]
fn quirks_are_read_as_meant() {
    let (cal, dropped) = Calendar::parse("quirks", &fixture("quirks.ics"));
    assert_eq!(dropped, 1, "the event without a start");
    assert_eq!(cal.name, "Odds & Ends");
    assert_eq!(cal.events[0].summary, "Mozilla-style zone continued with a tab");
    assert_eq!(cal.other.len(), 1, "the to-do is kept");
    let o = october(&cal, "Europe/Berlin");
    let moz: Vec<&str> = o.iter().filter(|(_, t)| t.starts_with("Mozilla")).map(|(s, _)| s.as_str()).collect();
    assert_eq!(moz, ["2026-10-19T08:00:00", "2026-10-22T08:00:00", "2026-10-23T08:00:00"]);
    assert_eq!(at(&o, datetime(2026, 10, 19, 0, 0, 0, 0)), Some("A date without VALUE=DATE"));
    assert_eq!(at(&o, datetime(2026, 10, 19, 12, 0, 0, 0)), Some("Unknown zone: floating"));
    let rd: Vec<&str> = o.iter().filter(|(_, t)| t == "With RDATEs").map(|(s, _)| s.as_str()).collect();
    assert_eq!(rd, ["2026-10-19T02:00:00", "2026-10-26T01:00:00"]);
    // An end before the start is no length at all.
    assert!(at(&o, datetime(2026, 10, 19, 12, 0, 0, 0)).is_some());
    let neg = cal.events.iter().find(|e| e.uid == "q6").unwrap();
    assert_eq!(neg.duration(&TimeZone::UTC).total_seconds(), 0);
    round_trips(&cal);
}
