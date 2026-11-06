// SPDX-License-Identifier: GPL-3.0-or-later
//! One function per screen: the page's body, its buttons, and handles to the
//! parts that change while it is shown (progress, sizes), which are updated
//! in place rather than rebuilt so dragging and focus aren't interrupted.
use objc2::Message;
use objc2_app_kit::NSAccessibility;
use std::sync::mpsc::Sender;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBox, NSBoxType, NSButton, NSColor, NSProgressIndicator, NSProgressIndicatorStyle,
    NSStackView, NSTextField, NSView,
};
use sekka_mac_contract::{
    Acknowledgement, Candidate, HumanStep, NextAction, Stage,
};

use sekka_mac_appkit::ui::{self, Targets, Weight};
use sekka_mac_appkit::views::{DiskBar, DiskLayout, Mark, MarkView};
use crate::model::{CheckState, Model, Msg, Screen};
use crate::text;

/// Parts of a page that change while it is shown.
#[derive(Default)]
pub struct Live {
    pub checks: Vec<(Retained<MarkView>, Retained<NSTextField>)>,
    pub disk: Option<Retained<DiskBar>>,
    pub sekka_size: Option<Retained<NSTextField>>,
    pub macos_size: Option<Retained<NSTextField>>,
    pub progress: Option<Retained<NSProgressIndicator>>,
    pub progress_text: Option<Retained<NSTextField>>,
    pub stage_title: Option<Retained<NSTextField>>,
    pub stage_detail: Option<Retained<NSTextField>>,
    pub stages: Vec<(Stage, Retained<MarkView>)>,
    pub install: Option<Retained<NSButton>>,
    pub cancel: Option<Retained<NSButton>>,
    pub log: Option<Retained<NSTextField>>,
}

pub struct Built {
    pub body: Retained<NSStackView>,
    /// Buttons on the left of the bar, then on the right (the one made with
    /// `primary` is the default button).
    pub left: Vec<Retained<NSButton>>,
    pub right: Vec<Retained<NSButton>>,
    pub live: Live,
}

struct Page<'a> {
    mtm: MainThreadMarker,
    tx: Sender<Msg>,
    targets: &'a mut Targets,
    views: Vec<Retained<NSView>>,
    left: Vec<Retained<NSButton>>,
    right: Vec<Retained<NSButton>>,
    live: Live,
}

impl<'a> Page<'a> {
    fn new(mtm: MainThreadMarker, tx: &Sender<Msg>, targets: &'a mut Targets) -> Self {
        Self {
            mtm,
            tx: tx.clone(),
            targets,
            views: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
            live: Live::default(),
        }
    }

    fn add(&mut self, v: &NSView) {
        self.views.push(v.retain());
    }

    fn title(&mut self, s: &str) {
        let t = ui::text(self.mtm, s, 26.0, Weight::Bold);
        t.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityStaticTextRole }));
        self.add(&t);
    }

    fn lede(&mut self, s: &str) {
        let t = ui::secondary(self.mtm, s, 14.0);
        self.add(&t);
    }

    fn body(&mut self, s: &str) -> Retained<NSTextField> {
        let t = ui::text(self.mtm, s, 13.0, Weight::Regular);
        self.add(&t);
        t
    }

    fn heading(&mut self, s: &str) {
        let t = ui::text(self.mtm, s, 15.0, Weight::Semibold);
        self.add(&t);
    }

    fn note(&mut self, s: &str) -> Retained<NSTextField> {
        let t = ui::secondary(self.mtm, s, 12.0);
        self.add(&t);
        t
    }

    fn gap(&mut self, height: f64) {
        let v = NSView::new(self.mtm);
        ui::size(&v, None, Some(height));
        self.add(&v);
    }

    fn divider(&mut self) {
        let b = NSBox::new(self.mtm);
        b.setBoxType(NSBoxType::Separator);
        self.add(&b);
    }

    /// A mark and words beside it.
    fn row(&mut self, mark: Mark, label: &str, detail: &str) -> (Retained<MarkView>, Retained<NSTextField>) {
        let m = MarkView::new(self.mtm, 18.0, mark);
        let l = ui::text(self.mtm, label, 13.0, Weight::Semibold);
        let d = ui::secondary(self.mtm, detail, 13.0);
        let words = ui::stack(self.mtm, true, 2.0, &[&l, &d]);
        let row = ui::stack(self.mtm, false, 10.0, &[&m, &words]);
        row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        row.setAccessibilityElement(true);
        ui::label(&row, &format!("{label}: {detail}"));
        self.add(&row);
        (m, d)
    }

    /// A numbered instruction, large enough to read from across a desk.
    fn numbered(&mut self, n: usize, s: &str) {
        let number = ui::text(self.mtm, &format!("{n}"), 15.0, Weight::Bold);
        number.setTextColor(Some(&ui::crest()));
        ui::size(&number, Some(22.0), None);
        let t = ui::text(self.mtm, s, 15.0, Weight::Regular);
        let row = ui::stack(self.mtm, false, 8.0, &[&number, &t]);
        row.setAlignment(objc2_app_kit::NSLayoutAttribute::FirstBaseline);
        self.add(&row);
    }

    fn callout(&mut self, mark: Mark, s: &str) {
        let m = MarkView::new(self.mtm, 16.0, mark);
        let t = ui::text(self.mtm, s, 13.0, Weight::Regular);
        let row = ui::stack(self.mtm, false, 8.0, &[&m, &t]);
        row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        self.add(&row);
    }

    fn send(&self, msg: impl Fn() -> Msg + 'static) -> impl FnMut() + 'static {
        let tx = self.tx.clone();
        move || {
            let _ = tx.send(msg());
        }
    }

    fn left(&mut self, title: &str, msg: impl Fn() -> Msg + 'static) -> Retained<NSButton> {
        let action = self.send(msg);
        let b = ui::button(self.mtm, self.targets, title, action);
        self.left.push(b.clone());
        b
    }

    fn right(&mut self, title: &str, msg: impl Fn() -> Msg + 'static) -> Retained<NSButton> {
        let action = self.send(msg);
        let b = ui::button(self.mtm, self.targets, title, action);
        self.right.push(b.clone());
        b
    }

    fn primary(&mut self, title: &str, msg: impl Fn() -> Msg + 'static) -> Retained<NSButton> {
        let action = self.send(msg);
        let b = ui::primary(self.mtm, self.targets, title, action);
        self.right.push(b.clone());
        b
    }

    fn quit(&mut self) {
        let mtm = self.mtm;
        let b = ui::button(mtm, self.targets, "Quit", move || {
            objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None);
        });
        self.left.push(b);
    }

    fn finish(self) -> Built {
        let refs: Vec<&NSView> = self.views.iter().map(|v| &**v).collect();
        let body = ui::stack(self.mtm, true, 14.0, &refs);
        Built {
            body,
            left: self.left,
            right: self.right,
            live: self.live,
        }
    }
}

/// What, besides the screen, changes a page's structure. Values that change
/// while a page is shown (the chosen size, progress) are left out and
/// updated in place. The plan's limits are in: the disk picture is built
/// with them, and they move when what is left to download is measured.
pub fn key(m: &Model) -> String {
    format!(
        "{:?}|{:?}|{}|{}|{}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{}",
        m.screen,
        m.notice,
        m.download_error.is_some(),
        m.downloading,
        m.plan.as_ref().and_then(|p| p.digest().ok()).map(|d| d.to_string()).unwrap_or_default(),
        m.plan_error.is_some(),
        m.state,
        m.checks().iter().map(|c| format!("{:?}", c.state)).collect::<String>(),
        m.details,
        m.release.is_some(),
        m.to_download.map(|n| n > 0),
        m.size_range(),
        m.staged.is_empty(),
    )
}

pub fn build(m: &Model, tx: &Sender<Msg>, mtm: MainThreadMarker, targets: &mut Targets) -> Built {
    let mut p = Page::new(mtm, tx, targets);
    if let Some(notice) = &m.notice {
        p.callout(Mark::Warning, notice);
        let dismiss = p.send(|| Msg::DismissNotice);
        let b = ui::button(mtm, p.targets, "Dismiss", dismiss);
        b.setControlSize(objc2_app_kit::NSControlSize::Small);
        p.add(&b);
        p.divider();
    }
    match m.screen {
        Screen::Checking => checking(m, &mut p),
        Screen::Unsupported => unsupported(m, &mut p),
        Screen::NotReady => not_ready(m, &mut p),
        Screen::NoRelease => no_release(m, &mut p),
        Screen::NeedsUpdate => needs_update(&mut p),
        Screen::Existing => existing(m, &mut p),
        Screen::Plan => plan(m, &mut p),
        Screen::Downloading => downloading(m, &mut p),
        Screen::Review => review(m, &mut p),
        Screen::Approval => approval(&mut p),
        Screen::Installing => installing(m, &mut p),
        Screen::Recovery => recovery(m, &mut p),
        Screen::Stopped => stopped(m, &mut p),
    }
    p.finish()
}

fn mark(state: CheckState) -> Mark {
    match state {
        CheckState::Pending => Mark::Empty,
        CheckState::Good => Mark::Good,
        CheckState::Warning => Mark::Warning,
        CheckState::Bad => Mark::Bad,
    }
}

fn checking(m: &Model, p: &mut Page) {
    p.title("Checking this Mac");
    p.lede("Nothing on your Mac changes while Sekka Installer looks at it.");
    p.gap(6.0);
    for row in m.checks() {
        let handles = p.row(mark(row.state), row.label, &row.detail);
        p.live.checks.push(handles);
    }
    p.quit();
}

fn unsupported(m: &Model, p: &mut Page) {
    p.title("Sekka doesn't support this Mac yet");
    for r in m.refusals() {
        p.body(&text::refusal(&r));
    }
    if let Some(i) = m.inspection() {
        p.gap(4.0);
        p.row(
            Mark::Bad,
            "This Mac",
            &format!("{} · {} · {} · {}", i.machine.product_name, i.machine.product_type, i.machine.device_class, i.machine.system_firmware),
        );
    }
    if let Some(v) = m.verified() {
        for profile in &v.catalog().profiles {
            p.row(
                Mark::Good,
                "Sekka runs on",
                &format!("{} ({}) with system firmware {}", profile.name, profile.machine.product_type, profile.firmware.system_firmware.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
            );
        }
    }
    p.note("Nothing on this Mac was changed.");
    p.quit();
    p.left("Copy Details", || Msg::CopyDiagnostics);
    p.primary("Check Again", || Msg::CheckAgain);
}

fn not_ready(m: &Model, p: &mut Page) {
    p.title("This Mac isn't ready yet");
    p.lede("Fix these, then check again. Nothing on your Mac was changed.");
    p.gap(4.0);
    match &m.inspection {
        Some(Err(e)) => {
            p.row(Mark::Bad, "Inspection", e);
        }
        Some(Ok(i)) => {
            for issue in i.blocking() {
                p.row(Mark::Bad, issue_label(issue.code), &text::issue(issue));
            }
            if i.blocking().next().is_none() {
                p.row(
                    Mark::Bad,
                    "Space",
                    "No part of the disk can give Sekka the room it needs. Free up space in macOS, then check again.",
                );
            }
        }
        None => {}
    }
    p.quit();
    p.left("Copy Details", || Msg::CopyDiagnostics);
    p.primary("Check Again", || Msg::CheckAgain);
}

fn issue_label(code: sekka_mac_contract::IssueCode) -> &'static str {
    use sekka_mac_contract::IssueCode::*;
    match code {
        OnBattery | BatteryLow => "Power",
        FileVaultConverting => "FileVault",
        NotEnoughSpace | LocalSnapshots | ResizeLimitsUnavailable => "Space",
        MacosTooOld | KnownMacosIssue => "macOS",
        NotNormalBoot => "Startup",
        UnsupportedMachine | UntestedFirmware => "This Mac",
        ExistingSekka | PartialInstallation | UnrecognizedLayout | AmbiguousDisk => "Disk",
    }
}

fn no_release(m: &Model, p: &mut Page) {
    p.title("There's nothing to install right now");
    match &m.catalog {
        Some(Err(e)) => {
            p.body(e);
        }
        _ => {
            p.body("Sekka's release list doesn't offer a release for this Mac yet.");
        }
    }
    p.note("Nothing on your Mac was changed.");
    p.quit();
    p.primary("Check Again", || Msg::CheckAgain);
}

fn needs_update(p: &mut Page) {
    p.title(text::NEEDS_UPDATE_TITLE);
    p.body(text::NEEDS_UPDATE);
    p.note("Nothing on your Mac was changed.");
    p.quit();
    p.left("Copy Details", || Msg::CopyDiagnostics);
    p.primary("Check Again", || Msg::CheckAgain);
}

fn existing(m: &Model, p: &mut Page) {
    if m.installed() {
        p.title("Sekka is installed");
        p.lede(text::INSTALLED);
        p.numbered(1, "Shut down your Mac.");
        p.numbered(2, "Press and hold the power button until “Loading startup options” appears.");
        p.numbered(3, "Choose Sekka. To return to macOS later, choose Macintosh HD the same way.");
    } else {
        p.title("Sekka is already on this Mac");
        p.body("Sekka Installer found a Sekka installation on this disk. It doesn't install a second one beside it, and this version doesn't remove or replace an installation.");
        if let Some(i) = m.inspection() {
            for x in &i.installations {
                p.row(
                    if x.state == sekka_mac_contract::InstallationState::Complete { Mark::Good } else { Mark::Warning },
                    x.name.as_str(),
                    match x.state {
                        sekka_mac_contract::InstallationState::Complete => "Installed",
                        sekka_mac_contract::InstallationState::WaitingForRecovery => "Waiting for its step in Recovery",
                        sekka_mac_contract::InstallationState::Partial => "Partly installed: some of its partitions are missing",
                        sekka_mac_contract::InstallationState::Unknown => "Its state couldn't be read",
                    },
                );
            }
        }
        p.note("To start Sekka, hold the power button when your Mac starts and choose Sekka.");
    }
    p.quit();
    p.left("Copy Details", || Msg::CopyDiagnostics);
}

fn disk_layout(m: &Model) -> Option<DiskLayout> {
    let i = m.inspection()?;
    let (min, max) = m.size_range()?;
    let candidate = m.candidate()?;
    match candidate {
        Candidate::ShrinkContainer { container, partition, current_size, adjacent_free, .. } => {
            let part = i.disk.partitions.iter().find(|x| &x.uuid == partition)?;
            let c = i.disk.containers.iter().find(|x| &x.uuid == container)?;
            Some(DiskLayout {
                total: i.disk.size,
                before: part.offset,
                macos: *current_size,
                macos_used: c.size.saturating_sub(c.free),
                adjacent_free: *adjacent_free,
                size: m.size,
                min,
                max,
            })
        }
        Candidate::FreeSpace { extent } => Some(DiskLayout {
            total: i.disk.size,
            before: 0,
            macos: extent.offset,
            macos_used: extent.offset,
            adjacent_free: extent.size,
            size: m.size,
            min,
            max,
        }),
    }
}

/// The plan screen's sizes, as words.
pub fn size_words(m: &Model) -> (String, String) {
    let sekka = format!("Sekka gets {}", text::bytes(m.size));
    let macos = match m.candidate() {
        Some(Candidate::ShrinkContainer { container, current_size, adjacent_free, .. }) => {
            let after = (current_size + adjacent_free).saturating_sub(m.size);
            let free = m
                .inspection()
                .and_then(|i| i.disk.containers.iter().find(|c| &c.uuid == container))
                .map(|c| (c.free + adjacent_free).saturating_sub(m.size).saturating_sub(m.to_download.unwrap_or(0)))
                .unwrap_or(0);
            format!("macOS keeps {} ({} free)", text::bytes(after), text::bytes(free))
        }
        _ => "macOS isn't resized: Sekka goes in free space".into(),
    };
    (sekka, macos)
}

fn plan(m: &Model, p: &mut Page) {
    p.title("Install Sekka beside macOS");
    if let Some(r) = &m.release {
        p.lede(&format!(
            "{}. Your files and apps in macOS stay as they are; macOS gives up some of its free space.",
            r.title
        ));
        if r.support == sekka_mac_contract::Support::Development {
            p.callout(Mark::Warning, "This is a development build. Sekka hasn't yet been qualified on a fresh installation of this Mac.");
        }
    }
    if let Some(notice) = m.verified().and_then(|v| v.catalog().notice.clone()) {
        p.note(notice.as_str());
    }
    p.gap(4.0);
    p.heading("Choose how much space Sekka gets");
    if let Some(layout) = disk_layout(m) {
        let tx = p.tx.clone();
        let bar = DiskBar::new(p.mtm, layout, move |size| {
            let _ = tx.send(Msg::SetSize(size));
        });
        p.add(&bar);
        p.live.disk = Some(bar);
    }
    let (sekka, macos) = size_words(m);
    let sekka_label = ui::text(p.mtm, &sekka, 15.0, Weight::Semibold);
    sekka_label.setTextColor(Some(&ui::crest()));
    let macos_label = ui::secondary(p.mtm, &macos, 13.0);
    let sizes = ui::stack(p.mtm, true, 2.0, &[&sekka_label, &macos_label]);
    p.add(&sizes);
    p.live.sekka_size = Some(sekka_label);
    p.live.macos_size = Some(macos_label);
    if let Some((min, max)) = m.size_range() {
        p.note(&format!(
            "Drag the divider, or select it and use the arrow keys. Sekka needs at least {}; this disk can give up to {}.",
            text::bytes(min),
            text::bytes(max)
        ));
    }
    if let Some(e) = &m.plan_error {
        p.callout(Mark::Bad, e);
    }
    p.divider();
    let apple: u64 = m.release.as_ref().map(|r| r.reserved()).unwrap_or(0);
    let own = m.to_download.unwrap_or(0);
    p.body(&text::to_download(own, !m.staged.is_empty()));
    if apple > 0 {
        p.note(&format!(
            "While installing, about {} more comes from Apple: the parts of macOS that Sekka's startup volume needs, downloaded from Apple over a secure connection. Stay connected to the internet until the installer finishes.",
            text::bytes(apple)
        ));
    }
    p.quit();
    if m.plan_error.is_some() {
        p.right("Check Again", || Msg::CheckAgain);
    }
    if own > 0 {
        p.primary("Download", || Msg::StartDownload);
    } else {
        p.primary("Continue", || Msg::Review);
    }
}

fn downloading(m: &Model, p: &mut Page) {
    p.title("Downloading Sekka");
    p.lede("Every byte is checked against the release before anything is installed. Nothing on your disk changes yet.");
    let bar = NSProgressIndicator::new(p.mtm);
    bar.setStyle(NSProgressIndicatorStyle::Bar);
    bar.setIndeterminate(false);
    bar.setMinValue(0.0);
    bar.setMaxValue(1.0);
    p.add(&bar);
    p.live.progress = Some(bar);
    let detail = p.note("Starting…");
    p.live.progress_text = Some(detail);
    if let Some(e) = &m.download_error {
        p.callout(Mark::Bad, e);
        p.left("Back", || Msg::StopDownload);
        p.primary("Try Again", || Msg::StartDownload);
    } else if m.downloading {
        p.right("Pause", || Msg::StopDownload);
    } else {
        p.note("Checking this Mac again…");
        p.quit();
    }
}

fn review(m: &Model, p: &mut Page) {
    p.title("Review the plan");
    p.lede("This is exactly what will change. Nothing has changed yet.");
    for (n, change) in m.changes().iter().enumerate() {
        p.numbered(n + 1, change);
    }
    p.note(text::NOT_ENCRYPTED);
    p.gap(2.0);
    p.heading("Then you finish in Recovery");
    let owner = m.user.0.clone();
    if let Some(plan) = &m.plan {
        let mut n = 1;
        for step in &plan.human_steps {
            if *step == HumanStep::CreateAccount {
                continue;
            }
            p.numbered(n, &text::human_step(*step, plan.volume_name.as_str(), &owner));
            n += 1;
        }
        p.note("Sekka Installer shows these steps again when it's done.");
        p.divider();
        if let Some(i) = m.inspection() {
            for a in &plan.acknowledgements {
                if *a == Acknowledgement::KnownMacosIssue {
                    let admission = m
                        .verified()
                        .and_then(|v| v.catalog().profile(&plan.release.profile).map(|pr| pr.admit(&i.machine_facts())));
                    for r in admission.map(|a| a.acknowledgements).unwrap_or_default() {
                        p.callout(Mark::Warning, &text::refusal(&r));
                    }
                }
            }
        }
        for a in plan.acknowledgements.clone() {
            let tx = p.tx.clone();
            let c = ui::checkbox(p.mtm, p.targets, text::acknowledgement(a), m.acknowledged.contains(&a), move |on| {
                let _ = tx.send(Msg::SetAcknowledged(a, on));
            });
            p.add(&c);
        }
        let tx = p.tx.clone();
        let backup = ui::checkbox(p.mtm, p.targets, text::BACKUP, m.backup, move |on| {
            let _ = tx.send(Msg::SetBackup(on));
        });
        p.add(&backup);
        p.gap(2.0);
        if let Ok(digest) = plan.digest() {
            let details = ui::selectable(
                p.mtm,
                &format!(
                    "Plan {} · {} on {} ({}) · release {}",
                    &digest.to_string()[..23],
                    text::bytes(plan.allocation.sekka),
                    plan.machine.product_type,
                    plan.machine.system_firmware,
                    plan.release.release
                ),
                11.0,
            );
            details.setTextColor(Some(&NSColor::secondaryLabelColor()));
            p.add(&details);
        }
    }
    p.left("Back", || Msg::BackToPlan);
    let install = p.primary("Install…", || Msg::Install);
    install.setEnabled(m.ready_to_install());
    p.live.install = Some(install);
}

fn approval(p: &mut Page) {
    p.title("Allow Sekka Installer to make changes");
    p.lede("Sekka Installer's helper makes the changes to your disk, and macOS asks you to allow it first.");
    p.numbered(1, "Open System Settings › General › Login Items & Extensions.");
    p.numbered(2, "Under Allow in the Background, turn on Sekka Installer.");
    p.numbered(3, "Come back here. Sekka Installer continues on its own.");
    let spinner = NSProgressIndicator::new(p.mtm);
    spinner.setStyle(objc2_app_kit::NSProgressIndicatorStyle::Spinning);
    spinner.setControlSize(objc2_app_kit::NSControlSize::Small);
    unsafe { spinner.startAnimation(None) };
    let waiting = ui::secondary(p.mtm, "Waiting for permission…", 12.0);
    let row = ui::stack(p.mtm, false, 8.0, &[&spinner, &waiting]);
    p.add(&row);
    p.left("Back", || Msg::BackToPlan);
    p.primary("Open Login Items", || Msg::OpenLoginItems);
}

fn installing(m: &Model, p: &mut Page) {
    p.title("Installing Sekka");
    p.lede("Keep your Mac open and plugged in. You can use other apps meanwhile; quitting Sekka Installer doesn't stop the installation.");
    let title = ui::text(p.mtm, "", 15.0, Weight::Semibold);
    let detail = ui::secondary(p.mtm, "", 13.0);
    p.add(&title);
    p.add(&detail);
    let bar = NSProgressIndicator::new(p.mtm);
    bar.setStyle(NSProgressIndicatorStyle::Bar);
    bar.setMinValue(0.0);
    bar.setMaxValue(1.0);
    p.add(&bar);
    p.live.stage_title = Some(title);
    p.live.stage_detail = Some(detail);
    p.live.progress = Some(bar);
    p.gap(2.0);
    let mut rows: Vec<Retained<NSView>> = Vec::new();
    for (stage, state) in m.stages() {
        let mk = MarkView::new(p.mtm, 14.0, mark(state));
        let l = ui::secondary(p.mtm, text::stage(stage), 12.0);
        let row = ui::stack(p.mtm, false, 8.0, &[&mk, &l]);
        rows.push(Retained::into_super(row));
        p.live.stages.push((stage, mk));
    }
    let refs: Vec<&NSView> = rows.iter().map(|v| &**v).collect();
    let list = ui::stack(p.mtm, true, 6.0, &refs);
    p.add(&list);
    if m.details {
        let log = ui::selectable(p.mtm, "", 11.0);
        p.add(&log);
        p.live.log = Some(log);
    }
    p.left(if m.details { "Hide Details" } else { "Show Details" }, || Msg::ToggleDetails);
    let cancel = p.right("Cancel", || Msg::CancelInstallation);
    cancel.setEnabled(m.cancel_allowed());
    cancel.setToolTip(Some(&ui::ns(
        "You can cancel until the disk starts to change. After that, stopping in the middle isn't safe.",
    )));
    p.live.cancel = Some(cancel);
}

fn recovery(m: &Model, p: &mut Page) {
    p.title("Finish in Recovery");
    p.lede("macOS's part is done and checked. Sekka isn't installed until you finish these steps; it takes about five minutes.");
    let owner = m.user.0.clone();
    let steps = m
        .plan
        .as_ref()
        .map(|plan| plan.human_steps.clone())
        .unwrap_or_else(|| {
            vec![
                HumanStep::ShutDown,
                HumanStep::HoldPowerButton,
                HumanStep::ChooseVolume,
                HumanStep::AuthorizeInRecovery,
                HumanStep::CreateAccount,
            ]
        });
    for (n, step) in steps.iter().enumerate() {
        p.numbered(n + 1, &text::human_step(*step, "Sekka", &owner));
    }
    p.gap(4.0);
    p.callout(Mark::Warning, "If your Mac starts up in macOS instead, open Sekka Installer again; it picks up where this left off.");
    p.note("Write these steps down or take a photo of them: this window isn't available in Recovery.");
    p.left("Copy Details", || Msg::CopyDiagnostics);
    p.primary("Shut Down…", || Msg::ShutDown);
}

fn stopped(m: &Model, p: &mut Page) {
    let Some(state) = &m.state else {
        p.title("The installation stopped");
        return;
    };
    let (headline, explanation) = text::stopped(state);
    p.title(&headline);
    p.body(&explanation);
    if let Some(f) = m.failure() {
        p.gap(2.0);
        p.row(Mark::Bad, "What happened", f.message.as_str());
        if let Some(apple) = &f.apple_diagnostic {
            p.note("macOS said:");
            p.add(&ui::selectable(p.mtm, apple.as_str(), 12.0));
        }
    }
    p.divider();
    p.heading("What you can do");
    let actions = m.stopped_actions();
    let cancel = actions.contains(&NextAction::Cancel);
    for a in &actions {
        match a {
            // Checking again finds this installation again until it is
            // cancelled.
            NextAction::Reinspect if cancel => {}
            NextAction::Cancel => {
                p.body(text::CANCEL_STOPPED);
            }
            NextAction::Reinspect => {
                p.body("Check this Mac again and review a new plan. If the same step fails again, copy the diagnostics and ask for help.");
            }
            NextAction::InspectActualState => {
                p.body("Don't start another installation. Copy the diagnostics and ask for help: the disk needs to be looked at as it is now, and Sekka Installer won't guess.");
            }
            NextAction::RetryStartupDisk => {
                p.body("Set the startup disk again. Only that step is repeated, after checking that everything else is still in place.");
            }
            NextAction::ReadPolicy => {
                p.body("Start up in Recovery again: hold the power button, choose Sekka, and Finish Installing Sekka picks up where it stopped.");
            }
            _ => {}
        }
    }
    p.left("Copy Diagnostics", || Msg::CopyDiagnostics);
    if actions.contains(&NextAction::RetryStartupDisk) {
        p.primary("Set Startup Disk Again…", || Msg::RetryStartupDisk);
    } else if cancel {
        p.primary("Cancel This Installation", || Msg::CancelInstallation);
    } else if actions.contains(&NextAction::Reinspect) {
        p.primary("Check This Mac Again", || Msg::CheckAgain);
    } else {
        p.quit();
    }
}
