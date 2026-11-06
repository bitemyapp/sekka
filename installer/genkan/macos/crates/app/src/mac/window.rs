// SPDX-License-Identifier: GPL-3.0-or-later
//! The window: the journey on the left, the page on the right, the buttons
//! along the bottom, and the authorization sheet. A timer on the main thread
//! drains worker results thirty times a second; nothing here waits on
//! anything slow.
use objc2_app_kit::NSAccessibility;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAlert, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSButtonCell, NSPanel,
    NSScrollView, NSSecureTextField, NSStackView, NSTextField, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectView, NSWindow, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSTimer};
use zeroize::Zeroize;

use super::pages::{self, Built, Live};
use sekka_mac_appkit::ui::{self, Targets, Weight};
use sekka_mac_appkit::views::{CrestView, FlippedView, Mark, MarkView};
use crate::env::Environment;
use crate::model::{Effect, Model, Msg, Screen, Step};
use crate::runtime::Runtime;
use crate::text;

const SIDEBAR: f64 = 236.0;

pub struct Ui {
    mtm: MainThreadMarker,
    pub model: Model,
    runtime: Runtime,
    rx: Receiver<Msg>,
    pub tx: Sender<Msg>,
    pub window: Retained<NSWindow>,
    steps: Vec<(Retained<MarkView>, Retained<NSTextField>)>,
    release: Retained<NSTextField>,
    document: Retained<FlippedView>,
    bar: Retained<NSStackView>,
    page: Option<(Retained<NSStackView>, Live, Targets)>,
    key: String,
    /// What the page's log shows, so it is replaced only when it changes
    /// and a selection in it lasts.
    log: String,
    sheet: Option<Sheet>,
    /// Download rate, from the last few progress reports.
    rate: Option<(Instant, u64, f64)>,
    last_tick: Instant,
    simulated: bool,
}

struct Sheet {
    panel: Retained<NSPanel>,
    owner: Retained<NSTextField>,
    password: Retained<NSSecureTextField>,
    error: Retained<NSTextField>,
    confirm: Retained<NSButton>,
    cancel: Retained<NSButton>,
    _targets: Targets,
}

impl Ui {
    /// Open the window and start the journey.
    pub fn open(mtm: MainThreadMarker, env: Arc<dyn Environment>, simulated: bool) -> Rc<RefCell<Self>> {
        let version = env!("CARGO_PKG_VERSION").parse().expect("the package version");
        let (model, effects) = Model::new(version, env.user());
        let ui = Self::new(mtm, env, simulated, model, effects);
        ui.borrow().window.makeKeyAndOrderFront(None);
        let pump = ui.clone();
        let block = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
            if let Ok(mut u) = pump.try_borrow_mut() {
                u.pump();
            }
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0 / 30.0, true, &block) };
        // The timer lives as long as the app.
        std::mem::forget(timer);
        ui
    }

    /// Build the window around a model, run its first effects and draw it,
    /// without showing it or starting the timer (screenshots use this).
    pub fn new(
        mtm: MainThreadMarker,
        env: Arc<dyn Environment>,
        simulated: bool,
        model: Model,
        effects: Vec<Effect>,
    ) -> Rc<RefCell<Self>> {
        let (tx, rx) = channel();
        let runtime = Runtime::new(env, tx.clone());

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(860.0, 600.0)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&ui::ns(text::APP_NAME));
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setContentMinSize(NSSize::new(760.0, 540.0));
        window.center();
        window.setFrameAutosaveName(&ui::ns("SekkaInstallerWindow"));

        let content = window.contentView().expect("a content view");

        // The journey.
        let sidebar = NSVisualEffectView::new(mtm);
        sidebar.setMaterial(NSVisualEffectMaterial::Sidebar);
        sidebar.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        content.addSubview(&sidebar);
        sidebar.setTranslatesAutoresizingMaskIntoConstraints(false);
        sidebar.topAnchor().constraintEqualToAnchor_constant(&content.topAnchor(), 0.0).setActive(true);
        sidebar.bottomAnchor().constraintEqualToAnchor_constant(&content.bottomAnchor(), 0.0).setActive(true);
        sidebar.leadingAnchor().constraintEqualToAnchor_constant(&content.leadingAnchor(), 0.0).setActive(true);
        sidebar.widthAnchor().constraintEqualToConstant(SIDEBAR).setActive(true);

        let crest = CrestView::new(mtm, 40.0);
        let name = ui::text(mtm, text::APP_NAME, 15.0, Weight::Semibold);
        let tagline = ui::secondary(mtm, "Sekka beside macOS", 12.0);
        let head = ui::stack(mtm, true, 3.0, &[&name, &tagline]);
        let brand = ui::stack(mtm, false, 10.0, &[&crest, &head]);
        let mut rows: Vec<Retained<NSView>> = Vec::new();
        let mut steps = Vec::new();
        for (n, (title, subtitle)) in text::JOURNEY.iter().enumerate() {
            let mark = MarkView::new(mtm, 22.0, Mark::Upcoming(n as u8 + 1));
            let number = ui::text(mtm, &format!("{}", n + 1), 11.0, Weight::Semibold);
            number.setAlignment(objc2_app_kit::NSTextAlignment::Center);
            mark.addSubview(&number);
            ui::pin(&number, &mark, 3.0, 0.0, 3.0, 0.0);
            let t = ui::text(mtm, title, 13.0, Weight::Medium);
            let s = ui::secondary(mtm, subtitle, 11.0);
            let words = ui::stack(mtm, true, 1.0, &[&t, &s]);
            let row = ui::stack(mtm, false, 10.0, &[&mark, &words]);
            row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
            row.setAccessibilityElement(true);
            ui::label(&row, &format!("Step {} of 6: {title}", n + 1));
            rows.push(Retained::into_super(row));
            steps.push((mark, t));
            let _ = number;
        }
        let refs: Vec<&NSView> = rows.iter().map(|v| &**v).collect();
        let journey = ui::stack(mtm, true, 16.0, &refs);
        let release = ui::secondary(mtm, "", 11.0);
        let column = ui::stack(mtm, true, 28.0, &[&brand, &journey]);
        sidebar.addSubview(&column);
        column.setTranslatesAutoresizingMaskIntoConstraints(false);
        column.topAnchor().constraintEqualToAnchor_constant(&sidebar.topAnchor(), 56.0).setActive(true);
        column.leadingAnchor().constraintEqualToAnchor_constant(&sidebar.leadingAnchor(), 20.0).setActive(true);
        sidebar.trailingAnchor().constraintEqualToAnchor_constant(&column.trailingAnchor(), 16.0).setActive(true);
        sidebar.addSubview(&release);
        release.setTranslatesAutoresizingMaskIntoConstraints(false);
        release.leadingAnchor().constraintEqualToAnchor_constant(&sidebar.leadingAnchor(), 20.0).setActive(true);
        sidebar.trailingAnchor().constraintEqualToAnchor_constant(&release.trailingAnchor(), 16.0).setActive(true);
        sidebar.bottomAnchor().constraintEqualToAnchor_constant(&release.bottomAnchor(), 18.0).setActive(true);

        // The page, in a scroll view for small windows and large text.
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        content.addSubview(&scroll);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        let document = FlippedView::new(mtm);
        scroll.setDocumentView(Some(&document));
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        let clip = scroll.contentView();
        document.topAnchor().constraintEqualToAnchor_constant(&clip.topAnchor(), 0.0).setActive(true);
        document.leadingAnchor().constraintEqualToAnchor_constant(&clip.leadingAnchor(), 0.0).setActive(true);
        document.widthAnchor().constraintEqualToAnchor_constant(&clip.widthAnchor(), 0.0).setActive(true);

        // The buttons, under a hairline.
        let line = NSBox::new(mtm);
        line.setBoxType(NSBoxType::Separator);
        content.addSubview(&line);
        let bar = ui::stack(mtm, false, 10.0, &[]);
        ui::insets(&bar, 14.0, 28.0, 14.0, 28.0);
        content.addSubview(&bar);

        scroll.topAnchor().constraintEqualToAnchor_constant(&content.topAnchor(), 0.0).setActive(true);
        scroll.leadingAnchor().constraintEqualToAnchor_constant(&sidebar.trailingAnchor(), 0.0).setActive(true);
        scroll.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), 0.0).setActive(true);
        line.setTranslatesAutoresizingMaskIntoConstraints(false);
        line.topAnchor().constraintEqualToAnchor_constant(&scroll.bottomAnchor(), 0.0).setActive(true);
        line.leadingAnchor().constraintEqualToAnchor_constant(&sidebar.trailingAnchor(), 0.0).setActive(true);
        line.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), 0.0).setActive(true);
        bar.topAnchor().constraintEqualToAnchor_constant(&line.bottomAnchor(), 0.0).setActive(true);
        bar.leadingAnchor().constraintEqualToAnchor_constant(&sidebar.trailingAnchor(), 0.0).setActive(true);
        bar.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), 0.0).setActive(true);
        bar.bottomAnchor().constraintEqualToAnchor_constant(&content.bottomAnchor(), 0.0).setActive(true);

        let ui = Rc::new(RefCell::new(Self {
            mtm,
            model,
            runtime,
            rx,
            tx,
            window,
            steps,
            release,
            document,
            bar,
            page: None,
            key: String::new(),
            log: String::new(),
            sheet: None,
            rate: None,
            last_tick: Instant::now(),
            simulated,
        }));
        {
            let mut u = ui.borrow_mut();
            for effect in effects {
                u.run(effect);
            }
            u.render();
        }
        ui
    }

    /// The authorization sheet's window, while it is open.
    #[cfg(feature = "simulation")]
    pub fn sheet_window(&self) -> Option<Retained<NSPanel>> {
        self.sheet.as_ref().map(|s| s.panel.clone())
    }

    /// Scroll the page to its end, as someone reading down it would.
    #[cfg(feature = "simulation")]
    pub fn scroll_to_end(&self) {
        if let Some(content) = self.window.contentView() {
            content.layoutSubtreeIfNeeded();
        }
        let end = self.document.bounds().size.height;
        self.document.scrollRectToVisible(NSRect::new(NSPoint::new(0.0, end - 1.0), NSSize::new(1.0, 1.0)));
    }

    fn run(&mut self, effect: Effect) {
        if let Some(main) = self.runtime.run(effect) {
            match main {
                Effect::OpenLoginItems => self.runtime.env().open_login_items(),
                Effect::CopyToPasteboard(text) => self.runtime.env().copy_to_pasteboard(&text),
                Effect::ShutDown => {
                    if let Err(e) = self.runtime.env().shut_down() {
                        let alert = NSAlert::new(self.mtm);
                        alert.setMessageText(&ui::ns("Shut down from the Apple menu"));
                        alert.setInformativeText(&ui::ns(&format!(
                            "Choose Apple menu › Shut Down, then follow the steps. ({e})"
                        )));
                        alert.beginSheetModalForWindow_completionHandler(&self.window, None);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn send(&mut self, msg: Msg) {
        for effect in self.model.update(msg) {
            self.run(effect);
        }
    }

    /// Drain what workers reported, then draw.
    fn pump(&mut self) {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            if let Msg::DownloadProgress(p) = &msg {
                let now = Instant::now();
                self.rate = match self.rate {
                    Some((t, done, rate)) if now.duration_since(t).as_secs_f64() > 0.5 => {
                        let sample = (p.done.saturating_sub(done)) as f64 / now.duration_since(t).as_secs_f64();
                        Some((now, p.done, if rate > 0.0 { rate * 0.8 + sample * 0.2 } else { sample }))
                    }
                    None => Some((now, p.done, 0.0)),
                    other => other,
                };
            }
            self.send(msg);
            changed = true;
        }
        if self.last_tick.elapsed().as_secs_f64() >= 1.0 {
            self.last_tick = Instant::now();
            self.send(Msg::Tick);
        }
        if changed {
            self.render();
        }
    }

    pub fn render(&mut self) {
        let mtm = self.mtm;
        // The journey.
        let step = self.model.step();
        let order = [Step::Check, Step::Size, Step::Download, Step::Install, Step::Recovery, Step::Account];
        for (n, (mark, title)) in self.steps.iter().enumerate() {
            let s = order[n];
            let m = if s < step {
                Mark::Done
            } else if s == step {
                Mark::Current(n as u8 + 1)
            } else {
                Mark::Upcoming(n as u8 + 1)
            };
            mark.set_mark(m);
            for sub in mark.subviews().iter() {
                sub.setHidden(matches!(m, Mark::Done));
                if let Ok(label) = sub.downcast::<NSTextField>() {
                    let color = if matches!(m, Mark::Current(_)) {
                        ui::on_fill()
                    } else {
                        objc2_app_kit::NSColor::secondaryLabelColor()
                    };
                    label.setTextColor(Some(&color));
                }
            }
            title.setFont(Some(&ui::font(13.0, if s == step { Weight::Semibold } else { Weight::Medium })));
        }
        let release = match (&self.model.release, self.simulated) {
            (_, true) => "Simulation: nothing on this Mac is changed".to_string(),
            (Some(r), false) => format!("{}\n{}", r.title, r.source.revision.short()),
            (None, false) => format!("{} {}", text::APP_NAME, self.model.app_version),
        };
        self.release.setStringValue(&ui::ns(&release));

        // The page: rebuilt when its structure changes, updated otherwise.
        let key = pages::key(&self.model);
        if key != self.key {
            self.key = key;
            let mut targets = Targets::default();
            let Built { body, left, right, live } = pages::build(&self.model, &self.tx, mtm, &mut targets);
            for v in self.document.subviews().iter() {
                v.removeFromSuperview();
            }
            self.document.addSubview(&body);
            ui::pin(&body, &self.document, 52.0, 36.0, 28.0, 36.0);
            // Words, rows and bars take the page's width, so they wrap and
            // stretch with it; a button (the notice's Dismiss) keeps its own.
            for v in body.arrangedSubviews().iter() {
                if v.downcast_ref::<NSButton>().is_none() {
                    ui::fill_width(&v, &body, 0.0);
                }
            }
            for v in self.bar.arrangedSubviews().iter() {
                self.bar.removeArrangedSubview(&v);
                v.removeFromSuperview();
            }
            for b in &left {
                self.bar.addArrangedSubview(b);
            }
            let spacer = NSView::new(mtm);
            spacer.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
            self.bar.addArrangedSubview(&spacer);
            for b in &right {
                self.bar.addArrangedSubview(b);
            }
            // Return presses the page's primary button, the one `ui::primary`
            // gave Return, and nothing else: not Cancel or Pause, and not a
            // button of the page just removed.
            let primary = left.iter().chain(&right).find(|b| b.keyEquivalent().to_string() == "\r");
            let cell = primary.and_then(|b| b.cell()).and_then(|c| c.downcast::<NSButtonCell>().ok());
            self.window.setDefaultButtonCell(cell.as_deref());
            self.page = Some((body, live, targets));
            self.log.clear();
        }
        self.update_live();
        self.sync_sheet();
    }

    fn update_live(&mut self) {
        let Some((_, live, _)) = &self.page else { return };
        let m = &self.model;
        for ((mark, detail), row) in live.checks.iter().zip(m.checks()) {
            mark.set_mark(match row.state {
                crate::model::CheckState::Pending => Mark::Empty,
                crate::model::CheckState::Good => Mark::Good,
                crate::model::CheckState::Warning => Mark::Warning,
                crate::model::CheckState::Bad => Mark::Bad,
            });
            detail.setStringValue(&ui::ns(&row.detail));
        }
        if let (Some(sekka), Some(macos)) = (&live.sekka_size, &live.macos_size) {
            let (s, mac) = pages::size_words(m);
            sekka.setStringValue(&ui::ns(&s));
            macos.setStringValue(&ui::ns(&mac));
        }
        if let Some(install) = &live.install {
            install.setEnabled(m.ready_to_install());
        }
        if let Some(cancel) = &live.cancel {
            cancel.setEnabled(m.cancel_allowed());
        }
        match m.screen {
            Screen::Downloading => {
                if let (Some(bar), Some(detail), Some(p)) = (&live.progress, &live.progress_text, &m.download) {
                    bar.setDoubleValue(if p.total == 0 { 0.0 } else { p.done as f64 / p.total as f64 });
                    let mut words = format!("{} of {}", text::bytes(p.done), text::bytes(p.total));
                    if p.verifying {
                        words = "Checking every byte…".into();
                    } else if let Some((_, _, rate)) = self.rate
                        && let Some(left) = text::remaining(p.total - p.done, rate)
                    {
                        words.push_str(&format!(" — {left}"));
                    }
                    if let Some(r) = &p.retrying {
                        words.push_str(&format!(". Retrying: {r}"));
                    }
                    detail.setStringValue(&ui::ns(&words));
                }
            }
            Screen::Installing => {
                let progress = m.progress;
                if let (Some(title), Some(detail)) = (&live.stage_title, &live.stage_detail) {
                    let stage = progress.map(|p| p.stage);
                    title.setStringValue(&ui::ns(stage.map(text::stage).unwrap_or("Starting…")));
                    detail.setStringValue(&ui::ns(stage.map(text::stage_detail).unwrap_or("")));
                }
                if let Some(bar) = &live.progress {
                    match progress.filter(|p| p.total > 0) {
                        Some(p) => {
                            bar.setIndeterminate(false);
                            bar.setDoubleValue(p.done as f64 / p.total as f64);
                        }
                        None => {
                            bar.setIndeterminate(true);
                            unsafe { bar.startAnimation(None) };
                        }
                    }
                }
                for ((stage, mark), (s, state)) in live.stages.iter().zip(m.stages()) {
                    debug_assert_eq!(*stage, s);
                    mark.set_mark(match state {
                        crate::model::CheckState::Pending => Mark::Empty,
                        crate::model::CheckState::Good => Mark::Good,
                        crate::model::CheckState::Warning => Mark::Warning,
                        crate::model::CheckState::Bad => Mark::Bad,
                    });
                }
                if let Some(log) = &live.log {
                    let lines: Vec<&str> = m.log.iter().rev().take(200).rev().map(String::as_str).collect();
                    let text = lines.join("\n");
                    if text != self.log {
                        log.setStringValue(&ui::ns(&text));
                        self.log = text;
                    }
                }
            }
            _ => {}
        }
    }

    /// Open, update or close the authorization sheet to match the model.
    fn sync_sheet(&mut self) {
        let mtm = self.mtm;
        match (&self.model.authorization, &self.sheet) {
            (None, Some(sheet)) => {
                sheet.password.setStringValue(&ui::ns(""));
                self.window.endSheet(&sheet.panel);
                self.sheet = None;
            }
            (Some(auth), None) => {
                let sheet = build_sheet(mtm, &self.tx, auth.retry, &auth.owner);
                self.window.beginSheet_completionHandler(&sheet.panel, None);
                sheet.panel.makeFirstResponder(Some(&sheet.password));
                self.sheet = Some(sheet);
                self.sync_sheet();
            }
            (Some(auth), Some(sheet)) => {
                sheet.error.setStringValue(&ui::ns(auth.error.as_deref().unwrap_or("")));
                sheet.error.setHidden(auth.error.is_none());
                sheet.confirm.setEnabled(!auth.busy);
                sheet.cancel.setEnabled(!auth.busy);
                sheet.owner.setEnabled(!auth.busy);
                sheet.password.setEnabled(!auth.busy);
                if auth.error.is_some() && !auth.busy {
                    sheet.panel.makeFirstResponder(Some(&sheet.password));
                }
            }
            (None, None) => {}
        }
    }
}

fn build_sheet(mtm: MainThreadMarker, tx: &Sender<Msg>, retry: bool, owner: &str) -> Sheet {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(440.0, 260.0)),
        NSWindowStyleMask::Titled,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { panel.setReleasedWhenClosed(false) };
    let mut targets = Targets::default();
    let title = ui::text(
        mtm,
        if retry { "Set the startup disk again" } else { "Allow Sekka Installer to install" },
        15.0,
        Weight::Semibold,
    );
    let explanation = ui::secondary(
        mtm,
        "Enter the name and password of an administrator who owns this Mac. macOS needs them to make Sekka a startup disk. The password isn't stored.",
        12.0,
    );
    let owner_field = NSTextField::textFieldWithString(&ui::ns(owner), mtm);
    owner_field.setPlaceholderString(Some(&ui::ns("Account name")));
    ui::label(&owner_field, "Administrator account name");
    let password = NSSecureTextField::new(mtm);
    password.setPlaceholderString(Some(&ui::ns("Password")));
    ui::label(&password, "Password");
    let error = ui::text(mtm, "", 12.0, Weight::Regular);
    error.setTextColor(Some(&ui::danger()));
    error.setHidden(true);

    let tx_cancel = tx.clone();
    let cancel = ui::button(mtm, &mut targets, "Cancel", move || {
        let _ = tx_cancel.send(Msg::CancelAuthorization);
    });
    cancel.setKeyEquivalent(&ui::ns("\u{1b}"));
    let tx_ok = tx.clone();
    let owner_read = owner_field.clone();
    let password_read = password.clone();
    let confirm = ui::primary(mtm, &mut targets, if retry { "Set Startup Disk" } else { "Install" }, move || {
        let owner = owner_read.stringValue().to_string();
        // Copied into a buffer that is wiped when the helper's request is
        // done; the field is cleared at once.
        let mut typed = password_read.stringValue().to_string();
        let secret = crate::env::Secret::new(typed.as_bytes().to_vec());
        typed.zeroize();
        password_read.setStringValue(&ui::ns(""));
        let _ = tx_ok.send(Msg::Authorize { owner, password: secret });
    });
    let spacer = NSView::new(mtm);
    spacer.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let buttons = ui::stack(mtm, false, 10.0, &[&spacer, &cancel, &confirm]);
    let column = ui::stack(mtm, true, 10.0, &[&title, &explanation, &owner_field, &password, &error, &buttons]);
    let content = panel.contentView().expect("a content view");
    content.addSubview(&column);
    ui::pin(&column, &content, 20.0, 20.0, 20.0, 20.0);
    for v in column.arrangedSubviews().iter() {
        ui::fill_width(&v, &column, 0.0);
    }
    Sheet {
        panel,
        owner: owner_field,
        password,
        error,
        confirm,
        cancel,
        _targets: targets,
    }
}

/// Whether quitting now needs a word first: the installation carries on
/// without the app.
pub fn confirm_quit(ui: &Ui) -> bool {
    if !ui.model.running {
        return true;
    }
    let alert = NSAlert::new(ui.mtm);
    alert.setMessageText(&ui::ns("Quit while Sekka is installing?"));
    alert.setInformativeText(&ui::ns(
        "The installation goes on without Sekka Installer. Open it again to see how it went.",
    ));
    alert.addButtonWithTitle(&ui::ns("Quit"));
    alert.addButtonWithTitle(&ui::ns("Cancel"));
    alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
}
