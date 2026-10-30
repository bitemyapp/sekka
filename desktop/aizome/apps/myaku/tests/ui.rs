//! Myaku headlessly, on made-up readings: the table's grouping, sorting
//! and search, the lenses, the inspector, quitting with a confirmation,
//! and what can't be quit. Signals go to a recorder: no process on this
//! computer is ever sent one.

use std::sync::Arc;

use aizome_kit::layout::Point;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_sys::sysinfo::Signal;
use myaku::app::{Msg, Myaku, Request, RowRef, Sel, Show};
use myaku::fixture;
use myaku::model::{Frame, Kind, Lens, Meter};

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
fn energy_stays_available_when_a_meter_starts_or_stops_reading() {
    let (mut h, _) = start();
    let w = h.window();
    h.command(w, '2');
    assert_eq!(h.program().lens, Lens::Memory);
    let firefox = h.program().frame.groups.iter().find(|g| g.name == "Firefox").unwrap().usage.private.unwrap();
    assert!(h.texts(w).iter().any(|t| *t == myaku::model::bytes(firefox)), "the Private column shows");
    // No meter here: Energy is still available through its shortcut and
    // toolbar, with missing measurements rather than invented zeros.
    h.command(w, '3');
    assert_eq!(h.program().lens, Lens::Energy);
    h.command(w, '2');
    let tab = h
        .inspected(w)
        .into_iter()
        .find_map(|node| node["parts"].as_array()?.iter().find(|part| part["name"] == "Energy").cloned())
        .expect("the Energy tab is visible without a meter");
    let [x, y, width, height] = std::array::from_fn(|i| tab["rect"][i].as_f64().unwrap());
    h.click(w, Point::new(x + width / 2.0, y + height / 2.0));
    assert_eq!(h.program().lens, Lens::Energy);
    assert!(h.texts(w).iter().any(|t| t == "No power meter"));
    assert!(h.texts(w).iter().any(|t| t == "—"));
    assert!(!h.texts(w).iter().any(|t| t.ends_with(" W") || t.ends_with(" mW") || t.ends_with("Wh")));
    h.send(Msg::Inspector(true));
    assert!(
        h.texts(w)
            .iter()
            .any(|t| t.contains("Not measured: the processor's power meter (RAPL) is readable only by root"))
    );
    // Readings appear in the selected lens when the battery discharges.
    let mut f: Frame = fixture::frame(300, 3);
    f.power.meter = Meter::Battery;
    f.power.battery = Some(9.0);
    f.power.on_battery = true;
    for p in &mut f.processes {
        p.usage.power = Some(p.usage.cpu / 100.0);
    }
    h.send(Msg::Frame(Arc::new(f)));
    assert_eq!(h.program().lens, Lens::Energy);
    assert!(h.texts(w).iter().any(|t| t == "Battery 9.0 W"), "the meter's reading shows");
    // Losing the meter must not switch the user's lens back to CPU.
    h.send(Msg::Frame(Arc::new(fixture::frame(300, 4))));
    assert_eq!(h.program().lens, Lens::Energy);
    assert!(h.texts(w).iter().any(|t| t == "No power meter"));
}

#[test]
fn launching_into_energy_without_a_meter_keeps_the_requested_lens() {
    let (h, _) = start_with(Request { lens: Some(Lens::Energy), ..Default::default() });
    assert_eq!(h.program().lens, Lens::Energy);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "No power meter"));
}

#[test]
fn a_desktop_without_root_or_a_battery_measures_energy_by_its_meters() {
    let (mut h, _) = start();
    let w = h.window();
    // RAPL is root's and there's no battery: Aizome's power meter reads
    // RAPL, and the RTX has its own meter. The Radeon's measures only
    // itself.
    let f = fixture::workstation(300, 40);
    assert_eq!(f.power.meter, Meter::Rapl);
    assert!(f.power.cpu.is_some_and(|w| w > 24.0));
    h.send(Msg::Frame(Arc::new(f)));
    h.send(Msg::Lens(Lens::Energy));
    assert_eq!(h.program().lens, Lens::Energy, "the Energy lens is offered");
    assert_eq!(names(&h)[0], "ollama", "the model's runner draws the most, on the RTX");
    assert!(h.texts(w).iter().any(|t| t.starts_with("Processor ") && t.contains(" · Graphics ")), "{:?}", h.texts(w));
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

#[test]
fn the_inspector_explains_group_membership_separately_from_parenthood() {
    let (mut h, _) = start();
    let w = h.window();
    let mut frame = (*h.program().frame).clone();
    let g = frame.groups.iter().position(|g| g.name == "Firefox").unwrap();
    let group = frame.groups[g].clone();
    let parent = frame.processes[group.members[0]].clone();
    let member = group.members[1];
    frame.processes[member].ppid = parent.pid;
    let process = frame.processes[member].key();
    h.send(Msg::Frame(Arc::new(frame.clone())));
    h.send(Msg::Inspector(true));
    h.send(Msg::Select(Sel::Group(group.key)));
    assert!(h.texts(w).iter().any(|t| t == "GROUPING"));
    let explanation = h.find(w, &group.attribution).expect("the full explanation is visible without scrolling");
    let inspector = h.find_id(w, "inspector").unwrap();
    assert!(explanation.x0 >= inspector.x0 && explanation.x1 <= inspector.x1, "the explanation fits the inspector");

    h.send(Msg::Select(Sel::Process(process)));
    assert!(h.find(w, &group.attribution).is_some(), "members show the same explanation");
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Application"));
    assert!(texts.iter().any(|t| t == "Parent"));
    assert!(texts.iter().any(|t| t == &format!("{} · {}", parent.name, parent.pid)));
    let link = h.find_id(w, "inspector-group").expect("a separate group link");
    h.click(w, link.center());
    assert_eq!(h.program().sel, Some(Sel::Group(group.key)));

    let background = frame.groups.iter().find(|g| g.kind == Some(Kind::Service)).unwrap();
    let process = frame.processes[background.members[0]].key();
    h.send(Msg::Select(Sel::Process(process)));
    assert!(h.find(w, &background.attribution).is_some());
    assert!(h.texts(w).iter().any(|t| t == "Group"));
    assert!(!h.texts(w).iter().any(|t| t == "Application"), "background groups are not labelled applications");

    frame.groups[g].attribution.clear();
    h.send(Msg::Frame(Arc::new(frame)));
    h.send(Msg::Select(Sel::Group(group.key)));
    assert!(!h.texts(w).iter().any(|t| t == "GROUPING"), "no empty explanation section");
}
