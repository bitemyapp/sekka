//! The sheets: what each operation asks before it joins the pending list,
//! the Apply sheet that says what will happen in plain words, the run's
//! progress, and SMART.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::disks::{SelfTest, TableKind, format_size};

use crate::app::{CreateForm, FormatForm, Msg, ResizeForm, Sheet, SheetKind, SheetMsg, Tansu, UNITS};
use crate::fs;
use crate::layout::{Flags, MIB, Seg};
use crate::plan::Loss;
use crate::run::Status;

fn s(m: SheetMsg) -> Msg {
    Msg::Sheet(m)
}

pub fn view(t: &Tansu, sheet: &Sheet) -> Element<Msg> {
    let (width, body) = match &sheet.kind {
        SheetKind::Format(f) => (470.0, format(t, f)),
        SheetKind::Create(f) => (500.0, create(t, f)),
        SheetKind::Resize(f) => (500.0, resize(t, f)),
        SheetKind::Label { seg, label } => (400.0, label_sheet(t, *seg, label)),
        SheetKind::Flags { seg, flags } => (460.0, flags_sheet(t, *seg, *flags)),
        SheetKind::Table { kind } => (470.0, table_sheet(t, *kind)),
        SheetKind::Unlock { seg, pass } => (400.0, unlock(t, *seg, pass)),
        SheetKind::Attach { path, read_only } => (480.0, attach(path, *read_only)),
        SheetKind::Apply(f) => (580.0, apply(t, f)),
        SheetKind::Running => (580.0, running(t)),
        SheetKind::Smart { drive, attrs, .. } => (580.0, smart(t, drive, attrs.as_ref())),
    };
    let mut parts = body;
    if let Some(e) = &sheet.error {
        parts.insert(
            parts.len().saturating_sub(1),
            text(e.clone()).role(Role::Danger).wrap().width(Length::Fill).into(),
        );
    }
    aizome_kit::widgets::layout::column(parts).spacing(12.0).padding([18.0, 20.0]).width(width).into()
}

fn title(t: &str) -> Element<Msg> {
    text(t.to_owned()).size(15.0).semibold().into()
}

fn sub(t: impl Into<String>) -> Element<Msg> {
    text(t.into()).role(Role::Text3).wrap().width(Length::Fill).into()
}

fn note(t: impl Into<String>) -> Element<Msg> {
    text(t.into()).size(11.5).role(Role::Text3).wrap().width(Length::Fill).into()
}

/// A warning box: red for loss, amber for risk.
fn warning(icon_: Icon, role: Role, message: impl Into<String>) -> Element<Msg> {
    container(
        row![icon(icon_).size(16.0).role(role), text(message.into()).wrap().width(Length::Fill)]
            .spacing(8.0)
            .align(Align::Start),
    )
    .padding([8.0, 10.0])
    .fill(Tone::Sunk)
    .border(Edge::Line)
    .width(Length::Fill)
    .into()
}

fn buttons(confirm: &str, enabled: bool, destructive: bool) -> Element<Msg> {
    let b = button(confirm.to_owned()).default().on_press_maybe(enabled.then_some(s(SheetMsg::Submit)));
    let b = if destructive { b.destructive() } else { b };
    row![spacer(), button("Cancel").cancel().on_press(Msg::CloseSheet), b].spacing(8.0).width(Length::Fill).into()
}

fn field_row(label: &str, control: impl Into<Element<Msg>>) -> Element<Msg> {
    row![text(label.to_owned()).role(Role::Text2).width(92.0), control.into()].spacing(10.0).width(Length::Fill).into()
}

fn seg_words(seg: &Seg) -> String {
    let mut w = format!("{} · {}", format_size(seg.size), fs::name(seg.shown_fs()));
    if !seg.shown_label().is_empty() {
        w.push_str(&format!(" “{}”", seg.shown_label()));
    }
    if let Some(u) = seg.shown_used() {
        w.push_str(&format!(" · {} used", format_size(u)));
    }
    w
}

fn fs_choices(t: &Tansu, unformatted: bool) -> Vec<String> {
    let mut v: Vec<String> = fs::FORMATS
        .iter()
        .map(|f| match t.missing.get(*f) {
            Some(program) => format!("{} (needs {program})", fs::name(f)),
            None if t.state.supported_filesystems.iter().any(|s| s == f) => fs::name(f),
            None => format!("{} (not installed)", fs::name(f)),
        })
        .collect();
    if unformatted {
        v.push("Unformatted".into());
    }
    v
}

fn label_error(fs_type: &str, label: &str) -> Option<String> {
    match fs::label_limit(fs_type) {
        Some(n) if label.trim().len() > n => Some(format!("{} labels have at most {n} bytes.", fs::name(fs_type))),
        _ => None,
    }
}

fn encryption(encrypt: bool, pass: &str, pass2: &str) -> Vec<Element<Msg>> {
    let mut v: Vec<Element<Msg>> =
        vec![checkbox("Encrypt with LUKS", encrypt).on_toggle(|on| s(SheetMsg::Encrypt(on))).into()];
    if encrypt {
        v.push(field_row(
            "Passphrase",
            text_field(pass).password().id("pass").on_change(|p| s(SheetMsg::Pass(p))).width(Length::Fill),
        ));
        let mismatch = (!pass2.is_empty() && pass != pass2).then_some("The passphrases don't match.");
        v.push(field_row(
            "Again",
            text_field(pass2)
                .password()
                .id("pass2")
                .on_change(|p| s(SheetMsg::Pass2(p)))
                .on_submit(s(SheetMsg::Submit))
                .error(mismatch)
                .width(Length::Fill),
        ));
        v.push(note("Unlocking it will ask for this passphrase. Lost, it can't be recovered — nor can what's inside."));
    }
    v
}

fn format(t: &Tansu, f: &FormatForm) -> Vec<Element<Msg>> {
    let Some(seg) = t.layout.segs.get(f.seg) else { return vec![title("Format")] };
    let fs_type = fs::FORMATS[f.fs.min(fs::FORMATS.len() - 1)];
    let new = seg.pending.contains(&crate::layout::Mark::New);
    let mut v = vec![title(&format!("Format {}", seg.short_name())), sub(seg_words(seg))];
    if !new {
        v.push(warning(
            Icon::TriangleAlert,
            Role::Danger,
            format!("Everything on {} will be erased.", seg.short_name()),
        ));
    }
    v.push(field_row(
        "Filesystem",
        dropdown(fs_choices(t, false), Some(f.fs)).on_select(|i| s(SheetMsg::Fs(i))).width(Length::Fill),
    ));
    v.push(note(fs::describe(fs_type)));
    v.push(field_row(
        "Label",
        text_field(&f.label)
            .id("format-label")
            .placeholder("None")
            .on_change(|l| s(SheetMsg::Label(l)))
            .on_submit(s(SheetMsg::Submit))
            .error(label_error(fs_type, &f.label))
            .width(Length::Fill),
    ));
    v.push(
        checkbox("Overwrite every byte with zeros first (slow)", f.erase).on_toggle(|on| s(SheetMsg::Erase(on))).into(),
    );
    if fs_type != "swap" {
        v.extend(encryption(f.encrypt, &f.pass, &f.pass2));
    }
    v.push(note("Nothing changes until you apply the pending operations."));
    v.push(buttons("Add to Pending", true, false));
    v
}

fn size_controls(
    size: u64,
    min: u64,
    max: u64,
    text_: &str,
    unit: usize,
    id: &str,
    error: Option<&str>,
) -> Element<Msg> {
    let lo = (min / MIB).max(1) as f64;
    let hi = (max / MIB).max(1) as f64;
    let mut parts: Vec<Element<Msg>> = vec![
        slider((size / MIB) as f64, lo..=hi.max(lo))
            .step(1.0)
            .on_change(|v| s(SheetMsg::Size(v)))
            .width(Length::Fill)
            .into(),
        row![
            text_field(text_)
                .id(id.to_owned())
                .error(error.map(|_| "Invalid size"))
                .on_change(|v| s(SheetMsg::SizeText(v)))
                .on_submit(s(SheetMsg::Submit))
                .mono()
                .width(120.0),
            dropdown(UNITS.map(|u| u.0), Some(unit)).on_select(|u| s(SheetMsg::Unit(u))).width(72.0),
            spacer(),
            text(format!("{} to {}", format_size(min), format_size(max))).size(11.5).role(Role::Text3).tabular(),
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .into(),
    ];
    if let Some(error) = error {
        parts.push(text(error.to_owned()).role(Role::Danger).wrap().width(Length::Fill).into());
    }
    aizome_kit::widgets::layout::column(parts).spacing(8.0).width(Length::Fill).into()
}

fn create(t: &Tansu, f: &CreateForm) -> Vec<Element<Msg>> {
    let fs_type = fs::FORMATS.get(f.fs).copied().unwrap_or("");
    let mut v = vec![
        title("New Partition"),
        sub(format!(
            "In {} of unallocated space on {} at {}.",
            format_size(f.max),
            t.layout.device.trim_start_matches("/dev/"),
            format_size(f.offset)
        )),
        field_row(
            "Size",
            size_controls(f.size, MIB, f.max, &f.size_text, f.unit, "create-size", f.size_error.as_deref()),
        ),
        field_row(
            "Filesystem",
            dropdown(fs_choices(t, true), Some(f.fs)).on_select(|i| s(SheetMsg::Fs(i))).width(Length::Fill),
        ),
    ];
    if !fs_type.is_empty() {
        v.push(note(fs::describe(fs_type)));
        v.push(field_row(
            "Label",
            text_field(&f.label)
                .placeholder("None")
                .on_change(|l| s(SheetMsg::Label(l)))
                .on_submit(s(SheetMsg::Submit))
                .error(label_error(fs_type, &f.label))
                .width(Length::Fill),
        ));
    }
    if t.layout.table == "gpt" {
        v.push(field_row(
            "Name",
            text_field(&f.name)
                .placeholder("None (GPT's partition name)")
                .on_change(|n| s(SheetMsg::Name(n)))
                .width(Length::Fill),
        ));
    }
    if !fs_type.is_empty() && fs_type != "swap" {
        v.extend(encryption(f.encrypt, &f.pass, &f.pass2));
    }
    let left = f.max.saturating_sub(f.size);
    if left >= MIB {
        v.push(note(format!("{} stays unallocated after it.", format_size(left))));
    }
    v.push(buttons("Add to Pending", f.size >= MIB && f.size_error.is_none(), false));
    v
}

fn resize(t: &Tansu, f: &ResizeForm) -> Vec<Element<Msg>> {
    let Some(seg) = t.layout.segs.get(f.seg) else { return vec![title("Resize")] };
    let r = fs::resizable(&seg.fs);
    let mut v = vec![
        title(&format!("Resize {}", seg.short_name())),
        sub(seg_words(seg)),
        warning(
            Icon::Info,
            Role::Accent,
            "Tansu moves a partition's end; its start stays where it is. Moving a start means moving all the data, which UDisks doesn't do: use GParted from a live system for that.",
        ),
        field_row(
            "New size",
            size_controls(f.size, f.min, f.max, &f.size_text, f.unit, "resize-size", f.size_error.as_deref()),
        ),
    ];
    let mut can = Vec::new();
    if r.grow {
        can.push(if f.max > f.from {
            format!("It can grow by {} into the free space after it.", format_size(f.max - f.from))
        } else {
            "There's no free space after it to grow into.".into()
        });
    }
    if r.shrink {
        can.push(match f.last_used {
            Some(u) => format!("It can shrink to {} ({} was in use when last mounted).", format_size(f.min), format_size(u)),
            None => "Unmounted, how full it is isn't known: if its files don't fit the new size, the filesystem refuses to shrink and nothing changes. (Mount it once to see.)".to_owned(),
        });
    } else if let Some(why) = r.why_not {
        can.push(why.to_owned());
    }
    v.push(note(can.join(" ")));
    if f.size != f.from {
        v.push(warning(
            Icon::TriangleAlert,
            Role::Warning,
            "Resizing rewrites the filesystem. If it's interrupted, data can be lost: back up first.",
        ));
    }
    v.push(buttons(
        if f.size > f.from {
            "Grow"
        } else if f.size < f.from {
            "Shrink"
        } else {
            "Resize"
        },
        f.size != f.from && f.size_error.is_none(),
        false,
    ));
    v
}

fn label_sheet(t: &Tansu, seg: usize, label: &str) -> Vec<Element<Msg>> {
    let Some(sg) = t.layout.segs.get(seg) else { return vec![title("Label")] };
    let limit = fs::label_limit(&sg.fs);
    vec![
        title(&format!("Label {}", sg.short_name())),
        sub(seg_words(sg)),
        text_field(label)
            .id("label-field")
            .placeholder("None")
            .on_change(|l| s(SheetMsg::Label(l)))
            .on_submit(s(SheetMsg::Submit))
            .error(label_error(&sg.fs, label))
            .width(Length::Fill)
            .into(),
        note(match limit {
            Some(n) => format!(
                "{} labels have up to {n} bytes. The label names the volume on the desktop and in Files.",
                fs::name(&sg.fs)
            ),
            None => "The label names the volume on the desktop and in Files.".into(),
        }),
        buttons("Add to Pending", true, false),
    ]
}

fn flags_sheet(t: &Tansu, seg: usize, flags: Flags) -> Vec<Element<Msg>> {
    let Some(sg) = t.layout.segs.get(seg) else { return vec![title("Flags")] };
    let offered = Flags::offered(&t.layout.table);
    let item = |label: &str, on: bool, which: u8, what: &str| -> Element<Msg> {
        column![
            checkbox(label.to_owned(), on).on_toggle(move |v| s(SheetMsg::Flag(which, v))),
            note(format!("      {what}"))
        ]
        .spacing(2.0)
        .width(Length::Fill)
        .into()
    };
    let mut v = vec![title(&format!("Flags of {}", sg.short_name())), sub(seg_words(sg))];
    v.push(item(
        "ESP",
        flags.esp,
        1,
        "An EFI System Partition: where UEFI firmware looks for boot loaders. It should hold FAT.",
    ));
    if offered.bios_grub {
        v.push(item(
            "BIOS boot",
            flags.bios_grub,
            2,
            "Room for GRUB on a GPT disk a BIOS boots: about 1 MiB, unformatted.",
        ));
    }
    v.push(item(
        "boot",
        flags.boot,
        0,
        if t.layout.table == "dos" {
            "The active partition, which a BIOS boots from."
        } else {
            "GPT's legacy BIOS bootable attribute."
        },
    ));
    v.push(item("hidden", flags.hidden, 3, "Windows and other systems won't mount it by themselves."));
    v.push(note("The partition's type and attributes change; what's on it doesn't."));
    v.push(buttons("Add to Pending", true, false));
    v
}

fn table_sheet(t: &Tansu, kind: TableKind) -> Vec<Element<Msg>> {
    let parts: Vec<String> = t.layout.partitions().map(|p| p.short_name()).collect();
    let device = t.layout.device.trim_start_matches("/dev/").to_owned();
    let big = t.layout.size > (2u64 << 40);
    let mut v = vec![
        title(&format!("New Partition Table on {device}")),
        radio("GPT", TableKind::Gpt, Some(kind)).on_select(|k| s(SheetMsg::Table(k))).into(),
        note("      Any size, up to 128 partitions. For any computer from the last fifteen years."),
        radio("MBR (DOS)", TableKind::Dos, Some(kind)).on_select(|k| s(SheetMsg::Table(k))).enabled(!big).into(),
        note(if big {
            "      Not for disks over 2 TB.".to_owned()
        } else {
            "      Up to 2 TB and four primary partitions: for old computers and some cameras.".to_owned()
        }),
    ];
    let loss = if parts.is_empty() {
        format!("Anything on {device} will be lost.")
    } else {
        format!("Every partition on {device} will be lost: {}.", parts.join(", "))
    };
    v.push(warning(Icon::TriangleAlert, Role::Danger, loss));
    v.push(buttons("Add to Pending", true, true));
    v
}

fn unlock(t: &Tansu, seg: usize, pass: &str) -> Vec<Element<Msg>> {
    let name = t.layout.segs.get(seg).map(|s| s.short_name()).unwrap_or_default();
    vec![
        title(&format!("Unlock {name}")),
        sub("Its passphrase opens the volume until it's locked or the computer restarts."),
        text_field(pass)
            .password()
            .id("unlock-pass")
            .placeholder("Passphrase")
            .on_change(|p| s(SheetMsg::Pass(p)))
            .on_submit(s(SheetMsg::Submit))
            .width(Length::Fill)
            .into(),
        buttons("Unlock", true, false),
    ]
}

fn attach(path: &str, read_only: bool) -> Vec<Element<Msg>> {
    vec![
        title("Attach a Disk Image"),
        sub(
            "An .iso or .img file shows as a drive: its partitions can be mounted, and, attached for writing, changed.",
        ),
        row![
            text_field(path)
                .id("attach-path")
                .placeholder("/path/to/image.iso")
                .on_change(|p| s(SheetMsg::Path(p)))
                .on_submit(s(SheetMsg::Submit))
                .width(Length::Fill),
            button("Choose…").on_press(s(SheetMsg::Choose)),
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .into(),
        checkbox("Read-only", read_only).on_toggle(|on| s(SheetMsg::ReadOnly(on))).into(),
        buttons("Attach", !path.trim().is_empty(), false),
    ]
}

fn apply(t: &Tansu, f: &crate::app::ApplyForm) -> Vec<Element<Msg>> {
    let n = t.plan.len();
    let mut v = vec![title(&format!("Apply {n} Operation{}", if n == 1 { "" } else { "s" })), sub("In this order:")];
    let mut destroys = false;
    let mut list: Vec<Element<Msg>> = Vec::new();
    for (k, op) in t.plan.ops.iter().enumerate() {
        let w = t.plan.words(op, &t.state);
        let mut lines: Vec<Element<Msg>> = vec![text(w.title.clone()).semibold().wrap().width(Length::Fill).into()];
        if !w.detail.is_empty() {
            lines.push(text(w.detail.clone()).size(12.0).role(Role::Text3).wrap().width(Length::Fill).into());
        }
        match &w.loss {
            Loss::Destroys(m) => {
                destroys = true;
                lines.push(
                    row![
                        icon(Icon::TriangleAlert).size(14.0).role(Role::Danger),
                        text(m.clone()).size(12.0).role(Role::Danger).wrap().width(Length::Fill)
                    ]
                    .spacing(6.0)
                    .align(Align::Start)
                    .width(Length::Fill)
                    .into(),
                );
            }
            Loss::Risk(m) => {
                lines.push(
                    row![
                        icon(Icon::TriangleAlert).size(14.0).role(Role::Warning),
                        text(m.clone()).size(12.0).role(Role::Warning).wrap().width(Length::Fill)
                    ]
                    .spacing(6.0)
                    .align(Align::Start)
                    .width(Length::Fill)
                    .into(),
                );
            }
            Loss::None => {}
        }
        list.push(
            row![
                text(format!("{}", k + 1)).mono().role(Role::Text3).width(20.0),
                aizome_kit::widgets::layout::column(lines).spacing(3.0).width(Length::Fill),
            ]
            .spacing(6.0)
            .align(Align::Start)
            .width(Length::Fill)
            .into(),
        );
    }
    v.push(well(list, 12.0, 52.0, 300.0));
    for p in t.plan.problems(&t.state) {
        v.push(warning(Icon::CircleX, Role::Danger, p));
    }
    for (i, (name, typed)) in f.typed.iter().enumerate() {
        v.push(note(format!("{name} is a built-in disk. To confirm, type its name:")));
        let ok = typed.trim() == name;
        v.push(
            row![
                text_field(typed)
                    .id(format!("type-{i}"))
                    .mono()
                    .placeholder(name.clone())
                    .on_change(move |x| s(SheetMsg::Typed(i, x)))
                    .on_submit(s(SheetMsg::Submit))
                    .width(Length::Fill),
                icon(if ok { Icon::CircleCheck } else { Icon::Circle }).size(16.0).role(if ok {
                    Role::Success
                } else {
                    Role::Faint
                }),
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
        );
    }
    v.push(note("Administrator rights may be asked for. Operations run one at a time; the first that fails stops the rest, which stay pending."));
    let ready = t.plan.problems(&t.state).is_empty() && f.typed.iter().all(|(name, typed)| typed.trim() == name);
    v.push(buttons(&format!("Apply {n} Operation{}", if n == 1 { "" } else { "s" }), ready, destroys));
    v
}

fn running(t: &Tansu) -> Vec<Element<Msg>> {
    let Some(run) = &t.run else { return vec![title("Done"), buttons("Close", true, false)] };
    let busy = run.is_running();
    let mut v = vec![title(if busy { "Applying…" } else { "Applied" })];
    let mut list: Vec<Element<Msg>> = Vec::new();
    for (k, item) in run.items.iter().enumerate() {
        let (glyph, role, state) = match &item.status {
            Status::Waiting => (Icon::Circle, Role::Faint, "Waiting".to_owned()),
            Status::Running(w) => (Icon::CircleDot, Role::Accent, w.clone()),
            Status::Done => (Icon::CircleCheck, Role::Success, "Done".to_owned()),
            Status::Failed(e) => (Icon::CircleX, Role::Danger, e.clone()),
        };
        let mut lines: Vec<Element<Msg>> = vec![
            text(item.words.title.clone()).semibold().wrap().width(Length::Fill).into(),
            text(state)
                .size(12.0)
                .role(if matches!(item.status, Status::Failed(_)) { Role::Danger } else { Role::Text3 })
                .wrap()
                .width(Length::Fill)
                .into(),
        ];
        if matches!(item.status, Status::Running(_)) && k == run.current {
            let job = run.working_on().and_then(|p| t.state.jobs_for(p).next());
            match job {
                Some(j) => {
                    let bar: Element<Msg> = match j.progress {
                        Some(p) => progress(p).width(Length::Fill).into(),
                        None => progress_indeterminate().width(Length::Fill).into(),
                    };
                    let mut facts = Vec::new();
                    if let Some(p) = j.progress {
                        facts.push(format!("{:.0}%", p * 100.0));
                    }
                    if j.rate > 0 {
                        facts.push(format!("{}/s", format_size(j.rate)));
                    }
                    if let Some(r) = j.remaining {
                        let m = r.as_secs() / 60;
                        facts.push(if m >= 1 { format!("about {m} min left") } else { "under a minute left".into() });
                    }
                    let mut row_: Vec<Element<Msg>> =
                        vec![bar, text(facts.join(" · ")).size(11.5).tabular().role(Role::Text3).into()];
                    if j.cancelable {
                        row_.push(button("Cancel").compact().on_press(Msg::CancelJob(j.path.clone())).into());
                    }
                    lines.push(aizome_kit::widgets::layout::row(row_).spacing(8.0).width(Length::Fill).into());
                }
                None => lines.push(progress_indeterminate().width(Length::Fill).into()),
            }
        }
        list.push(
            row![
                icon(glyph).size(16.0).role(role),
                aizome_kit::widgets::layout::column(lines).spacing(3.0).width(Length::Fill)
            ]
            .spacing(8.0)
            .align(Align::Start)
            .width(Length::Fill)
            .into(),
        );
    }
    v.push(well(list, 12.0, 52.0, 340.0));
    match &run.outcome {
        None => {
            v.push(note(if run.stop_requested {
                "Stopping once this operation is done."
            } else {
                "Keep the drives plugged in until this is done."
            }));
            v.push(
                row![
                    spacer(),
                    button("Stop After This One").on_press_maybe((!run.stop_requested).then_some(Msg::StopRun))
                ]
                .width(Length::Fill)
                .into(),
            );
        }
        Some(outcome) => {
            let words = match outcome {
                crate::run::Outcome::Done => "Every operation ran.".to_owned(),
                crate::run::Outcome::Failed { .. } => {
                    "It stopped at the operation that failed; those after it are still pending.".to_owned()
                }
                crate::run::Outcome::Stopped => "Stopped: what didn't run is still pending.".to_owned(),
            };
            v.push(note(words));
            v.push(row![spacer(), button("Done").default().on_press(s(SheetMsg::Submit))].width(Length::Fill).into());
        }
    }
    v
}

fn smart(
    t: &Tansu,
    drive: &str,
    attrs: Option<&Result<Vec<aizome_sys::disks::SmartAttribute>, String>>,
) -> Vec<Element<Msg>> {
    let d = t.state.drive(drive);
    let name = d.map(|d| d.display_name()).unwrap_or_default();
    let smart = d.and_then(|d| d.smart.as_ref());
    let health = smart.map(|s| s.health()).unwrap_or_default();
    let mut v = vec![title(&format!("SMART — {name}"))];
    let mut facts = vec![super::glyphs::health_words(health).to_owned()];
    if let Some(s) = smart {
        if let Some(c) = s.temperature_celsius {
            facts.push(format!("{c:.0} °C"));
        }
        if let Some(h) = s.power_on_hours {
            facts.push(if h >= 48 { format!("powered on {} days", h / 24) } else { format!("powered on {h} hours") });
        }
        if let Some(b) = s.bad_sectors.filter(|b| *b > 0) {
            facts.push(format!("{b} bad sectors"));
        }
        for w in &s.critical_warnings {
            facts.push(format!("warning: {w}"));
        }
    }
    v.push(
        row![super::glyphs::dot(health), text(facts.join(" · ")).wrap().width(Length::Fill)]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
    );
    match attrs {
        None => v.push(
            row![progress_indeterminate().width(120.0), text("Reading the drive's attributes…").role(Role::Text3)]
                .spacing(8.0)
                .into(),
        ),
        Some(Err(e)) => v.push(note(e.clone())),
        Some(Ok(list)) => {
            let ata = list.iter().any(|a| a.id.is_some());
            let mut rows: Vec<Element<Msg>> = Vec::new();
            for a in list {
                let mut cells: Vec<Element<Msg>> = Vec::new();
                if ata {
                    cells.push(
                        text(a.id.map(|i| i.to_string()).unwrap_or_default())
                            .mono()
                            .size(11.5)
                            .role(Role::Text3)
                            .width(32.0)
                            .into(),
                    );
                }
                cells.push(
                    text(a.name.clone())
                        .role(if a.failing { Role::Danger } else { Role::Text })
                        .width(Length::Fill)
                        .into(),
                );
                cells.push(
                    text(a.value.clone()).tabular().role(if a.failing { Role::Danger } else { Role::Text2 }).into(),
                );
                if ata {
                    let n = |x: Option<i32>| x.map(|x| x.to_string()).unwrap_or_else(|| "—".into());
                    cells.push(
                        text(format!("{} / {} / {}", n(a.normalized), n(a.worst), n(a.threshold)))
                            .mono()
                            .size(11.5)
                            .role(Role::Text3)
                            .width(104.0)
                            .into(),
                    );
                }
                rows.push(aizome_kit::widgets::layout::row(cells).spacing(10.0).width(Length::Fill).into());
            }
            if ata {
                v.push(note(
                    "Normalised value / worst / threshold: higher is better; at the threshold, an attribute fails.",
                ));
            }
            v.push(well(rows, 5.0, 18.0, 260.0));
        }
    }
    if let Some(s) = smart.filter(|s| s.supported) {
        let status = match (s.selftest_status.as_str(), s.selftest_percent_remaining) {
            ("inprogress", Some(p)) => format!("A self-test is running: {p}% to go."),
            ("inprogress", None) => "A self-test is running.".into(),
            ("success", _) | ("", _) => "The last self-test passed.".into(),
            (other, _) => format!("The last self-test: {other}."),
        };
        let testing = s.selftest_status == "inprogress";
        v.push(note(format!("{status} A short test takes a couple of minutes; an extended one reads the whole drive, which takes hours. The drive stays usable meanwhile.")));
        v.push(
            row![
                button("Short Self-Test").on_press_maybe((!testing).then_some(s_(SelfTest::Short))),
                button("Extended Self-Test").on_press_maybe((!testing).then_some(s_(SelfTest::Extended))),
                spacer(),
                button("Close").default().on_press(Msg::CloseSheet),
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
        );
    } else {
        v.push(row![spacer(), button("Close").default().on_press(Msg::CloseSheet)].width(Length::Fill).into());
    }
    v
}

fn s_(test: SelfTest) -> Msg {
    s(SheetMsg::SelfTest(test))
}

/// A sunk, edged well holding `items`: as tall as they are, scrolling
/// past `max` (`row` estimates an item's height: a scroll view takes all the height it's
/// offered, so it's used only when the items need it).
pub fn well(items: Vec<Element<Msg>>, spacing: f64, row: f64, max: f64) -> Element<Msg> {
    let many = items.len() as f64 * (spacing + row) > max;
    let col = aizome_kit::widgets::layout::column(items).spacing(spacing).padding(10.0).width(Length::Fill);
    let inner: Element<Msg> = if many { scroll(col).width(Length::Fill).height(max).into() } else { col.into() };
    container(inner).fill(Tone::Sunk).border(Edge::Line).width(Length::Fill).into()
}
