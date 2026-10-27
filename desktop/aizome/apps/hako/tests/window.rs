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
