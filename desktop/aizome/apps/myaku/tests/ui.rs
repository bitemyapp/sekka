//! Myaku headlessly, on made-up readings: the table's grouping, sorting
//! and search, the lenses, the inspector, quitting with a confirmation,
//! and what can't be quit. Signals go to a recorder: no process on this
//! computer is ever sent one.

use std::sync::Arc;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_sys::sysinfo::Signal;
use myaku::app::{Msg, Myaku, Request, RowRef, Sel, Show};
use myaku::fixture;
use myaku::model::{Frame, Lens, Meter};

type Sent = std::rc::Rc<std::cell::RefCell<Vec<(u32, Signal)>>>;

fn start_with(request: Request) -> (Harness<Myaku>, Sent) {
    let (signals, sent) = Myaku::recorder();
    let mut h = Harness::new(Myaku::with_frame(Arc::new(fixture::frame(300, 1)), signals, request));
    h.send(Msg::Frame(Arc::new(fixture::frame(300, 2))));
    (h, sent)
}

fn start() -> (Harness<Myaku>, Sent) {
    start_with(Request::default())
}

fn names(h: &Harness<Myaku>) -> Vec<String> {
    let m = h.program();
    m.rows
        .iter()
        .map(|r| match r {
            RowRef::Group(g) => m.frame.groups[*g].name.clone(),
            RowRef::Process(p, member) => format!("{}{}", if *member { "  " } else { "" }, m.frame.processes[*p].name),
        })
        .collect()
}

fn group_key(h: &Harness<Myaku>, name: &str) -> (u32, u64) {
    h.program().frame.groups.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name}")).key
}

#[test]
fn applications_come_busiest_first_and_open_to_their_processes() {
    let (mut h, _) = start();
    assert_eq!(names(&h)[0], "Firefox");
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Visual Studio Code"));
    // It opens on the application that had the focus.
    assert_eq!(h.program().sel, Some(Sel::Group(group_key(&h, "Sumi"))));
    h.send(Msg::Toggle(group_key(&h, "Firefox")));
    let n = names(&h);
    assert_eq!(&n[1..4], ["  firefox", "  firefox", "  firefox"]);
    assert_eq!(n.iter().filter(|r| r.starts_with("  ")).count(), 14);
    // Sorted by name, ascending: applications first, then services.
    h.send(Msg::Sort(0, true));
    let n = names(&h);
    assert_eq!(n[0], "Firefox");
    assert!(
        n.iter().position(|r| r == "Visual Studio Code").unwrap() < n.iter().position(|r| r == "aizome-shell").unwrap()
    );
}

#[test]
fn searching_finds_applications_and_their_processes() {
    let (mut h, _) = start();
    let w = h.window();
    h.command(w, 'f');
    assert_eq!(h.focused_name(w).as_deref(), Some("search"));
    h.type_text(w, "renderer");
    let n = names(&h);
    assert!(n.contains(&"Firefox".to_owned()) && n.contains(&"  firefox".to_owned()), "{n:?}");
    assert!(!n.contains(&"Sumi".to_owned()));
    h.send(Msg::Search("zzz".into()));
    assert!(h.program().rows.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "Nothing matches “zzz”."));
}

#[test]
fn lenses_change_the_columns_and_energy_needs_a_meter() {
    let (mut h, _) = start();
    let w = h.window();
    h.command(w, '2');
    assert_eq!(h.program().lens, Lens::Memory);
    let firefox = h.program().frame.groups.iter().find(|g| g.name == "Firefox").unwrap().usage.private.unwrap();
    assert!(h.texts(w).iter().any(|t| *t == myaku::model::bytes(firefox)), "the Private column shows");
    // No meter here: the Energy lens isn't offered, and says so.
    h.send(Msg::Lens(Lens::Energy));
    assert_eq!(h.program().lens, Lens::Memory);
    assert!(!h.texts(w).iter().any(|t| t == "Energy"));
    h.send(Msg::Inspector(true));
    assert!(
        h.texts(w)
            .iter()
            .any(|t| t.contains("Not measured: the processor's power meter (RAPL) is readable only by root"))
    );
    // With the battery discharging, it is.
    let mut f: Frame = fixture::frame(300, 3);
    f.power.meter = Meter::Battery;
    f.power.battery = Some(9.0);
    f.power.on_battery = true;
    for p in &mut f.processes {
        p.usage.power = Some(p.usage.cpu / 100.0);
    }
    h.send(Msg::Frame(Arc::new(f)));
    h.send(Msg::Lens(Lens::Energy));
    assert_eq!(h.program().lens, Lens::Energy);
    let average = h.program().frame.groups[0].usage.average_power();
    assert!(
        average.is_none() || h.texts(w).iter().any(|t| t.ends_with(" W") || t.ends_with(" mW")),
        "power figures show"
    );
}

#[test]
fn quitting_asks_first_and_signals_every_process_of_an_application() {
    let (mut h, sent) = start();
    let w = h.window();
    h.send(Msg::Select(Sel::Group(group_key(&h, "Signal"))));
    h.send(Msg::AskQuit(false));
    assert!(sent.borrow().is_empty(), "nothing until it's confirmed");
    assert!(h.texts(w).iter().any(|t| t == "Quit “Signal”?"), "{:?}", h.texts(w));
    // Return presses the alert's default button, Quit.
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    let got = sent.borrow().clone();
    assert_eq!(got.len(), 6);
    assert!(got.iter().all(|(_, s)| *s == Signal::Terminate));
    let pids: Vec<u32> = got.iter().map(|(p, _)| *p).collect();
    assert!(pids.windows(2).all(|w| w[0] > w[1]), "children first: {pids:?}");
    sent.borrow_mut().clear();
    h.send(Msg::AskQuit(true));
    assert!(h.click_text(w, "Force Quit"));
    assert!(sent.borrow().iter().all(|(_, s)| *s == Signal::Kill));
    // Cancel sends nothing.
    sent.borrow_mut().clear();
    h.send(Msg::AskQuit(false));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(sent.borrow().is_empty());
}

#[test]
fn the_kernel_and_other_users_are_not_quit_from_here() {
    let (mut h, sent) = start();
    h.send(Msg::Select(Sel::Group(group_key(&h, "Kernel"))));
    assert!(h.program().can_quit().unwrap_err().contains("kernel"));
    h.send(Msg::AskQuit(true));
    assert!(sent.borrow().is_empty());
    let root = h.program().frame.processes.iter().find(|p| p.uid == 0 && !p.kernel).unwrap().key();
    h.send(Msg::Select(Sel::Process(root)));
    assert!(h.program().can_quit().unwrap_err().contains("another user"));
    h.send(Msg::Pause(true));
    assert!(sent.borrow().is_empty());
    assert!(h.program().notice.as_ref().is_some_and(|(n, e)| *e && n.contains("another user")));
}

#[test]
fn pausing_and_resuming() {
    let (mut h, sent) = start();
    h.send(Msg::Select(Sel::Group(group_key(&h, "Kura"))));
    h.send(Msg::Pause(true));
    h.send(Msg::Pause(false));
    let s: Vec<Signal> = sent.borrow().iter().map(|(_, s)| *s).collect();
    assert_eq!(s, [Signal::Stop, Signal::Continue]);
}

#[test]
fn a_launch_can_name_a_process_lens_and_search() {
    let target = fixture::frame(300, 1).processes.iter().find(|p| p.role == "GPU process").unwrap().clone();
    let (h, _) = start_with(Request { pid: Some(target.pid), lens: Some(Lens::Gpu), search: None });
    let m = h.program();
    assert_eq!(m.sel, Some(Sel::Process(target.key())));
    assert!(m.inspector);
    assert_eq!(m.lens, Lens::Gpu);
    assert!(names(&h).contains(&"  firefox".to_owned()), "its application is open");
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Video memory"));
}

#[test]
fn processes_can_be_listed_flat() {
    let (mut h, _) = start();
    h.send(Msg::Show(Show::Mine));
    let m = h.program();
    assert!(m.rows.iter().all(|r| matches!(r, RowRef::Process(p, false) if m.frame.processes[*p].uid == 1000)));
    h.send(Msg::Show(Show::All));
    assert_eq!(h.program().rows.len(), h.program().frame.processes.len());
}
