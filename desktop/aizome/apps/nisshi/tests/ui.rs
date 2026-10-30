//! Nisshi headlessly, on a made-up boot: the places, search and its keys,
//! completion, the suggestions and what they do, the timeline, a message's
//! context, its fields as filters, copying, and the keyboard.

use std::time::Duration;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use nisshi::app::{Access, Brush, Msg, Nisshi};
use nisshi::entry::{Priority, Transport};
use nisshi::filter::{Chip, Scope};
use nisshi::fixture;
use nisshi::suggest::Action;

fn start() -> Harness<Nisshi> {
    Harness::new(Nisshi::sample(
        fixture::journal(),
        fixture::boots(),
        fixture::files(),
        fixture::clock(),
        fixture::UID,
        Access::System,
    ))
}

fn shown(h: &Harness<Nisshi>) -> Vec<String> {
    h.program().view.shown.iter().map(|e| e.message.to_string()).collect()
}

fn seq_of(h: &Harness<Nisshi>, needle: &str) -> u64 {
    h.program()
        .store
        .entries
        .iter()
        .rev()
        .find(|e| e.message.contains(needle))
        .unwrap_or_else(|| panic!("{needle}"))
        .seq
}

/// Select the newest message with `needle`, and wait the moment its
/// topics take.
fn select(h: &mut Harness<Nisshi>, needle: &str) {
    let seq = seq_of(h, needle);
    h.send(Msg::Select(seq));
    h.advance(Duration::from_millis(50));
}

/// The suggestion offered with this label.
fn suggestion(h: &Harness<Nisshi>, label: &str) -> usize {
    let s = &h.program().suggestions;
    s.iter()
        .position(|s| s.label == label)
        .unwrap_or_else(|| panic!("{label} in {:?}", s.iter().map(|s| &s.label).collect::<Vec<_>>()))
}

#[test]
fn the_places() {
    let mut h = start();
    let total = h.program().store.entries.len();
    assert_eq!(h.program().view.rows.len(), total);
    h.send(Msg::Scope(Scope::Problems));
    let n = h.program();
    assert!(n.view.shown.iter().all(|e| e.priority <= Priority::Warning));
    assert_eq!(n.view.rows.len(), n.view.scope_count(&Scope::Problems).unwrap());
    h.send(Msg::Scope(Scope::Kernel));
    assert!(h.program().view.shown.iter().all(|e| e.transport == Transport::Kernel));
    h.send(Msg::Scope(Scope::Crashes));
    let crashes = shown(&h);
    assert_eq!(crashes.len(), 2, "{crashes:?}");
    assert!(crashes[0].starts_with("traps: gamescope-wl"));
    assert!(crashes[1].starts_with("Process 298773 (gamescope-wl) of user 1000 dumped core."));
    h.send(Msg::Scope(Scope::Mine));
    assert!(h.program().view.shown.iter().all(|e| e.uid == Some(fixture::UID)));
    h.send(Msg::Scope(Scope::Security));
    let sources: std::collections::HashSet<String> =
        h.program().view.shown.iter().map(|e| e.source.to_string()).collect();
    assert!(sources.contains("sshd") && sources.contains("sudo") && sources.contains("systemd-logind"), "{sources:?}");
    // The sidebar's sources count with the place.
    let s = &h.program().view.sources;
    assert_eq!(s[0].name.as_ref(), "sshd");
}

#[test]
fn search_and_its_keys() {
    let mut h = start();
    h.send(Msg::Search("kex".into()));
    assert_eq!(shown(&h).len(), 34);
    // Words in any order, any case; a capital makes one exact.
    h.send(Msg::Search("closed REMOTE".into()));
    assert_eq!(shown(&h).len(), 0);
    h.send(Msg::Search("closed remote".into()));
    assert_eq!(shown(&h).len(), 34);
    h.send(Msg::Search("app:sshd -kex".into()));
    let rows = shown(&h);
    assert!(rows.iter().all(|m| !m.contains("kex")) && rows.len() == 35, "{}", rows.len());
    h.send(Msg::Search("level:err app:sudo".into()));
    assert_eq!(
        shown(&h),
        [
            "alice : 3 incorrect password attempts ; TTY=pts/1 ; PWD=/home/alice ; USER=root ; COMMAND=/run/current-system/sw/bin/true"
        ]
    );
    h.send(Msg::Search(r"/203\.0\.113\.(7|8)\b/ invalid".into()));
    assert_eq!(shown(&h).len(), 14);
    h.send(Msg::Search("unit:fwupd-refresh".into()));
    assert_eq!(shown(&h).len(), 4, "the unit's own and systemd's about it");
    h.send(Msg::Search("since:15:00 until:15:30 app:sudo".into()));
    assert_eq!(shown(&h).len(), 0);
    h.send(Msg::Search("since:1h app:sudo".into()));
    assert_eq!(shown(&h).len(), 0, "the administrator was 70 minutes ago");
    h.send(Msg::Search("since:2h app:sudo".into()));
    assert_eq!(shown(&h).len(), 4);
    // What's typed and highlighted is the search's.
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.contains("4 of")), "{:?}", h.texts(w));
}

#[test]
fn completing_a_key() {
    let mut h = start();
    h.send(Msg::Search("error app:net".into()));
    let s = &h.program().suggestions;
    assert_eq!(s[0].label, "NetworkManager");
    assert_eq!(s[0].action, Action::Search("error app:NetworkManager ".into()));
    // Return takes the first.
    h.send(Msg::SubmitSearch);
    assert_eq!(h.program().search_text, "error app:NetworkManager ");
    assert!(h.program().view.shown.iter().all(|e| e.source.as_ref() == "NetworkManager"));
    h.send(Msg::Search("unit:fw".into()));
    assert_eq!(h.program().suggestions[0].label, "fwupd-refresh.service");
    h.send(Msg::Search("level:w".into()));
    assert_eq!(h.program().suggestions[0].label, "Warning and worse");
}

#[test]
fn what_stands_out() {
    let h = start();
    let labels: Vec<String> = h.program().suggestions.iter().map(|s| s.label.clone()).collect();
    for want in ["Errors", "sshd's errors", "fwupd-refresh.service failed", "gamescope-wl crashed"] {
        assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
    }
    assert!(labels.iter().any(|l| l.starts_with("Errors at 14:3")), "the burst: {labels:?}");
}

#[test]
fn suggestions_for_a_message_and_what_they_do() {
    let mut h = start();
    let total = h.program().view.rows.len();
    // Telegram's warning, over and over.
    select(&mut h, "QWaylandTextInputv3");
    let i = suggestion(&h, "Hide messages like this");
    let alike = h.program().suggestions[i].count.unwrap();
    assert!(alike > 50);
    h.send(Msg::Suggest(i));
    assert_eq!(h.program().view.rows.len(), total - alike);
    assert!(shown(&h).iter().all(|m| !m.contains("QWaylandTextInputv3")));
    assert!(matches!(h.program().filter.chips[..], [Chip::Shape { not: true, .. }]));
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.starts_with("Hiding “virtual void")), "{:?}", h.texts(w));
    h.send(Msg::RemoveChip(0));
    assert_eq!(h.program().view.rows.len(), total);
    // A program Nisshi knows: its topics, this message's own first.
    select(&mut h, "dhcp4 (wlp14s0): state changed new lease");
    let labels: Vec<String> = h.program().suggestions.iter().map(|s| s.label.clone()).collect();
    assert_eq!(labels[0], "Only NetworkManager");
    assert!(labels.iter().any(|l| l == "DHCP"), "{labels:?}");
    // Every one of its messages here names wlp14s0: a topic that finds
    // them all narrows nothing, and isn't offered.
    assert!(!labels.iter().any(|l| l == "Wi-Fi"), "{labels:?}");
    let i = suggestion(&h, "DHCP");
    h.send(Msg::Suggest(i));
    let rows = shown(&h);
    assert!(rows.iter().all(|m| m.contains("dhcp4")), "{rows:?}");
    assert!(h.program().filter.source().is_some_and(|s| s.as_ref() == "NetworkManager"));
    // Around this time: everything, a minute either side.
    let seq = h.program().selected[0];
    let i = suggestion(&h, "Around this time");
    h.send(Msg::Suggest(i));
    let n = h.program();
    assert_eq!(n.selected, [seq]);
    assert!(n.filter.source().is_none());
    assert!(matches!(n.filter.chips[..], [Chip::Range { .. }]));
    let t = n.selection().unwrap().time;
    assert!(n.view.shown.iter().all(|e| (e.time - t).abs() <= 60_000_000));
}

#[test]
fn a_program_nisshi_doesnt_know_by_its_words() {
    let mut h = start();
    select(&mut h, "frame budget: p50");
    let labels: Vec<String> = h.program().suggestions.iter().map(|s| s.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "Drawing"), "Aizome's own topics: {labels:?}");
    select(&mut h, "accepted connection from pid");
    let labels: Vec<String> = h.program().suggestions.iter().map(|s| s.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "Builds"), "{labels:?}");
}

#[test]
fn the_timeline() {
    let mut h = start();
    h.send(Msg::Brush(Brush::Down(0.70)));
    h.send(Msg::Brush(Brush::Move(0.75)));
    assert_eq!(h.program().brush, Some((0.70, 0.75)));
    h.send(Msg::Brush(Brush::Up(0.80)));
    let n = h.program();
    let (from, to) = n.filter.range().unwrap();
    assert!(n.view.shown.iter().all(|e| e.time >= from && e.time <= to));
    assert!(!n.view.shown.is_empty() && n.view.rows.len() < n.store.entries.len());
    // The histogram still shows all of the boot.
    assert_eq!(
        n.view.histogram.buckets.iter().map(|b| b[0] + b[1] + b[2]).sum::<u32>() as usize,
        n.store.entries.len()
    );
    h.send(Msg::ClearFilters);
    // A press goes to the message nearest that moment.
    h.send(Msg::Brush(Brush::Down(0.0)));
    h.send(Msg::Brush(Brush::Up(0.0)));
    let first = h.program().store.entries[0].seq;
    assert_eq!(h.program().selected, [first]);
}

#[test]
fn fields_become_filters() {
    let mut h = start();
    let seq = seq_of(&h, "Failed with result 'exit-code'");
    h.send(Msg::Select(seq));
    h.advance(Duration::from_millis(200));
    let fields = h.program().fields.clone().expect("its fields").1;
    assert!(fields.iter().any(|(k, v)| k == "MESSAGE_ID" && v == nisshi::filter::UNIT_FAILED));
    h.send(Msg::FilterField("UNIT".into(), "fwupd-refresh.service".into()));
    assert_eq!(shown(&h).len(), 4);
    h.send(Msg::ClearFilters);
    h.send(Msg::FilterField("_PID".into(), "1301".into()));
    assert!(h.program().view.shown.iter().all(|e| e.pid == 1301));
    h.send(Msg::FilterField("PRIORITY".into(), "3".into()));
    assert_eq!(h.program().filter.level, Some(Priority::Error));
    // Another field is matched by the journal: it goes into search.
    h.send(Msg::ClearFilters);
    h.send(Msg::FilterField("_HOSTNAME".into(), "runnymede".into()));
    assert_eq!(h.program().search_text, "_HOSTNAME=runnymede");
    assert_eq!(h.program().filter.search.fields, [("_HOSTNAME".to_owned(), "runnymede".to_owned())]);
}

#[test]
fn copying_and_exporting() {
    let mut h = start();
    h.send(Msg::Search("app:sudo".into()));
    h.send(Msg::SelectRows(vec![0, 1]));
    h.send(Msg::Copy);
    let copied = h.platform.clipboard.clone().unwrap();
    let lines: Vec<&str> = copied.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("14:55:00.000 sudo[6754]: alice : TTY=pts/1"), "{copied}");
    let text = h.program().export_text(false);
    assert_eq!(text.lines().count(), 4);
    let json = h.program().export_text(true);
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["source"], "sudo");
}

#[test]
fn the_keyboard() {
    let mut h = start();
    let w = h.window();
    h.command(w, 'f');
    assert_eq!(h.focused_name(w).as_deref(), Some("search"));
    h.type_text(w, "kex");
    assert_eq!(h.program().view.rows.len(), 34);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().view.rows.len(), h.program().store.entries.len());
    // ❖2 for problems, ❖1 for everything.
    h.command(w, '2');
    assert_eq!(h.program().filter.scope, Scope::Problems);
    h.command(w, '1');
    assert_eq!(h.program().filter.scope, Scope::All);
    // ❖I hides the details.
    h.command(w, 'i');
    assert!(!h.program().details);
}

#[test]
fn newest_at_the_top() {
    let mut h = start();
    let newest = h.program().store.entries.last().unwrap().seq;
    h.send(Msg::NewestFirst(true));
    h.send(Msg::SelectRows(vec![0]));
    assert_eq!(h.program().selected, [newest]);
    assert_eq!(h.program().selected_rows(), [0]);
}

#[test]
fn a_log_file() {
    let mut h = start();
    h.program_mut().sample_file("/var/log/Xorg.0.log", fixture::xorg());
    h.advance(Duration::from_millis(50));
    let rows = shown(&h);
    assert_eq!(rows.len(), 7);
    let n = h.program();
    assert_eq!(n.view.shown[4].priority, Priority::Error, "(EE)");
    assert_eq!(n.view.shown[5].priority, Priority::Warning, "(WW)");
    h.send(Msg::Level(Some(Priority::Warning)));
    assert_eq!(shown(&h).len(), 2);
}

#[test]
fn launches_hand_over_what_to_show() {
    use nisshi::app::Request;
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let r = Request::parse(&args(&["--search", "kex -fail", "--level", "warn", "--scope", "crashes"]));
    assert_eq!(r.search.as_deref(), Some("kex -fail"));
    assert_eq!(r.level, Some(Priority::Warning));
    assert_eq!(r.scope, Some(Scope::Crashes));
    // An empty value doesn't survive the hand-over: the next flag isn't it.
    let r = Request::parse(&args(&["--search", "--scope", "kernel"]));
    assert_eq!((r.search, r.scope), (None, Some(Scope::Kernel)));
    // Opening on one message, by its cursor.
    let mut h = start();
    let e = h.program().store.entries.iter().find(|e| e.message.contains("dumped core")).unwrap().clone();
    let cursor = e.cursor.text().unwrap();
    h.send(Msg::Launched(args(&["--cursor", &cursor])));
    assert_eq!(h.program().selected, [e.seq]);
    assert!(!h.program().following);
    h.send(Msg::Launched(args(&["--cursor", "s=0123;i=1"])));
    assert!(h.program().notice.as_ref().is_some_and(|(n, _)| n.starts_with("That message isn't among those read")));
}

#[test]
fn terminal_colours_and_the_level_written() {
    let mut h = start();
    let e = h.program().store.entries.iter().find(|e| e.message.contains("no EDID")).unwrap().clone();
    // Its escapes are styles now, not text; its WARN a warning.
    assert!(!e.message.contains('\u{1b}'));
    assert!(e.styles.is_some());
    assert_eq!(e.priority, Priority::Warning);
    h.send(Msg::Scope(Scope::Problems));
    assert!(shown(&h).iter().any(|m| m.contains("no EDID")));
    // Search and copying see the plain text.
    h.send(Msg::Scope(Scope::All));
    h.send(Msg::Search("\"WARN aizome_comp_logic\"".into()));
    assert_eq!(shown(&h).len(), 1);
    h.send(Msg::Select(e.seq));
    h.send(Msg::CopyMessage);
    assert_eq!(
        h.platform.clipboard.as_deref(),
        Some("2026-10-29T21:04:45.902517Z  WARN aizome_comp_logic::outputs: HDMI-A-1: no EDID, assuming 60 Hz")
    );
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Warning (as written in the message)"), "{:?}", h.texts(w));
}

#[test]
fn an_apps_launches() {
    let mut h = start();
    // Each launch's output under its own identifier, all of it Koe's.
    let sources: Vec<String> = h.program().view.sources.iter().map(|s| s.name.to_string()).collect();
    assert!(sources.iter().any(|s| s == "Koe"), "{sources:?}");
    assert!(!sources.iter().any(|s| s.starts_with("aizome-launch")), "{sources:?}");
    select(&mut h, "no such source: alsa_input");
    let e = h.program().selection().unwrap().clone();
    assert_eq!(e.priority, Priority::Error, "its ERROR, though journald had Info");
    let i = suggestion(&h, "This launch");
    assert_eq!(h.program().suggestions[i].count, Some(3));
    h.send(Msg::Suggest(i));
    assert_eq!(
        shown(&h),
        [
            "koe: loading ggml-base.en (142 MB)",
            "ERROR: no such source: alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo",
            "koe: daemon did not become ready"
        ]
    );
    // Opened on one launch, as the shell can ask.
    let first = "aizome-launch-0a1b2c3d4e5f60718293a4b5c6d7e8f9";
    h.send(Msg::Launched(vec!["--launch".into(), first.into()]));
    assert_eq!(shown(&h).len(), 3);
    assert!(shown(&h)[2].starts_with("koe: listening on"));
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "One launch (0a1b2c3d)"), "{:?}", h.texts(w));
    // Not an identifier a launch has: ignored.
    h.send(Msg::Launched(vec!["--launch".into(), "--rotate".into()]));
    assert!(h.program().filter.chips.is_empty());
}
