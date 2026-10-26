//! Pictures of Tansu's main views in Night and Day, from the sample disks:
//! golden images (`AIZOME_UPDATE_GOLDENS=1` rewrites them).

use std::rc::Rc;

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use tansu::app::{Msg, SheetKind, SheetMsg, Tansu};
use tansu::mock::Mock;
use tansu::plan::Action;
use tansu::run::Run;

const THEMES: [(ThemeChoice, &str); 2] = [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")];

fn harness(theme: ThemeChoice) -> (Harness<Tansu>, Rc<Mock>) {
    let mock = Rc::new(Mock::default());
    let h = Harness::with(Tansu::new(mock.clone()), Appearance { theme, ..Appearance::default() }, false);
    (h, mock)
}

fn drive(h: &mut Harness<Tansu>, name: &str) {
    let key = h.program().drives.iter().find(|d| d.name.contains(name)).unwrap().key.clone();
    h.send(Msg::SelectDrive(key));
}

fn seg(h: &mut Harness<Tansu>, short: &str) {
    let i = h.program().layout.segs.iter().position(|s| s.short_name() == short).unwrap();
    h.send(Msg::SelectSeg(i));
}

fn largest_free(h: &mut Harness<Tansu>) {
    let l = &h.program().layout;
    let i = (0..l.segs.len()).filter(|&i| l.segs[i].is_free()).max_by_key(|&i| l.segs[i].size).unwrap();
    h.send(Msg::SelectSeg(i));
}

fn golden(h: &mut Harness<Tansu>, name: &str, theme: &str) {
    let w = h.window();
    assert_golden(h, w, format!("tests/snapshots/{name}-{theme}.png"), 1.0);
}

/// The stick with a partition deleted, a new one in its place and the
/// other relabelled: what the map and the pending strip show.
fn plan_on_the_stick(h: &mut Harness<Tansu>) {
    drive(h, "Cruzer");
    seg(h, "sdc2");
    h.send(Msg::Do(Action::Unmount));
    h.send(Msg::Do(Action::Delete));
    largest_free(h);
    h.send(Msg::Do(Action::New));
    h.send(Msg::Sheet(SheetMsg::SizeText("10".into())));
    h.send(Msg::Sheet(SheetMsg::Fs(4))); // exFAT
    h.send(Msg::Sheet(SheetMsg::Label("SHARE".into())));
    h.send(Msg::Sheet(SheetMsg::Submit));
    seg(h, "sdc1");
    h.send(Msg::Do(Action::Format));
    h.send(Msg::Sheet(SheetMsg::Fs(0)));
    h.send(Msg::Sheet(SheetMsg::Label("PHOTOS".into())));
    h.send(Msg::Sheet(SheetMsg::Erase(true)));
    h.send(Msg::Sheet(SheetMsg::Submit));
}

#[test]
fn the_system_disk() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        golden(&mut h, "system-disk", name);
    }
}

#[test]
fn the_archive_disk_with_a_locked_volume() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        drive(&mut h, "WDC");
        seg(&mut h, "sdb2");
        golden(&mut h, "archive-disk", name);
    }
}

#[test]
fn pending_operations_on_a_usb_stick() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        plan_on_the_stick(&mut h);
        h.send(Msg::TogglePending);
        golden(&mut h, "pending", name);
    }
}

#[test]
fn the_apply_sheet() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        drive(&mut h, "Crucial");
        seg(&mut h, "sda2");
        h.send(Msg::Do(Action::Delete));
        largest_free(&mut h);
        h.send(Msg::Do(Action::New));
        h.send(Msg::Sheet(SheetMsg::Submit));
        h.send(Msg::Apply);
        let w = h.window();
        h.type_text(w, "sd");
        golden(&mut h, "apply", name);
    }
}

#[test]
fn the_format_and_new_partition_sheets() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        drive(&mut h, "Cruzer");
        seg(&mut h, "sdc1");
        h.send(Msg::Do(Action::Format));
        h.send(Msg::Sheet(SheetMsg::Encrypt(true)));
        golden(&mut h, "format", name);
        h.send(Msg::CloseSheet);
        drive(&mut h, "Crucial");
        largest_free(&mut h);
        h.send(Msg::Do(Action::New));
        h.send(Msg::Sheet(SheetMsg::SizeText("8".into())));
        golden(&mut h, "new-partition", name);
    }
}

#[test]
fn the_resize_sheet() {
    let (mut h, _) = harness(ThemeChoice::Night);
    drive(&mut h, "SD SC64G");
    seg(&mut h, "mmcblk0p2");
    h.send(Msg::Do(Action::Resize));
    h.send(Msg::Sheet(SheetMsg::SizeText("24".into())));
    golden(&mut h, "resize", "night");
}

#[test]
fn a_run_under_way() {
    for (theme, name) in THEMES {
        let (mut h, mock) = harness(theme);
        plan_on_the_stick(&mut h);
        h.send(Msg::Apply);
        // Hold the run at the format, erasing, as UDisks's job would show.
        mock.add_job("format-erase", "/org/freedesktop/UDisks2/block_devices/sdc1", 0.42, 38_000_000);
        let t = h.program_mut();
        let mut run = Run::agree(&t.plan, &t.state);
        run.picture(2, "Erasing, then formatting…");
        t.run = Some(run);
        if let Some(s) = &mut t.sheet {
            s.kind = SheetKind::Running;
        }
        h.send(Msg::Changed);
        golden(&mut h, "running", name);
    }
}

#[test]
fn smart() {
    for (theme, name) in THEMES {
        let (mut h, _) = harness(theme);
        drive(&mut h, "WDC");
        h.send(Msg::ShowSmart);
        golden(&mut h, "smart", name);
    }
}

/// This computer's drives as UDisks2 reports them, read once (nothing is
/// changed: the window gets a frozen copy). Not a golden, as it depends on
/// the machine: `cargo test -p aizome-tansu --test look -- --ignored`
/// writes `this-computer-*.png` to the target's temporary directory.
#[test]
#[ignore = "reads this machine's drives through UDisks2: a picture to look at, not a check"]
fn this_computer() {
    let disks = aizome_sys::disks::Disks::new(aizome_sys::Wake::none());
    let start = std::time::Instant::now();
    while disks.snapshot().drives.is_empty() && start.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let state = (*disks.snapshot()).clone();
    assert!(state.available, "UDisks2 isn't running here");
    for (theme, name) in THEMES {
        let mock = Rc::new(tansu::mock::frozen(state.clone()));
        let mut h = Harness::with(Tansu::new(mock), Appearance { theme, ..Appearance::default() }, false);
        let w = h.window();
        let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("this-computer-{name}.png"));
        h.save_png(w, &out, 1.0);
        eprintln!("{}", out.display());
    }
}
