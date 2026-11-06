// SPDX-License-Identifier: GPL-3.0-or-later
//! Sekka Installer's AppKit interface.
//!
//! `sekka-installer` starts the app. Development builds also accept
//! `--simulate SCENARIO` (the whole journey with nothing real behind it)
//! and `--screenshots DIR` (every screen of every scenario, in light and
//! dark, written as PNGs from real AppKit rendering). Production builds
//! refuse both.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSEventModifierFlags, NSMenu, NSMenuItem, NSWindow,
    NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};

pub mod live;
mod pages;
#[cfg(feature = "simulation")]
mod shots;
use sekka_mac_appkit::ui;
mod window;

use crate::env::Environment;
use crate::model::Msg;
use window::Ui;

enum Mode {
    Live,
    #[cfg(feature = "simulation")]
    Simulate(crate::sim::Scenario),
    #[cfg(feature = "simulation")]
    Screenshots(std::path::PathBuf),
}

fn mode() -> Result<Mode, String> {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    match args.as_slice() {
        [] => Ok(Mode::Live),
        #[cfg(feature = "simulation")]
        [flag, scenario] if flag == "--simulate" => crate::sim::Scenario::parse(scenario)
            .map(Mode::Simulate)
            .ok_or_else(|| {
                let names: Vec<&str> = crate::sim::Scenario::ALL.iter().map(|s| s.name()).collect();
                format!("--simulate takes one of: {}", names.join(", "))
            }),
        #[cfg(feature = "simulation")]
        [flag, dir] if flag == "--screenshots" => Ok(Mode::Screenshots(dir.into())),
        _ if cfg!(feature = "production") => Err("Sekka Installer takes no arguments.".into()),
        _ => Err("usage: sekka-installer [--simulate SCENARIO | --screenshots DIR]".into()),
    }
}

pub struct DelegateIvars {
    env: Arc<dyn Environment>,
    simulated: bool,
    ui: RefCell<Option<Rc<RefCell<Ui>>>>,
    _targets: RefCell<ui::Targets>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaInstallerDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _: &NSNotification) {
            let mtm = self.mtm();
            let ui = Ui::open(mtm, self.ivars().env.clone(), self.ivars().simulated);
            ui.borrow().window.setDelegate(Some(ProtocolObject::from_ref(self)));
            *self.ivars().ui.borrow_mut() = Some(ui.clone());
            menus(mtm, &mut self.ivars()._targets.borrow_mut(), &ui);
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _: &NSApplication) -> bool {
            true
        }

        /// Clicking the Dock icon brings the window back, minimized or not.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _: &NSApplication, visible: bool) -> bool {
            if !visible
                && let Some(ui) = self.ivars().ui.borrow().as_ref()
                && let Ok(u) = ui.try_borrow()
            {
                if u.window.isMiniaturized() {
                    u.window.deminiaturize(None);
                }
                u.window.makeKeyAndOrderFront(None);
            }
            // With a window showing, AppKit does what it always does.
            visible
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _: &NSApplication) -> NSApplicationTerminateReply {
            let ok = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .and_then(|u| u.try_borrow().ok().map(|u| window::confirm_quit(&u)))
                .unwrap_or(true);
            if ok {
                NSApplicationTerminateReply::TerminateNow
            } else {
                NSApplicationTerminateReply::TerminateCancel
            }
        }
    }

    unsafe impl NSWindowDelegate for Delegate {
        /// The app has one window, and closing it quits. While Sekka is
        /// installing, closing asks what quitting asks, with the window
        /// still there: choosing Cancel leaves it open, not an app with no
        /// window to come back to.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _: &NSWindow) -> bool {
            let running = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .and_then(|u| u.try_borrow().ok().map(|u| u.model.running))
                .unwrap_or(false);
            if running {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
            !running
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, env: Arc<dyn Environment>, simulated: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            env,
            simulated,
            ui: RefCell::new(None),
            _targets: RefCell::new(ui::Targets::default()),
        });
        unsafe { msg_send![super(this), init] }
    }
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

fn custom(
    mtm: MainThreadMarker,
    targets: &mut ui::Targets,
    title: &str,
    key: &str,
    action: impl FnMut() + 'static,
) -> Retained<NSMenuItem> {
    let target = targets.keep(ui::Target::new(mtm, action));
    let i = item(mtm, title, Some(sel!(fire:)), key);
    unsafe { i.setTarget(Some(&target)) };
    i
}

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for i in items {
        menu.addItem(i);
    }
    let holder = item(mtm, title, None, "");
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
    menu
}

fn menus(mtm: MainThreadMarker, targets: &mut ui::Targets, ui: &Rc<RefCell<Ui>>) {
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);
    let send = |msg: fn() -> Msg| {
        let tx = ui.borrow().tx.clone();
        move || {
            let _ = tx.send(msg());
        }
    };
    let hide_others = item(mtm, "Hide Others", Some(sel!(hideOtherApplications:)), "h");
    hide_others.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    submenu(
        mtm,
        &bar,
        "Sekka Installer",
        &[
            item(mtm, "About Sekka Installer", Some(sel!(orderFrontStandardAboutPanel:)), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Hide Sekka Installer", Some(sel!(hide:)), "h"),
            hide_others,
            item(mtm, "Show All", Some(sel!(unhideAllApplications:)), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Quit Sekka Installer", Some(sel!(terminate:)), "q"),
        ],
    );
    submenu(
        mtm,
        &bar,
        "Edit",
        &[
            item(mtm, "Undo", Some(sel!(undo:)), "z"),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Cut", Some(sel!(cut:)), "x"),
            item(mtm, "Copy", Some(sel!(copy:)), "c"),
            item(mtm, "Paste", Some(sel!(paste:)), "v"),
            item(mtm, "Select All", Some(sel!(selectAll:)), "a"),
        ],
    );
    submenu(
        mtm,
        &bar,
        "Installation",
        &[
            custom(mtm, targets, "Check This Mac Again", "r", send(|| Msg::CheckAgain)),
            custom(mtm, targets, "Show Details", "l", send(|| Msg::ToggleDetails)),
            NSMenuItem::separatorItem(mtm),
            custom(mtm, targets, "Copy Diagnostics", "", send(|| Msg::CopyDiagnostics)),
        ],
    );
    let window = submenu(
        mtm,
        &bar,
        "Window",
        &[
            item(mtm, "Minimize", Some(sel!(performMiniaturize:)), "m"),
            item(mtm, "Zoom", Some(sel!(performZoom:)), ""),
        ],
    );
    // AppKit lists the window here, with Bring All to Front.
    app.setWindowsMenu(Some(&window));
    submenu(
        mtm,
        &bar,
        "Help",
        &[custom(mtm, targets, "Sekka for Mac Help", "?", || {
            let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str("https://sekka.computer/mac"));
            if let Some(url) = url {
                objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
            }
        })],
    );
    app.setMainMenu(Some(&bar));
}

pub fn main() {
    let mtm = MainThreadMarker::new().expect("Sekka Installer starts on the main thread");
    let mode = match mode() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let (env, simulated): (Arc<dyn Environment>, bool) = match mode {
        Mode::Live => (Arc::new(live::Live::new()), false),
        #[cfg(feature = "simulation")]
        Mode::Simulate(s) => (Arc::new(crate::sim::Simulation::new(s, 1.0)), true),
        #[cfg(feature = "simulation")]
        Mode::Screenshots(dir) => {
            let code = match shots::run(mtm, &dir) {
                Ok(n) => {
                    println!("Wrote {n} screenshots to {}", dir.display());
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            };
            std::process::exit(code);
        }
    };
    let delegate = Delegate::new(mtm, env, simulated);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    drop(delegate);
}
