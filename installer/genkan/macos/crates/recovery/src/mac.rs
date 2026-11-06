// SPDX-License-Identifier: GPL-3.0-or-later
//! Finish Installing Sekka on macOS's Recovery: the system calls and the
//! window. Programmatic AppKit only (Recovery stubs lack SwiftUI), one
//! small window, the password field focused at launch.
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSButton, NSMenu, NSMenuItem, NSPanel, NSScrollView, NSSecureTextField, NSStackView,
    NSTextView, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};
use sekka_mac_appkit::ui::{self, Targets, Weight};
use sekka_mac_appkit::views::{CrestView, Mark, MarkView};
use sekka_mac_contract::Uuid;
use zeroize::Zeroize;

use crate::finish::{Environment, FinishError, Finisher, Phase, System};
use crate::install::{Install, volume_of};
use crate::pty::{self, AuthError, Program};

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", PATH)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn plist_string(xml: &str, key: &str) -> Option<String> {
    let at = xml.find(&format!("<key>{key}</key>"))?;
    let rest = &xml[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].to_string())
}

/// The real Mac, from inside its Recovery.
pub struct Mac;

impl System for Mac {
    fn volume(&self, path: &Path) -> Result<(Uuid, Uuid), String> {
        let info = run("/usr/sbin/diskutil", &["info", "-plist", &path.to_string_lossy()])?;
        let volume = plist_string(&info, "VolumeUUID").ok_or("the volume has no UUID")?;
        let group = plist_string(&info, "APFSVolumeGroupID").ok_or("the volume has no volume group")?;
        Ok((
            Uuid::parse_any(&volume).map_err(|e| e.to_string())?,
            Uuid::parse_any(&group).map_err(|e| e.to_string())?,
        ))
    }
    fn device(&self, path: &Path) -> Result<String, String> {
        let info = run("/usr/sbin/diskutil", &["info", "-plist", &path.to_string_lossy()])?;
        plist_string(&info, "DeviceIdentifier").ok_or_else(|| "the volume has no device".into())
    }
    fn policy(&self, group: Uuid) -> Result<String, String> {
        run("/usr/bin/bputil", &["-d", "-v", &group.to_upper()])
    }
    fn handoff(&self, esp: Uuid) -> Result<Vec<u8>, String> {
        let id = esp.to_upper();
        let mount = |info: &str| plist_string(info, "MountPoint").filter(|m| !m.is_empty());
        let mut info = run("/usr/sbin/diskutil", &["info", "-plist", &id])?;
        if mount(&info).is_none() {
            run("/usr/sbin/diskutil", &["mount", "readOnly", &id])?;
            info = run("/usr/sbin/diskutil", &["info", "-plist", &id])?;
        }
        let at = PathBuf::from(mount(&info).ok_or("Sekka's boot partition couldn't be mounted")?);
        let path = at.join(sekka_mac_contract::HANDOFF_PATH);
        let meta = fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() || meta.len() > sekka_mac_contract::limits::HANDOFF as u64 {
            return Err("Sekka's boot partition has no usable description of this installation".into());
        }
        fs::read(&path).map_err(|e| e.to_string())
    }
    fn authorized(&self, program: Program, args: &[&str], user: &str, password: &str, log: &mut dyn FnMut(&str)) -> Result<(), AuthError> {
        if cfg!(feature = "preview") {
            log(&format!("Preview: would run {} {}", program.path(), args.join(" ")));
            return Err(AuthError::Failed("This preview build never changes how a Mac starts up.".into()));
        }
        pty::authorized(program, args, user, password, log)
    }
    fn disable_autolaunch(&self, volume: &Path) -> Result<(), String> {
        run("/sbin/mount", &["-u", "-w", &volume.to_string_lossy()])?;
        let media = volume.join(".IAPhysicalMedia");
        if media.exists() {
            fs::rename(&media, volume.join("IAPhysicalMedia-disabled.plist")).map_err(|e| e.to_string())?;
        }
        let services = volume.join("System/Library/CoreServices");
        let disabled = services.join("SystemVersion-disabled.plist");
        if disabled.exists() {
            fs::rename(&disabled, services.join("SystemVersion.plist")).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn startup_device(&self) -> Result<String, String> {
        Ok(run("/usr/sbin/bless", &["--getBoot"])?.trim().to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Screen {
    Checking,
    Broken(String),
    WrongRecovery,
    NotOneTrue,
    Ready { error: Option<String> },
    Working(Phase),
    Done,
}

/// What the worker thread reports. The finisher travels with the result
/// back to the main thread, so only one step runs at a time.
enum Event {
    Checked(Result<Environment, String>, Box<Finisher<Mac>>),
    Phase(Phase),
    Log(String),
    Finished(Result<(), FinishError>, Box<Finisher<Mac>>),
}

struct State {
    screen: Screen,
    owner: (String, String),
    log: Vec<String>,
    finisher: Option<Finisher<Mac>>,
}

struct Window {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    column: Retained<NSStackView>,
    targets: Targets,
    password: Option<Retained<NSSecureTextField>>,
    state: Rc<RefCell<State>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    shown: Option<Screen>,
    details: Option<Retained<NSPanel>>,
    details_text: Option<Retained<NSTextView>>,
}

impl Window {
    /// Check the installation and where the app is running, off the main
    /// thread.
    fn check(&self, finisher: Finisher<Mac>) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut finisher = finisher;
            let result = finisher.check();
            let _ = tx.send(Event::Checked(result, Box::new(finisher)));
        });
    }

    fn pump(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            changed = true;
            let mut s = self.state.borrow_mut();
            match event {
                Event::Checked(result, finisher) => {
                    s.screen = match result {
                        Ok(_) if finisher.done() => Screen::Done,
                        Ok(Environment::Ready) => Screen::Ready { error: None },
                        Ok(Environment::WrongRecovery) => Screen::WrongRecovery,
                        Ok(Environment::NotOneTrue) => Screen::NotOneTrue,
                        Err(e) => Screen::Broken(e),
                    };
                    s.finisher = Some(*finisher);
                }
                Event::Phase(p) => s.screen = Screen::Working(p),
                Event::Log(line) => {
                    s.log.push(line);
                    if s.log.len() > 2000 {
                        s.log.remove(0);
                    }
                }
                Event::Finished(result, finisher) => {
                    s.screen = match result {
                        Ok(()) => Screen::Done,
                        Err(FinishError::Password) => Screen::Ready {
                            error: Some(format!("That password isn't right for {}. Enter it again.", s.owner.0)),
                        },
                        Err(FinishError::Failed(m)) => Screen::Ready {
                            error: Some(format!(
                                "{m} You can try again; only these steps are repeated. Choose Utilities › Show Details for what macOS said."
                            )),
                        },
                    };
                    s.finisher = Some(*finisher);
                }
            }
        }
        if changed || self.shown.is_none() {
            self.render();
        }
        if let Some(text) = &self.details_text {
            text.setString(&NSString::from_str(&self.state.borrow().log.join("")));
        }
    }

    fn render(&mut self) {
        let screen = self.state.borrow().screen.clone();
        if self.shown.as_ref() == Some(&screen) {
            return;
        }
        self.shown = Some(screen.clone());
        let mtm = self.mtm;
        for v in self.column.arrangedSubviews().iter() {
            self.column.removeArrangedSubview(&v);
            v.removeFromSuperview();
        }
        self.targets = Targets::default();
        self.password = None;
        let (short, full) = self.state.borrow().owner.clone();
        let add = |column: &NSStackView, v: &NSView| {
            column.addArrangedSubview(v);
            ui::fill_width(v, column, 0.0);
        };
        let title = |s: &str| ui::text(mtm, s, 24.0, Weight::Bold);
        let body = |s: &str| ui::text(mtm, s, 14.0, Weight::Regular);
        let state = self.state.clone();
        let tx = self.tx.clone();
        match &screen {
            Screen::Checking => {
                add(&self.column, &title("Finish installing Sekka"));
                add(&self.column, &ui::secondary(mtm, "Checking this installation…", 14.0));
            }
            Screen::Broken(message) => {
                add(&self.column, &title("This installation can't be finished here"));
                add(&self.column, &body(message));
                add(&self.column, &ui::secondary(mtm, "Nothing was changed. To start up in macOS, hold the power button while your Mac starts and choose Macintosh HD.", 13.0));
                let b = ui::primary(mtm, &mut self.targets, "Shut Down", shut_down);
                add(&self.column, &buttons(mtm, &[&b]));
            }
            Screen::WrongRecovery => {
                add(&self.column, &title("Start up from Sekka's Recovery"));
                add(&self.column, &body("Each system on a Mac has its own Recovery, and only Sekka's can allow Sekka to start. Shut down, then press and hold the power button until “Loading startup options” appears, and choose Sekka."));
                let b = ui::primary(mtm, &mut self.targets, "Shut Down", shut_down);
                add(&self.column, &buttons(mtm, &[&b]));
            }
            Screen::NotOneTrue => {
                add(&self.column, &title("Hold the power button a little longer"));
                add(&self.column, &body("This step needs the Recovery you reach by pressing and holding the power button in one continuous press, until “Loading startup options” appears. Tapping it and pressing again shows the same choices but isn't enough."));
                add(&self.column, &ui::secondary(mtm, "Shut down, wait ten seconds after the screen goes dark, then press and hold the power button and choose Sekka.", 13.0));
                let b = ui::primary(mtm, &mut self.targets, "Shut Down", shut_down);
                add(&self.column, &buttons(mtm, &[&b]));
            }
            Screen::Ready { error } => {
                add(&self.column, &title("Finish installing Sekka"));
                add(&self.column, &body(&format!(
                    "Enter the password for {full} ({short}) to allow Sekka to start up on this Mac. This changes the security setting for Sekka only; macOS keeps its own."
                )));
                let field = NSSecureTextField::new(mtm);
                field.setPlaceholderString(Some(&NSString::from_str("Password")));
                ui::label(&field, &format!("Password for {short}"));
                add(&self.column, &field);
                if let Some(e) = error {
                    let l = ui::text(mtm, e, 13.0, Weight::Regular);
                    l.setTextColor(Some(&ui::danger()));
                    add(&self.column, &l);
                }
                let read = field.clone();
                let install = ui::primary(mtm, &mut self.targets, "Install", move || {
                    let mut typed = read.stringValue().to_string();
                    read.setStringValue(&NSString::from_str(""));
                    if typed.is_empty() {
                        return;
                    }
                    let Some(finisher) = state.borrow_mut().finisher.take() else { return };
                    state.borrow_mut().screen = Screen::Working(Phase::Allowing);
                    let password = std::mem::take(&mut typed);
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let mut finisher = finisher;
                        let mut password = password;
                        let ptx = tx.clone();
                        let ltx = tx.clone();
                        let r = finisher.finish(
                            &password,
                            &mut |p| {
                                let _ = ptx.send(Event::Phase(p));
                            },
                            &mut |line| {
                                let _ = ltx.send(Event::Log(line.to_string()));
                            },
                        );
                        password.zeroize();
                        let _ = tx.send(Event::Finished(r, Box::new(finisher)));
                    });
                });
                add(&self.column, &buttons(mtm, &[&install]));
                self.window.makeFirstResponder(Some(&field));
                self.password = Some(field);
            }
            Screen::Working(phase) => {
                add(&self.column, &title("Finishing…"));
                add(&self.column, &ui::secondary(mtm, "Don't turn off your Mac. macOS may show progress of its own.", 14.0));
                for (p, label) in [
                    (Phase::Allowing, "Allowing Sekka's startup software"),
                    (Phase::Installing, "Installing the startup software"),
                    (Phase::Selecting, "Making Sekka the startup disk"),
                ] {
                    let mark = if (p as u8) < (*phase as u8) {
                        Mark::Good
                    } else if p == *phase {
                        Mark::Current(0)
                    } else {
                        Mark::Empty
                    };
                    let m = MarkView::new(mtm, 16.0, mark);
                    let l = ui::text(mtm, label, 14.0, if p == *phase { Weight::Semibold } else { Weight::Regular });
                    add(&self.column, &ui::stack(mtm, false, 10.0, &[&m, &l]));
                }
            }
            Screen::Done => {
                add(&self.column, &title("Sekka is ready"));
                add(&self.column, &body("Your Mac starts up in Sekka next, and Sekka asks you to create your account."));
                add(&self.column, &ui::secondary(mtm, "To choose between Sekka and macOS later, press and hold the power button while your Mac starts.", 13.0));
                let b = ui::primary(mtm, &mut self.targets, "Restart", || {
                    let _ = Command::new("/sbin/reboot").env_clear().spawn();
                });
                add(&self.column, &buttons(mtm, &[&b]));
            }
        }
    }

    fn show_details(&mut self) {
        let mtm = self.mtm;
        if self.details.is_none() {
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 360.0)),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            );
            unsafe { panel.setReleasedWhenClosed(false) };
            panel.setTitle(&NSString::from_str("Details"));
            let scroll = NSTextView::scrollableTextView(mtm);
            let text: Retained<NSTextView> = scroll
                .documentView()
                .and_then(|v| v.downcast().ok())
                .expect("a text view");
            text.setEditable(false);
            text.setFont(Some(&ui::digits(11.0)));
            panel.setContentView(Some(&scroll));
            let _: &NSScrollView = &scroll;
            self.details_text = Some(text);
            self.details = Some(panel);
        }
        if let Some(p) = &self.details {
            p.center();
            p.makeKeyAndOrderFront(None);
        }
    }
}

fn shut_down() {
    let _ = Command::new("/sbin/shutdown").args(["-h", "now"]).env_clear().spawn();
}

fn buttons(mtm: MainThreadMarker, buttons: &[&NSButton]) -> Retained<NSStackView> {
    let spacer = NSView::new(mtm);
    spacer.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let mut views: Vec<&NSView> = vec![&spacer];
    views.extend(buttons.iter().map(|b| -> &NSView { b }));
    ui::stack(mtm, false, 10.0, &views)
}

pub struct DelegateIvars {
    window: RefCell<Option<Rc<RefCell<Window>>>>,
    targets: RefCell<Targets>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaRecoveryDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _: &NSNotification) {
            let mtm = self.mtm();
            let w = open(mtm);
            menus(mtm, &mut self.ivars().targets.borrow_mut(), &w);
            *self.ivars().window.borrow_mut() = Some(w);
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _: &NSApplication) -> bool {
            true
        }
    }
);

fn open(mtm: MainThreadMarker) -> Rc<RefCell<Window>> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 440.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Finish Installing Sekka"));
    window.setTitlebarAppearsTransparent(true);
    window.center();
    let content = window.contentView().expect("a content view");
    let crest = CrestView::new(mtm, 72.0);
    let column = ui::stack(mtm, true, 14.0, &[]);
    let row = ui::stack(mtm, false, 28.0, &[&crest, &column]);
    row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
    content.addSubview(&row);
    ui::pin(&row, &content, 64.0, 44.0, 36.0, 44.0);

    let (tx, rx) = channel();
    // Find the installation from where the app is, before anything else.
    let bundle = PathBuf::from(objc2_foundation::NSBundle::mainBundle().bundlePath().to_string());
    let loaded = volume_of(&bundle).ok_or_else(|| "The app isn't on a volume.".to_string()).and_then(|v| Install::load(&v));
    let (screen, owner, finisher) = match loaded.and_then(|i| Finisher::open(Mac, i)) {
        Ok(f) => {
            let owner = (f.owner().0.to_string(), f.owner().1.to_string());
            (Screen::Checking, owner, Some(f))
        }
        Err(e) => (Screen::Broken(e), (String::new(), String::new()), None),
    };
    let state = Rc::new(RefCell::new(State {
        screen,
        owner,
        log: Vec::new(),
        finisher: None,
    }));
    let w = Rc::new(RefCell::new(Window {
        mtm,
        window,
        column,
        targets: Targets::default(),
        password: None,
        state,
        tx,
        rx,
        shown: None,
        details: None,
        details_text: None,
    }));
    if let Some(f) = finisher {
        w.borrow().check(f);
    }
    {
        let mut b = w.borrow_mut();
        b.render();
        b.window.makeKeyAndOrderFront(None);
    }
    let pump = w.clone();
    let block = RcBlock::new(move |_t: std::ptr::NonNull<NSTimer>| {
        if let Ok(mut w) = pump.try_borrow_mut() {
            w.pump();
        }
    });
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.05, true, &block) };
    std::mem::forget(timer);
    w
}

fn item(mtm: MainThreadMarker, title: &str, action: Option<objc2::runtime::Sel>, key: &str) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

fn menus(mtm: MainThreadMarker, targets: &mut Targets, w: &Rc<RefCell<Window>>) {
    let bar = NSMenu::new(mtm);
    let add = |title: &str, items: Vec<Retained<NSMenuItem>>| {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for i in &items {
            menu.addItem(i);
        }
        let holder = item(mtm, title, None, "");
        holder.setSubmenu(Some(&menu));
        bar.addItem(&holder);
    };
    add(
        "Finish Installing Sekka",
        vec![
            item(mtm, "About Finish Installing Sekka", Some(sel!(orderFrontStandardAboutPanel:)), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Quit Finish Installing Sekka", Some(sel!(terminate:)), "q"),
        ],
    );
    add(
        "Edit",
        vec![
            item(mtm, "Copy", Some(sel!(copy:)), "c"),
            item(mtm, "Paste", Some(sel!(paste:)), "v"),
            item(mtm, "Select All", Some(sel!(selectAll:)), "a"),
        ],
    );
    let details = w.clone();
    let target = targets.keep(ui::Target::new(mtm, move || {
        if let Ok(mut d) = details.try_borrow_mut() {
            d.show_details();
        }
    }));
    let show = item(mtm, "Show Details", Some(sel!(fire:)), "l");
    unsafe { show.setTarget(Some(&target)) };
    add("Utilities", vec![show]);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
}

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            window: RefCell::new(None),
            targets: RefCell::new(Targets::default()),
        });
        unsafe { msg_send![super(this), init] }
    }
}

pub fn main() {
    let mtm = MainThreadMarker::new().expect("the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
