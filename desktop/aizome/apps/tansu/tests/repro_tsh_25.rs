//! TSH-25: a sheet (Format…, Resize…, Label…, Flags…, New Partition
//! Table…, Unlock…) is open for a device when that device goes or is
//! replaced: its stick is unplugged and Tansu falls back to another drive,
//! another stick of the same model comes up at the same names, or another
//! tool remakes it. The sheet must not quietly name what took the device's
//! place, Add to Pending must not queue anything for it, and an operation
//! already pending must not run on it.
//!
//! The in-memory mock only. The stick "goes" by the mock's power-off,
//! which removes it as UDisks would; a swap or another tool's change is
//! made to the mock's state directly. No real device is involved.

use std::rc::Rc;

use aizome_kit::testing::Harness;
use aizome_sys::disks::{DisksState, TableKind};
use tansu::app::{Msg, SheetKind, SheetMsg, Tansu};
use tansu::backend::Backend;
use tansu::mock::{self, Mock};
use tansu::plan::Action;

fn start(state: DisksState) -> (Harness<Tansu>, Rc<Mock>) {
    let mock = Rc::new(Mock::new(state));
    (Harness::new(Tansu::new(mock.clone())), mock)
}

/// The sample with the SD card listed first, so the drive Tansu falls back
/// to when the stick goes is another removable one.
fn sd_card_first() -> DisksState {
    let mut state = mock::sample();
    let sd = state.drives.iter().position(|d| d.model == "SD SC64G").expect("the SD card");
    let sd = state.drives.remove(sd);
    state.drives.insert(0, sd);
    state
}

/// The sample with a stick of a model that has no serial number, so UDisks
/// names another stick of the same model exactly as it names this one.
fn serialless_stick() -> DisksState {
    let mut state = mock::sample();
    for d in state.drives.iter_mut().filter(|d| d.model == "Cruzer Blade") {
        d.serial.clear();
    }
    // Filesystem UUIDs of its own (the sample's repeat across disks, and a
    // mounted one elsewhere with the same UUID bounds how far this shrinks),
    // and partition UUIDs as an MBR has them: its disk signature, then the
    // partition's number.
    for b in state.blocks.iter_mut().filter(|b| b.device.starts_with("/dev/sdc")) {
        let name = b.device.trim_start_matches("/dev/").to_owned();
        if let Some(p) = &mut b.partition {
            b.id_uuid = format!("stick-{name}");
            p.uuid = format!("4a3b2c1d-{:02}", p.number);
        }
    }
    state
}

/// The stick is pulled out and another of the same model plugged in: it
/// comes up at the same object paths and device nodes, with the same
/// layout, but its disk signature and filesystems are its own.
fn swap_for_another_stick(mock: &Mock) {
    mock.edit(|s| {
        for b in s.blocks.iter_mut().filter(|b| b.device.starts_with("/dev/sdc")) {
            if let Some(p) = &mut b.partition {
                b.id_uuid = format!("{}-another", b.id_uuid);
                p.uuid = format!("9f8e7d6c-{:02}", p.number);
            }
        }
    });
}

fn select(h: &mut Harness<Tansu>, drive: &str, seg: &str) {
    let key = h.program().drives.iter().find(|d| d.name.contains(drive)).expect(drive).key.clone();
    h.send(Msg::SelectDrive(key));
    let i = h.program().layout.segs.iter().position(|s| s.short_name() == seg).expect(seg);
    h.send(Msg::SelectSeg(i));
}

fn queued(h: &Harness<Tansu>) -> Vec<String> {
    h.program().plan.ops.iter().map(|o| format!("{} {}", tansu::plan::verb(&o.kind), o.device)).collect()
}

#[test]
fn a_format_sheet_for_a_stick_that_goes_formats_nothing_else() {
    let (mut h, mock) = start(sd_card_first());
    let key = h.program().drives.iter().find(|d| d.name.contains("Cruzer")).expect("the stick").key.clone();
    h.send(Msg::SelectDrive(key.clone()));
    let stick_disk = h.program().entry().and_then(|e| e.disk.clone()).expect("the stick's disk");
    let i = h.program().layout.segs.iter().position(|s| s.short_name() == "sdc2").expect("sdc2");
    h.send(Msg::SelectSeg(i));
    h.send(Msg::Do(Action::Unmount));
    h.send(Msg::Do(Action::Format));
    assert!(matches!(h.program().sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Format(_))), "the format sheet");

    // The stick is unplugged (UDisks drops it).
    let _ = mock.power_off(&key);
    h.send(Msg::Changed);
    let names = h.program().layout.segs.get(i).map(|s| s.short_name());
    let still_open = matches!(h.program().sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Format(_)));
    // The sheet still says what it was for, not what took its place.
    if let Some(key) = h.program().sheet.as_ref().map(|s| s.key) {
        let texts = h.texts(key);
        assert!(
            texts.iter().any(|t| t == "Format sdc2")
                && !texts.iter().any(|t| t.starts_with("Format mmcblk0") || t.starts_with("Everything on mmcblk0")),
            "the sheet's title changed with the layout: {texts:?}"
        );
    }
    // Add to Pending.
    if still_open {
        h.send(Msg::Sheet(SheetMsg::Fs(0)));
        h.send(Msg::Sheet(SheetMsg::Submit));
    }
    let t = h.program();
    let wrong: Vec<String> = t
        .plan
        .ops
        .iter()
        .filter(|o| o.disk != stick_disk)
        .map(|o| format!("{} on {}", tansu::plan::verb(&o.kind), o.device))
        .collect();
    let typed = t.disks_to_type();
    assert!(
        wrong.is_empty(),
        "the gone stick's Format sheet (its segment now {names:?} on {:?}) queued {wrong:?}; Apply would ask \
         to type {typed:?}",
        t.entry().map(|e| e.name.clone())
    );
    assert!(mock.calls().iter().all(|c| !c.starts_with("Format")), "{:?}", mock.calls());
}

#[test]
fn sheets_open_on_a_stick_swapped_for_another_queue_nothing() {
    type Step = fn(&mut Harness<Tansu>);
    let cases: [(&str, Step, Step); 5] = [
        ("format", |h| h.send(Msg::Do(Action::Format)), |h| h.send(Msg::Sheet(SheetMsg::Fs(0)))),
        ("resize", |h| h.send(Msg::Do(Action::Resize)), |h| h.send(Msg::Sheet(SheetMsg::Size(4096.0)))),
        ("label", |h| h.send(Msg::Do(Action::Label)), |h| h.send(Msg::Sheet(SheetMsg::Label("OTHER".into())))),
        ("flags", |h| h.send(Msg::Do(Action::Flags)), |h| h.send(Msg::Sheet(SheetMsg::Flag(3, true)))),
        (
            "new partition table",
            |h| {
                select(h, "Cruzer", "sdc2");
                h.send(Msg::Do(Action::Unmount));
                h.send(Msg::NewTable);
            },
            |h| h.send(Msg::Sheet(SheetMsg::Table(TableKind::Gpt))),
        ),
    ];
    let mut wrong = Vec::new();
    for (what, open, edit) in cases {
        let (mut h, mock) = start(serialless_stick());
        select(&mut h, "Cruzer", "sdc1");
        open(&mut h);
        assert!(h.program().sheet.is_some(), "the {what} sheet");
        swap_for_another_stick(&mock);
        h.send(Msg::Changed);
        edit(&mut h);
        h.send(Msg::Sheet(SheetMsg::Submit));
        let queued = queued(&h);
        if !queued.is_empty() {
            wrong.push(format!("the {what} sheet for the first stick queued {queued:?} for the second"));
        } else if h.program().sheet.as_ref().is_some_and(|s| s.error.is_none()) {
            wrong.push(format!("the {what} sheet queued nothing but doesn't say why"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_table_sheet_for_a_stick_that_goes_wipes_no_other_disk() {
    let (mut h, mock) = start(sd_card_first());
    select(&mut h, "Cruzer", "sdc2");
    h.send(Msg::Do(Action::Unmount));
    h.send(Msg::NewTable);
    assert!(matches!(h.program().sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Table { .. })));
    let key = h.program().drive.clone().expect("the stick");
    let _ = mock.power_off(&key);
    h.send(Msg::Changed);
    h.send(Msg::Sheet(SheetMsg::Table(TableKind::Gpt)));
    h.send(Msg::Sheet(SheetMsg::Submit));
    let queued = queued(&h);
    assert!(queued.is_empty(), "the gone stick's New Partition Table sheet queued {queued:?}");
}

#[test]
fn an_unlock_sheet_tries_its_passphrase_on_no_other_volume() {
    let (mut h, mock) = start(mock::sample());
    select(&mut h, "WDC", "sdb2");
    h.send(Msg::Do(Action::Unlock));
    assert!(matches!(h.program().sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Unlock { .. })));
    // The archive disk goes (an external enclosure unplugged): Tansu falls
    // back to the system disk.
    mock.edit(|s| {
        s.drives.retain(|d| !d.model.starts_with("WD40"));
        s.blocks.retain(|b| !b.device.starts_with("/dev/sdb"));
    });
    h.send(Msg::Changed);
    h.send(Msg::Sheet(SheetMsg::Pass("correct horse".into())));
    h.send(Msg::Sheet(SheetMsg::Submit));
    let calls = mock.calls();
    assert!(calls.iter().all(|c| !c.starts_with("Unlock")), "the passphrase was tried on another volume: {calls:?}");
}

#[test]
fn pending_operations_do_not_follow_a_swapped_stick() {
    let (mut h, mock) = start(serialless_stick());
    select(&mut h, "Cruzer", "sdc1");
    h.send(Msg::Do(Action::Format));
    h.send(Msg::Sheet(SheetMsg::Fs(0)));
    h.send(Msg::Sheet(SheetMsg::Submit));
    h.send(Msg::Do(Action::Flags));
    h.send(Msg::Sheet(SheetMsg::Flag(3, true)));
    h.send(Msg::Sheet(SheetMsg::Submit));
    assert_eq!(queued(&h), ["Format /dev/sdc1", "Flags on /dev/sdc1"]);

    swap_for_another_stick(&mock);
    h.send(Msg::Changed);
    h.send(Msg::Apply);
    if h.program().sheet.is_some() {
        h.send(Msg::Sheet(SheetMsg::Submit));
    }
    for _ in 0..4 {
        h.send(Msg::Tick);
    }
    let calls = mock.calls();
    assert!(calls.is_empty(), "operations queued for the first stick ran on the second: {calls:?}");
    assert!(
        h.program().notice.as_ref().is_some_and(|(n, error)| *error && n.contains("sdc1")),
        "Tansu says what it dropped: {:?}",
        h.program().notice
    );
}
