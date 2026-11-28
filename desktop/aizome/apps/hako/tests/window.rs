//! Hako's window, headless: browsing in columns, lists and icons, and
//! pictures of each.

mod common;

use aizome_kit::theme::ThemeChoice;
use common::*;
use hako::ui::Msg;
use hako::ui::browser::Loc;
use hako::ui::fileview::Layout;

#[test]
fn shows_home_in_columns() {
    let s = Sandbox::new("columns");
    populate(&s);
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    h.focus_window(w, true);
    let b = h.program().browser(w).unwrap();
    assert_eq!(b.columns.len(), 1);
    assert_eq!(b.columns[0].loc, Loc::Folder(s.home()));
    let names: Vec<String> = b.columns[0].rows.iter().map(|e| e.display.clone()).collect();
    assert_eq!(
        names,
        ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos", "work"],
        "folders, sorted, hidden files hidden"
    );
    h.send(Msg::GoPath(w, s.home().join("work/sidestep/README.md")));
    settle(&mut h);
    h.save_png(w, out("columns-night.png"), 2.0);
    h.send(Msg::SetLayout(w, Layout::List));
    h.send(Msg::GoPath(w, s.home().join("Downloads")));
    settle(&mut h);
    h.save_png(w, out("list-night.png"), 2.0);
    h.send(Msg::SetLayout(w, Layout::Icons));
    h.send(Msg::GoPath(w, s.home().join("Pictures")));
    settle(&mut h);
    h.save_png(w, out("icons-night.png"), 2.0);
}

#[test]
fn day() {
    let s = Sandbox::new("day");
    populate(&s);
    let mut h = harness(&s, ThemeChoice::Day);
    let w = h.window();
    h.focus_window(w, true);
    h.send(Msg::GoPath(w, s.home().join("Pictures/wallpaper-draft.png")));
    settle(&mut h);
    h.save_png(w, out("columns-day.png"), 2.0);
}

#[test]
fn volumes_show_in_the_sidebar_with_eject_buttons() {
    let s = Sandbox::new("volumes");
    populate(&s);
    let stick = s.dir("media/SEKKA");
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    {
        let p = h.program_mut();
        p.volumes = vec![
            aizome_sys::disks::Volume {
                block: "/org/freedesktop/UDisks2/block_devices/sdb1".into(),
                name: "SEKKA".into(),
                icon_name: "drive-removable-media".into(),
                mount_point: Some(stick.to_string_lossy().into_owned()),
                ejectable: true,
                ..Default::default()
            },
            aizome_sys::disks::Volume {
                block: "/org/freedesktop/UDisks2/block_devices/loop0".into(),
                name: "sekka-nightly.iso".into(),
                icon_name: "media-optical".into(),
                image_file: Some("/home/u/Downloads/sekka-nightly.iso".into()),
                ..Default::default()
            },
        ];
    }
    h.send(Msg::GoPath(w, stick.clone()));
    settle(&mut h);
    assert!(h.texts(w).iter().any(|t| t == "SEKKA"));
    assert!(h.texts(w).iter().any(|t| t == "sekka-nightly.iso"));
    h.save_png(w, out("volumes-night.png"), 2.0);
}

#[test]
fn internal_data_volumes_show_in_the_sidebar_beside_removable_ones() {
    use aizome_sys::disks::{BlockDevice, DisksState, Drive, Filesystem, Partition, Swap};
    let s = Sandbox::new("internal-volumes");
    populate(&s);
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    // The machine of issue 345 (its system's partitions labelled here, so
    // they can be looked for) with a USB stick plugged in. UDisks marks
    // everything on the two built-in drives HintSystem.
    let disk = |path: &str, drive: &str| BlockDevice {
        path: path.into(),
        drive: Some(drive.into()),
        hint_system: drive != "usb",
        ..Default::default()
    };
    let part = |path: &str, table: &str, type_id: &str, label: &str, size: u64, mounts: &[&str]| BlockDevice {
        path: path.into(),
        size,
        id_label: label.into(),
        hint_system: table != "sdb",
        partition: Some(Partition { table: table.into(), type_id: type_id.into(), ..Default::default() }),
        filesystem: Some(Filesystem {
            mount_points: mounts.iter().map(|m| (*m).to_owned()).collect(),
            ..Default::default()
        }),
        ..Default::default()
    };
    const GIB: u64 = 1 << 30;
    let (esp, linux, swap) = (
        "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
        "0fc63daf-8483-4772-8e79-3d69d8477de4",
        "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f",
    );
    let state = DisksState {
        available: true,
        drives: vec![
            Drive { path: "nvme0".into(), ..Default::default() },
            Drive { path: "nvme1".into(), ..Default::default() },
            Drive { path: "usb".into(), removable: true, ejectable: true, ..Default::default() },
        ],
        blocks: vec![
            disk("nvme0n1", "nvme0"),
            part("nvme0n1p1", "nvme0n1", esp, "BOOT", GIB, &["/boot"]),
            part("nvme0n1p2", "nvme0n1", linux, "sekka", 9145 * GIB / 10, &["/nix/store", "/"]),
            BlockDevice {
                filesystem: None,
                swap: Some(Swap { active: true }),
                ..part("nvme0n1p3", "nvme0n1", swap, "SWAP", 16 * GIB, &[])
            },
            disk("nvme1n1", "nvme1"),
            part("nvme1n1p1", "nvme1n1", linux, "", 4769 * GIB / 10, &[]),
            disk("sdb", "usb"),
            part("sdb1", "sdb", linux, "STICK", 32 * GIB, &[]),
        ],
        ..Default::default()
    };
    h.program_mut().volumes = hako::ui::actions::sidebar_volumes(&state);
    settle(&mut h);
    let blocks: Vec<&str> = h.program().volumes.iter().map(|v| v.block.as_str()).collect();
    assert_eq!(blocks, ["nvme1n1p1", "sdb1"], "the second drive's data, then removable volumes as before");
    let stick = &h.program().volumes[1];
    assert!(stick.ejectable && !stick.internal, "a removable volume is still one to eject");
    assert_eq!(std::slice::from_ref(stick), state.volumes());
    let texts = h.texts(w);
    for shown in ["512 GB Volume", "STICK"] {
        assert!(texts.iter().any(|t| t == shown), "{shown} in {texts:?}");
    }
    for hidden in ["BOOT", "sekka", "SWAP"] {
        assert!(!texts.iter().any(|t| t == hidden), "{hidden} in {texts:?}");
    }
    h.save_png(w, out("internal-volumes-night.png"), 2.0);
}

#[test]
fn locked_volume_prompts_once_and_cancel_preserves_browser() {
    use hako::ui::app::Sheet;
    let s = Sandbox::new("locked-volume");
    populate(&s);
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    h.focus_window(w, true);
    let block = "/org/freedesktop/UDisks2/block_devices/sdb1".to_owned();
    h.program_mut().volumes.push(aizome_sys::disks::Volume {
        block: block.clone(),
        name: "PRIVATE".into(),
        locked: true,
        ..Default::default()
    });
    h.send(Msg::Mount(block.clone()));
    assert_eq!(h.program().sheets.len(), 1);
    let (&sheet, state) = h.program().sheets.iter().next().unwrap();
    assert!(matches!(state, Sheet::Unlock { window, pass, busy: false, .. } if *window == w && pass.is_empty()));
    assert!(h.texts(sheet).iter().any(|text| text == "Unlock PRIVATE"));
    h.send(Msg::Mount(block));
    assert_eq!(h.program().sheets.len(), 1, "repeat activation reuses the prompt");
    h.send(Msg::UnlockPass(sheet, "test-only".into()));
    assert!(h.click_text(sheet, "Cancel"));
    assert!(h.program().sheets.is_empty());
    assert!(h.program().mounting.is_empty());
    assert_eq!(h.program().browser(w).unwrap().columns[0].loc, Loc::Folder(s.home()));
}
