//! Tansu headlessly, on the sample disks: choosing drives and partitions,
//! queueing operations through their sheets, the Apply sheet's typed
//! confirmation for built-in disks, the run and how a failure stops it,
//! and the operations done at once. Nothing touches a real device: the
//! backend is the in-memory mock.

use std::rc::Rc;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use tansu::app::{Msg, SheetKind, SheetMsg, Tansu};
use tansu::layout::{Mark, Target};
use tansu::mock::Mock;
use tansu::plan::{Action, OpKind};

const GB: f64 = 1_000_000_000.0;

#[test]
fn invalid_create_size_cannot_queue_the_previous_size() {
    let (mut h, mock) = start();
    drive(&mut h, "Crucial");
    largest_free(&mut h);
    h.send(Msg::Do(Action::New));
    let w = h.window();
    for text in ["1.02.1", "", "lots", "0", "-1", "999999999999999999999999999999999999999"] {
        sheet(&mut h, SheetMsg::SizeText(text.into()));
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert_eq!(sheet_kind(&h), Some("create"), "invalid size {text:?} submitted by Return");
        sheet(&mut h, SheetMsg::Submit);
        assert!(h.program().plan.is_empty(), "invalid size {text:?} queued a partition");
        assert!(h.texts(w).iter().any(|t| t.starts_with("Enter a size")));
    }
    assert!(mock.calls().is_empty());
    sheet(&mut h, SheetMsg::SizeText("5 GB".into()));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(sheet_kind(&h), None);
    assert_eq!(h.program().plan.len(), 1);
    assert!(mock.calls().is_empty(), "only queued");
}

#[test]
fn invalid_resize_size_cannot_queue_the_last_valid_prefix() {
    let (mut h, mock) = start();
    drive(&mut h, "Crucial");
    seg(&mut h, "sda3");
    h.send(Msg::Do(Action::Unmount));
    h.send(Msg::Do(Action::Resize));
    sheet(&mut h, SheetMsg::Unit(1));
    sheet(&mut h, SheetMsg::SizeText("120".into()));
    for text in ["120x", "", "0", "999999999999999999999999999999999999999"] {
        sheet(&mut h, SheetMsg::SizeText(text.into()));
        let w = h.window();
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert_eq!(sheet_kind(&h), Some("resize"), "invalid size {text:?} submitted by Return");
        sheet(&mut h, SheetMsg::Submit);
        assert_eq!(sheet_kind(&h), Some("resize"), "invalid size {text:?} submitted");
        assert!(h.program().plan.is_empty());
    }
    assert_eq!(mock.calls(), ["Unmount /dev/sda3"]);
    sheet(&mut h, SheetMsg::SizeText("120".into()));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(sheet_kind(&h), None);
    let OpKind::Resize { from, to } = h.program().plan.ops[0].kind else { panic!() };
    assert!(to < from);
}

#[test]
fn size_slider_and_unit_selection_recover_invalid_text_without_rounding_the_size() {
    let (mut h, _) = start();
    drive(&mut h, "Crucial");
    largest_free(&mut h);
    h.send(Msg::Do(Action::New));
    sheet(&mut h, SheetMsg::Size(4777.0));
    let exact = match &h.program().sheet.as_ref().unwrap().kind {
        SheetKind::Create(f) => f.size,
        _ => panic!(),
    };
    sheet(&mut h, SheetMsg::SizeText("invalid".into()));
    sheet(&mut h, SheetMsg::Unit(0));
    sheet(&mut h, SheetMsg::Submit);
    let OpKind::Create(ref p) = h.program().plan.ops[0].kind else { panic!() };
    assert_eq!(p.size, exact, "rounded display text must not change the chosen bytes");

    h.send(Msg::Undo);
    largest_free(&mut h);
    h.send(Msg::Do(Action::New));
    sheet(&mut h, SheetMsg::SizeText("invalid".into()));
    sheet(&mut h, SheetMsg::Size(4777.0));
    sheet(&mut h, SheetMsg::Submit);
    let OpKind::Create(ref p) = h.program().plan.ops[0].kind else { panic!() };
    assert_eq!(p.size, exact, "moving the slider replaces invalid typed text");
}

fn start() -> (Harness<Tansu>, Rc<Mock>) {
    let mock = Rc::new(Mock::default());
    (Harness::new(Tansu::new(mock.clone())), mock)
}

fn drive(h: &mut Harness<Tansu>, name: &str) {
    let key = h.program().drives.iter().find(|d| d.name.contains(name)).unwrap_or_else(|| panic!("{name}")).key.clone();
    h.send(Msg::SelectDrive(key));
}

fn seg(h: &mut Harness<Tansu>, short: &str) {
    let i = h.program().layout.segs.iter().position(|s| s.short_name() == short).unwrap_or_else(|| panic!("{short}"));
    h.send(Msg::SelectSeg(i));
}

fn largest_free(h: &mut Harness<Tansu>) {
    let l = &h.program().layout;
    let i = (0..l.segs.len()).filter(|&i| l.segs[i].is_free()).max_by_key(|&i| l.segs[i].size).expect("free space");
    h.send(Msg::SelectSeg(i));
}

fn sheet(h: &mut Harness<Tansu>, m: SheetMsg) {
    h.send(Msg::Sheet(m));
}

fn sheet_kind(h: &Harness<Tansu>) -> Option<&'static str> {
    h.program().sheet.as_ref().map(|s| match s.kind {
        SheetKind::Format(_) => "format",
        SheetKind::Create(_) => "create",
        SheetKind::Resize(_) => "resize",
        SheetKind::Label { .. } => "label",
        SheetKind::Flags { .. } => "flags",
        SheetKind::Table { .. } => "table",
        SheetKind::Unlock { .. } => "unlock",
        SheetKind::Attach { .. } => "attach",
        SheetKind::Apply(_) => "apply",
        SheetKind::Running => "running",
        SheetKind::Smart { .. } => "smart",
    })
}

#[test]
fn show_picks_the_drive_holding_a_device() {
    // As the desktop's disk icon asks: by a partition's /dev node, through
    // its encryption, by a drive, and an image by its loop device.
    let at = |what: &str| {
        let mock = Rc::new(Mock::default());
        let h = Harness::new(Tansu::new(mock).with_show(Some(what.into())));
        let p = h.program();
        p.drives.iter().find(|d| Some(&d.key) == p.drive.as_ref()).map(|d| d.name.clone()).unwrap_or_default()
    };
    assert_eq!(at("/dev/nvme0n1p2"), "Samsung SSD 990 PRO 1TB");
    assert_eq!(at("/org/freedesktop/UDisks2/block_devices/dm_2d0"), "Samsung SSD 990 PRO 1TB");
    assert_eq!(at("/org/freedesktop/UDisks2/block_devices/loop0"), "sekka-nightly.iso");
    let (h, _) = start();
    let sd = h.program().drives.iter().find(|d| d.name == "SD SC64G").unwrap().key.clone();
    assert_eq!(at(&sd), "SD SC64G");
    // Nothing it knows: the first drive, as without --show.
    assert_eq!(at("/dev/nothing"), "Samsung SSD 990 PRO 1TB");
}

#[test]
fn the_sidebar_lists_drives_then_images() {
    let (h, _) = start();
    let names: Vec<&str> = h.program().drives.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Samsung SSD 990 PRO 1TB",
            "Crucial CT500MX500SSD1",
            "WDC WD40EFRX-68N32N0",
            "SanDisk Cruzer Blade",
            "SD SC64G",
            "sekka-nightly.iso"
        ]
    );
    let w = h.window();
    let texts = h.texts(w);
    for want in ["Samsung SSD 990 PRO 1TB", "nvme0n1p1", "nvme0n1p2", "dm-0", "Unallocated", "/boot"] {
        assert!(texts.iter().any(|t| t.contains(want)), "{want} in {texts:?}");
    }
    // The first partition is selected, so the toolbar has something to act on.
    assert_eq!(h.program().selected(), Some(0));
}

#[test]
fn formatting_a_usb_stick_runs_without_typing_its_name() {
    let (mut h, mock) = start();
    let w = h.window();
    drive(&mut h, "Cruzer");
    seg(&mut h, "sdc1");
    h.send(Msg::Do(Action::Format));
    assert_eq!(sheet_kind(&h), Some("format"));
    assert_eq!(h.focused_name(w).as_deref(), Some("format-label"));
    sheet(&mut h, SheetMsg::Fs(0)); // ext4
    sheet(&mut h, SheetMsg::Label("PHOTOS".into()));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(sheet_kind(&h), None);
    assert_eq!(h.program().plan.len(), 1);
    assert!(mock.calls().is_empty(), "queued, not run");
    // The preview shows it.
    let i = h.program().selected().unwrap();
    assert_eq!(h.program().layout.segs[i].fs, "ext4");
    assert_eq!(h.program().layout.segs[i].pending, [Mark::Format]);
    assert!(h.texts(w).iter().any(|t| t == "pending operation"));
    h.send(Msg::Apply);
    assert_eq!(sheet_kind(&h), Some("apply"));
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t.contains("Format /dev/sdc1 as Ext4 labelled “PHOTOS”")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Everything on /dev/sdc1 (FAT “STICK”) will be erased.")));
    assert!(!texts.iter().any(|t| t.contains("type its name")), "a USB stick needs no typing");
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(mock.calls(), ["Format(ext4) on /dev/sdc1"]);
    assert_eq!(sheet_kind(&h), Some("running"));
    assert!(h.program().plan.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "Every operation ran."));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(sheet_kind(&h), None);
    let state = h.program().state.clone();
    let b = state.block_by_device("/dev/sdc1").unwrap();
    assert_eq!((b.id_type.as_str(), b.id_label.as_str()), ("ext4", "PHOTOS"));
}

#[test]
fn a_built_in_disk_needs_its_name_typed() {
    let (mut h, mock) = start();
    let w = h.window();
    drive(&mut h, "Crucial");
    seg(&mut h, "sda2");
    h.send(Msg::Do(Action::Delete));
    assert_eq!(h.program().plan.len(), 1);
    h.send(Msg::Apply);
    let typed = match &h.program().sheet.as_ref().unwrap().kind {
        SheetKind::Apply(f) => f.typed.clone(),
        _ => panic!("the apply sheet"),
    };
    assert_eq!(typed, [("sda".to_owned(), String::new())]);
    assert_eq!(h.focused_name(w).as_deref(), Some("type-0"));
    // Return without the name does nothing.
    sheet(&mut h, SheetMsg::Submit);
    assert!(mock.calls().is_empty());
    assert!(h.program().sheet.as_ref().unwrap().error.as_deref().is_some_and(|e| e.contains("Type each disk's name")));
    h.type_text(w, "sdb");
    sheet(&mut h, SheetMsg::Submit);
    assert!(mock.calls().is_empty(), "the wrong disk's name");
    sheet(&mut h, SheetMsg::Typed(0, String::new()));
    h.type_text(w, "sda");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(mock.calls(), ["Delete /dev/sda2"]);
}

#[test]
fn a_new_partition_is_made_in_free_space_with_its_flags() {
    let (mut h, mock) = start();
    drive(&mut h, "Crucial");
    largest_free(&mut h);
    h.send(Msg::Do(Action::New));
    assert_eq!(sheet_kind(&h), Some("create"));
    sheet(&mut h, SheetMsg::SizeText("5".into())); // GB
    sheet(&mut h, SheetMsg::Fs(3)); // FAT
    sheet(&mut h, SheetMsg::Label("EFI".into()));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(sheet_kind(&h), None);
    let Some(tansu::app::Sel::Target(Target::New(id))) = h.program().sel.clone() else {
        panic!("the new partition is selected")
    };
    let i = h.program().layout.find(&Target::New(id)).unwrap();
    let size = h.program().layout.segs[i].size;
    assert!((size as f64 - 5.0 * GB).abs() < 2.0 * 1_048_576.0, "{size}");
    // Flag it as an ESP: the create operation takes it.
    h.send(Msg::Do(Action::Flags));
    sheet(&mut h, SheetMsg::Flag(1, true));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(h.program().plan.len(), 1);
    h.send(Msg::Apply);
    h.type_text(h.window(), "sda");
    sheet(&mut h, SheetMsg::Submit);
    let calls = mock.calls();
    assert!(calls[0].starts_with("CreatePartition(") && calls[0].ends_with("vfat) on /dev/sda"), "{calls:?}");
    let state = h.program().state.clone();
    let made = state.block_by_device("/dev/sda4").expect("sda4");
    let p = made.partition.as_ref().unwrap();
    assert_eq!(p.type_id, "c12a7328-f81f-11d2-ba4b-00a0c93ec93b", "made as an ESP");
    assert_eq!(made.id_label, "EFI");
}

#[test]
fn shrinking_checks_then_shrinks_the_filesystem_then_the_partition() {
    let (mut h, mock) = start();
    drive(&mut h, "Crucial");
    seg(&mut h, "sda3");
    // Mounted: Resize says why it can't.
    let why = h.program().can(Action::Resize).unwrap_err();
    assert!(why.contains("mounted at /mnt/games"), "{why}");
    h.send(Msg::Do(Action::Unmount));
    assert_eq!(mock.calls(), ["Unmount /dev/sda3"]);
    h.send(Msg::Do(Action::Resize));
    assert_eq!(sheet_kind(&h), Some("resize"));
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.contains("its start stays where it is")));
    sheet(&mut h, SheetMsg::Unit(1));
    sheet(&mut h, SheetMsg::SizeText("120".into()));
    sheet(&mut h, SheetMsg::Submit);
    let OpKind::Resize { from, to } = h.program().plan.ops[0].kind else { panic!() };
    assert!(to < from);
    h.send(Msg::Apply);
    h.type_text(w, "sda");
    sheet(&mut h, SheetMsg::Submit);
    let calls = mock.calls();
    assert_eq!(calls[1], "Repair /dev/sda3");
    assert_eq!(calls[2], format!("ResizeFilesystem({}) /dev/sda3", to - (1 << 20)));
    assert_eq!(calls[3], format!("ResizePartition({to}) /dev/sda3"));
    assert_eq!(calls[4], "ResizeFilesystem(0) /dev/sda3", "then it fills the partition");
}

#[test]
fn a_failure_stops_the_run_and_keeps_the_rest_pending() {
    let (mut h, mock) = start();
    drive(&mut h, "SD SC64G");
    seg(&mut h, "mmcblk0p1");
    h.send(Msg::Do(Action::Label));
    sheet(&mut h, SheetMsg::Label("BOOT".into()));
    sheet(&mut h, SheetMsg::Submit);
    seg(&mut h, "mmcblk0p2");
    h.send(Msg::Do(Action::Format));
    sheet(&mut h, SheetMsg::Fs(1)); // btrfs
    sheet(&mut h, SheetMsg::Submit);
    seg(&mut h, "mmcblk0p1");
    h.send(Msg::Do(Action::Flags));
    sheet(&mut h, SheetMsg::Flag(0, false));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(h.program().plan.len(), 3);
    mock.fail_next("Format", "mkfs.btrfs: device is busy");
    h.send(Msg::Apply);
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(mock.calls(), ["SetLabel(BOOT) /dev/mmcblk0p1", "Format(btrfs) on /dev/mmcblk0p2"]);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.contains("mkfs.btrfs: device is busy")));
    sheet(&mut h, SheetMsg::Submit);
    let left: Vec<&str> = h.program().plan.ops.iter().map(|o| tansu::plan::verb(&o.kind)).collect();
    assert_eq!(left, ["Format", "Flags on"], "the label ran; the rest waits");
    assert!(h.program().notice.as_ref().is_some_and(|(n, e)| *e && n.contains("busy")));
}

#[test]
fn undo_and_clear_change_only_the_plan() {
    let (mut h, mock) = start();
    drive(&mut h, "Cruzer");
    seg(&mut h, "sdc1");
    h.send(Msg::Do(Action::Delete));
    largest_free(&mut h);
    h.send(Msg::Do(Action::New));
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(h.program().plan.len(), 2);
    let w = h.window();
    h.command(w, 'z');
    assert_eq!(h.program().plan.len(), 1, "❖Z undoes the last");
    h.send(Msg::ClearPlan);
    assert!(h.program().plan.is_empty());
    h.send(Msg::Undo);
    assert_eq!(h.program().plan.len(), 1, "and clearing can be undone");
    assert!(mock.calls().is_empty());
}

#[test]
fn a_new_partition_table_on_a_stick() {
    let (mut h, mock) = start();
    drive(&mut h, "Cruzer");
    // Something is mounted on it: refused until it isn't.
    assert!(h.program().can_new_table().is_err());
    seg(&mut h, "sdc2");
    h.send(Msg::Do(Action::Unmount));
    h.send(Msg::NewTable);
    assert_eq!(sheet_kind(&h), Some("table"));
    sheet(&mut h, SheetMsg::Table(aizome_sys::disks::TableKind::Gpt));
    sheet(&mut h, SheetMsg::Submit);
    assert!(h.program().layout.new_table);
    assert!(h.program().layout.segs.iter().all(|s| s.is_free()));
    h.send(Msg::Apply);
    sheet(&mut h, SheetMsg::Submit);
    assert_eq!(mock.calls(), ["Unmount /dev/sdc2", "Format(gpt) on /dev/sdc"]);
    assert_eq!(h.program().layout.table, "gpt");
}

#[test]
fn unlocking_mounting_and_images_happen_at_once() {
    let (mut h, mock) = start();
    drive(&mut h, "WDC");
    seg(&mut h, "sdb2");
    h.send(Msg::Do(Action::Unlock));
    assert_eq!(sheet_kind(&h), Some("unlock"));
    sheet(&mut h, SheetMsg::Pass("wrong".into()));
    sheet(&mut h, SheetMsg::Submit);
    assert!(
        h.program().notice.as_ref().is_some_and(|(n, e)| *e && n.contains("wrong passphrase")),
        "{:?}",
        h.program().notice
    );
    h.send(Msg::Do(Action::Unlock));
    sheet(&mut h, SheetMsg::Pass("correct horse".into()));
    sheet(&mut h, SheetMsg::Submit);
    let i = h.program().selected().unwrap();
    assert!(h.program().layout.segs[i].is_unlocked());
    h.send(Msg::Do(Action::Mount));
    assert!(h.program().layout.segs[i].shown_mount().is_some());
    h.send(Msg::Do(Action::Lock));
    assert!(h.program().layout.segs[i].is_locked());
    assert_eq!(mock.calls(), ["Unlock /dev/sdb2", "Unlock /dev/sdb2", "Mount /dev/dm-1", "Lock /dev/sdb2"]);
    // An image, attached read-only.
    let file = std::env::temp_dir().join(format!("tansu-test-{}.iso", std::process::id()));
    std::fs::write(&file, vec![0u8; 8192]).unwrap();
    h.send(Msg::AttachImage);
    sheet(&mut h, SheetMsg::Path(file.display().to_string()));
    sheet(&mut h, SheetMsg::Submit);
    std::fs::remove_file(&file).unwrap();
    assert!(mock.calls().last().unwrap().ends_with("read-only=true"));
    assert!(h.program().drives.iter().any(|d| d.name.starts_with("tansu-test-")));
}

#[test]
fn smart_shows_attributes_and_starts_self_tests() {
    let (mut h, mock) = start();
    drive(&mut h, "WDC");
    h.send(Msg::ShowSmart);
    assert_eq!(sheet_kind(&h), Some("smart"));
    let w = h.window();
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Reallocated sector count"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("8 bad sectors")));
    h.send(Msg::Sheet(SheetMsg::SelfTest(aizome_sys::disks::SelfTest::Short)));
    assert!(mock.calls().iter().any(|c| c.starts_with("SmartSelftestStart(Short)")));
    assert!(h.texts(w).iter().any(|t| t.contains("A self-test is running: 90% to go")));
}

#[test]
fn the_keyboard_reaches_the_map() {
    let (mut h, _) = start();
    let w = h.window();
    let r = h.find_id(w, "partition-map").expect("the map");
    h.click(w, aizome_kit::Point::new(r.x0 + r.width() * 0.5, r.center().y));
    assert_eq!(h.program().selected(), Some(1), "the click picks the big LUKS partition");
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().selected(), Some(2));
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.program().selected(), Some(0));
}
