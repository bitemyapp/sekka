//! The GPU lens with two cards, on made-up readings of a desktop whose
//! RTX 5090 does most of the work beside an integrated Radeon: which card
//! the table shows, chosen from the toolbar, the strip or the menu; what
//! the figures mean across cards; and what can't be read staying unknown.

use std::sync::Arc;

use aizome_kit::Size;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use myaku::app::{Msg, Myaku, Request, RowRef, Sel, Show};
use myaku::fixture;
use myaku::model::{Frame, GpuUsage, Lens};
use myaku::ui::columns::Col;

const RADEON: &str = "0000:78:00.0";
const NVIDIA: &str = "0000:01:00.0";

fn start_with(frame: Frame) -> Harness<Myaku> {
    let (signals, _) = Myaku::recorder();
    Harness::new(Myaku::with_frame(Arc::new(frame), signals, Request { lens: Some(Lens::Gpu), ..Default::default() }))
}

fn start() -> Harness<Myaku> {
    start_with(fixture::workstation(300, 40))
}

/// Choose the toolbar pop-up's `index`th item: All GPUs, then the cards,
/// discrete first.
fn choose(h: &mut Harness<Myaku>, index: usize) {
    let w = h.window();
    let rect = h.find_id(w, "gpu-selector").expect("GPU pop-up");
    h.click(w, rect.center());
    let popup = h.popups(w).last().expect("its menu").0;
    h.popup_keyboard(popup, Key::Named(NamedKey::Home), Modifiers::NONE);
    for _ in 0..index {
        h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::NONE);
    }
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::NONE);
}

fn group(h: &Harness<Myaku>, name: &str) -> usize {
    h.program().frame.groups.iter().position(|g| g.name == name).unwrap_or_else(|| panic!("{name}"))
}

fn names(h: &Harness<Myaku>) -> Vec<String> {
    let m = h.program();
    m.rows
        .iter()
        .map(|r| match r {
            RowRef::Group(g) => m.frame.groups[*g].name.clone(),
            RowRef::Process(p, _) => m.frame.processes[*p].name.clone(),
        })
        .collect()
}

#[test]
fn every_card_shows_with_the_busiest_first_and_which_card_each_uses() {
    let h = start();
    let w = h.window();
    assert_eq!(names(&h)[0], "ollama", "the model's runner keeps the RTX 5090 busiest");
    let cols = h.program().columns();
    assert!(cols.contains(&Col::Card) && cols.contains(&Col::GpuShared));
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Both"), "Firefox draws on both cards");
    assert!(texts.iter().any(|t| t == "GeForce RTX 5090"));
    // The strip: the processor's and memory's graphs, then the cards'.
    let strip = ["gpu-card-1", "gpu-card-0"].map(|id| h.find_id(w, id).expect(id));
    assert!(strip[0].x1 <= strip[1].x0, "the discrete card first");
    assert!(!texts.iter().any(|t| t == "Network"), "the cards take Disk's and Network's places");
    // The shell keeps 1% of each card busy: shares of different cards
    // don't add up, so it's 1%. Their memory does; shared memory is the
    // Radeon's alone, as NVML reports none.
    let shell = h.program().frame.groups[group(&h, "aizome-shell")].usage.graphics;
    assert_eq!((shell.busy, shell.vram, shell.shared), (Some(1.0), Some(102 << 20), Some(4 << 20)));
}

#[test]
fn a_card_chosen_shows_its_clients_alone() {
    let mut h = start();
    let w = h.window();
    choose(&mut h, 1);
    assert_eq!(h.program().gpu.as_deref(), Some(NVIDIA));
    let cols = h.program().columns();
    assert!(!cols.contains(&Col::Card), "every row uses the card shown");
    assert!(!cols.contains(&Col::GpuShared), "NVML reports no shared memory");
    let shown = names(&h);
    assert_eq!(shown[0], "ollama");
    assert!(!shown.contains(&"Spotify".to_owned()), "Spotify draws on the Radeon");
    // Each application's figures are its use of that card.
    let shell = &h.program().frame.groups[group(&h, "aizome-shell")];
    assert_eq!(shell.usage.graphics.vram, Some(96 << 20));
    let status = h.texts(w);
    assert!(
        status.iter().any(|t| t.starts_with("5 of ") && t.ends_with("processes use GeForce RTX 5090")),
        "{status:?}"
    );
    assert!(status.iter().any(|t| t.starts_with("NVIDIA GeForce RTX 5090 · 2.61 GHz")));
    // An application on both shows its processes using this one.
    h.send(Msg::Toggle(h.program().frame.groups[group(&h, "Firefox")].key));
    assert!(h.texts(w).iter().any(|t| t == "1 of 14 processes"));
    // The Radeon: Spotify and the compositor's helper.
    choose(&mut h, 2);
    assert_eq!(h.program().gpu.as_deref(), Some(RADEON));
    assert!(h.program().columns().contains(&Col::GpuShared));
    assert!(names(&h).contains(&"Spotify".to_owned()) && !names(&h).contains(&"ollama".to_owned()));
    choose(&mut h, 0);
    assert_eq!(h.program().gpu, None);
    // Searching and listing processes keep to the card.
    choose(&mut h, 1);
    h.send(Msg::Show(Show::All));
    assert!(
        h.program()
            .rows
            .iter()
            .all(|r| matches!(r, RowRef::Process(p, false) if h.program().frame.processes[*p].uses(1)))
    );
    h.send(Msg::Search("spotify".into()));
    assert!(h.program().rows.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "Nothing matches “spotify”."));
}

#[test]
fn the_strip_and_the_menu_choose_too() {
    let mut h = start();
    let w = h.window();
    let card = h.find_id(w, "gpu-card-1").unwrap();
    h.move_to(w, card.center());
    assert_eq!(h.program().hover_gpu.as_deref(), Some(NVIDIA));
    h.click(w, card.center());
    assert_eq!(h.program().gpu.as_deref(), Some(NVIDIA));
    // Pressed again, every card shows.
    h.click(w, card.center());
    assert_eq!(h.program().gpu, None);
    // View ▸ GPUs, from another lens: it opens the GPU lens on the card.
    h.send(Msg::Lens(Lens::Memory));
    let items = view_menu(&h);
    let at = |label: &str| items.iter().position(|(l, _)| l == label).unwrap_or_else(|| panic!("{label}: {items:?}"));
    assert!(
        items[at("All GPUs")].1
            && at("All GPUs") < at("GeForce RTX 5090")
            && at("GeForce RTX 5090") < at("Radeon Graphics")
    );
    h.send(Msg::Gpu(Some(RADEON.into())));
    assert_eq!((h.program().lens, h.program().gpu.as_deref()), (Lens::Gpu, Some(RADEON)));
    assert!(view_menu(&h).contains(&("Radeon Graphics".into(), true)));
}

/// The View menu's items, submenus' too, and whether each is checked.
fn view_menu(h: &Harness<Myaku>) -> Vec<(String, bool)> {
    use aizome_kit::Program;
    let bar = h.program().menus();
    let view = bar.menus().iter().find(|m| m.title == "View").expect("View");
    view.actions().iter().map(|a| (a.label.clone(), a.checked)).collect()
}

#[test]
fn other_lenses_show_every_cards_use() {
    let mut h = start();
    choose(&mut h, 1);
    let shell = group(&h, "aizome-shell");
    assert_eq!(h.program().frame.groups[shell].usage.graphics.vram, Some(96 << 20));
    h.send(Msg::Lens(Lens::Cpu));
    assert_eq!(
        h.program().frame.groups[shell].usage.graphics.vram,
        Some(102 << 20),
        "the CPU lens's figures are as read"
    );
    h.send(Msg::Select(Sel::Group(h.program().frame.groups[group(&h, "Firefox")].key)));
    h.send(Msg::Inspector(true));
    // The inspector gives its use of each card, whichever the table shows.
    let texts = h.texts(h.window());
    assert!(texts.iter().any(|t| t == "GeForce RTX 5090") && texts.iter().any(|t| t == "Radeon Graphics"), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("0.0% · 12.0 KiB")));
    h.send(Msg::Lens(Lens::Gpu));
    assert_eq!(h.program().gpu.as_deref(), Some(NVIDIA), "the GPU lens comes back to the card chosen");
    assert_eq!(h.program().frame.groups[shell].usage.graphics.vram, Some(96 << 20));
}

#[test]
fn the_choice_follows_the_card_and_goes_with_it() {
    let mut h = start();
    choose(&mut h, 1);
    let mut f = fixture::workstation(300, 41);
    f.gpus.swap(0, 1);
    for p in &mut f.processes {
        for g in &mut p.gpus {
            g.device = 1 - g.device;
        }
    }
    f.total_graphics();
    h.send(Msg::Frame(Arc::new(f)));
    assert_eq!(h.program().gpu.as_deref(), Some(NVIDIA));
    assert_eq!(names(&h)[0], "ollama");
    assert_eq!(h.program().history.card(NVIDIA).unwrap().busy.len(), 2, "its graph goes on");
    let mut gone = fixture::workstation(300, 42);
    gone.gpus.pop();
    for p in &mut gone.processes {
        p.gpus.retain(|g| g.device == 0);
    }
    gone.total_graphics();
    h.send(Msg::Frame(Arc::new(gone)));
    assert_eq!(h.program().gpu, None);
    let w = h.window();
    assert!(h.find_id(w, "gpu-selector").is_none());
    assert!(!h.program().columns().contains(&Col::Card));
}

#[test]
fn what_cant_be_read_stays_unknown() {
    // NVML couldn't be opened: the NVIDIA card lists no clients, and that
    // says nothing about who uses it.
    let mut f = fixture::workstation(300, 40);
    f.gpus[1].unlisted_usage = GpuUsage::UNKNOWN;
    f.gpus[1].busy = None;
    for p in &mut f.processes {
        p.gpus.retain(|g| g.device != 1);
    }
    f.total_graphics();
    let mut h = start_with(f);
    let w = h.window();
    let spotify = &h.program().frame.groups[group(&h, "Spotify")];
    assert_eq!(spotify.usage.graphics.busy, None);
    assert_eq!(spotify.usage.graphics.shared, Some(18 << 20), "the Radeon's shared memory is known");
    assert!(h.texts(w).iter().any(|t| t == "Busy —"));
    choose(&mut h, 1);
    assert!(h.program().rows.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "The processes using GeForce RTX 5090 can't be read here."));
    choose(&mut h, 2);
    assert!(h.program().frame.groups[group(&h, "Spotify")].usage.graphics.busy.is_some());
}

#[test]
fn one_card_or_many() {
    let mut one = fixture::workstation(300, 40);
    one.gpus.truncate(1);
    for p in &mut one.processes {
        p.gpus.retain(|g| g.device == 0);
    }
    one.total_graphics();
    let mut h = start_with(one);
    let w = h.window();
    assert!(h.find_id(w, "gpu-selector").is_none(), "nothing to choose");
    assert!(!h.program().columns().contains(&Col::Card));
    let card = h.find_id(w, "gpu-card-0").expect("its graph");
    assert!(card.x0 > 1080.0 * 0.7, "one card takes Network's place alone: {card:?}");
    h.click(w, card.center());
    assert_eq!(h.program().gpu, None, "it doesn't choose");

    let mut many = fixture::workstation(300, 40);
    for bus in ["0000:41:00.0", "0000:61:00.0", "0000:81:00.0"] {
        let mut more = many.gpus[1].clone();
        more.pci = bus.into();
        many.gpus.push(more);
    }
    let mut h = start_with(many);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "GeForce RTX 5090 · 41:00"), "alike cards are told apart by bus");
    let fifth = h.find_id(w, "gpu-card-0").expect("five cards fill the strip and scroll");
    assert!(fifth.x0 > h.find_id(w, "gpu-card-4").unwrap().x1, "the integrated card last");
    choose(&mut h, 4);
    assert_eq!(h.program().gpu.as_deref(), Some("0000:81:00.0"));
    assert!(h.texts(w).iter().any(|t| t == "Nothing is using GeForce RTX 5090 · 81:00."));
}

#[test]
fn the_toolbar_fits_at_the_smallest_size() {
    let mut h = start();
    let w = h.window();
    h.resize(w, Size::new(760.0, 480.0));
    let selector = h.find_id(w, "gpu-selector").unwrap();
    let search = h.find_id(w, "search").unwrap();
    assert!(selector.x1 < search.x0 && search.x1 <= 760.0, "{selector:?} {search:?}");
    // Beside all five lenses and the selector, the search field gives a
    // little room.
    assert!(search.width() >= 110.0, "the search field gives a little room: {search:?}");
    let strip = ["gpu-card-1", "gpu-card-0"].map(|id| h.find_id(w, id).unwrap());
    assert!(strip[1].x1 <= 760.0);
}

#[test]
#[ignore = "requires an NVIDIA GPU, driver and nvidia-smi"]
fn live_nvidia_memory_matches_nvidia_smi() {
    let mut sampler = myaku::sampler::Sampler::new();
    let _ = sampler.sample();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let f = sampler.sample();
    let gpu = f.gpus.iter().find(|g| g.driver == "nvidia").expect("NVIDIA GPU");
    assert!(gpu.vram_used.is_some_and(|n| n > 0));
    let output = std::process::Command::new("nvidia-smi")
        .args(["-i", &gpu.pci, "--query-gpu=memory.used", "--format=csv,noheader,nounits"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let mib: u64 = String::from_utf8(output.stdout).unwrap().trim().parse().unwrap();
    eprintln!("{}: Myaku {} MiB, nvidia-smi {mib} MiB", gpu.name, gpu.vram_used.unwrap() >> 20);
    assert!(gpu.vram_used.unwrap().abs_diff(mib << 20) < 128 << 20, "allow allocations between samples");
    let output = std::process::Command::new("nvidia-smi")
        .args(["-i", &gpu.pci, "--query-compute-apps=pid,used_gpu_memory", "--format=csv,noheader,nounits"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut matched = 0;
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let Some((pid, memory)) = line.split_once(',') else { continue };
        let (Ok(pid), Ok(mib)) = (pid.trim().parse::<u32>(), memory.trim().parse::<u64>()) else { continue };
        let Some(p) = f.processes.iter().find(|p| p.pid == pid) else { continue };
        let device = f.gpus.iter().position(|g| g.pci == gpu.pci).unwrap();
        let usage = p.gpus.iter().find(|g| g.device == device).expect("NVIDIA process measurement");
        let bytes = usage.usage.vram.expect("process VRAM");
        eprintln!("{} ({pid}): Myaku {} MiB, nvidia-smi {mib} MiB", p.name, bytes >> 20);
        assert!(bytes.abs_diff(mib << 20) < 32 << 20);
        matched += 1;
    }
    assert!(matched > 0, "at least one compute process must be running");
}

#[test]
#[ignore = "requires an AMD card with idle clients, as an AMD and NVIDIA desktop has"]
fn live_idle_amd_clients_are_idle_not_unknown() {
    let mut sampler = myaku::sampler::Sampler::new();
    let _ = sampler.sample();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let f = sampler.sample();
    let amd = f.gpus.iter().position(|g| g.driver == "amdgpu").expect("an AMD card");
    let clients: Vec<_> = f.processes.iter().filter_map(|p| p.gpus.iter().find(|g| g.device == amd)).collect();
    assert!(!clients.is_empty(), "the desktop has clients on it");
    assert!(clients.iter().all(|g| g.usage.busy.is_some()), "amdgpu leaves out idle engines: idle, not unknown");
}
