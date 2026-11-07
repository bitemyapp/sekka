// SPDX-License-Identifier: GPL-3.0-or-later
//! `--screenshots DIR`: every screen of every simulated scenario, in light
//! and dark, rendered by AppKit offscreen and written as PNGs. These are
//! real captures of the app's own views (docs/screenshots.md: honest
//! provenance), driven by the simulation, so no disk, helper or account is
//! involved. One thing differs from the screen: the sidebar's translucent
//! material, which blurs whatever is behind the window, is drawn as the
//! flat under-page background color.
use objc2_app_kit::NSAppearanceCustomization;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use block2::StackBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSBezierPath,
    NSBitmapImageFileType, NSColor, NSCompositingOperation, NSGraphicsContext,
    NSImageInterpolation, NSView, NSVisualEffectView,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize};
use sekka_mac_appkit::ui;
use sekka_mac_contract::{ArtifactRole, Progress, Stage};

use super::window::Ui;
use crate::env::{DownloadProgress, Environment};
use crate::model::{Model, Msg};
use crate::sim::{Scenario, Simulation, settle};

/// A window's content view as the window shows it. AppKit draws the views
/// over transparency; the window's own background is drawn by its frame,
/// and a material (the sidebar's) is composited by the window server from
/// what is behind the window, so neither is in that drawing. Under the
/// views goes the window's background color and, where a material is, the
/// under-page background color in its place: offscreen, there is nothing
/// behind the window to show through it. The crest's cut-outs show what is
/// under them, as they do on screen.
fn capture(view: &NSView, path: &Path) -> Result<(), String> {
    view.layoutSubtreeIfNeeded();
    let bounds = view.bounds();
    let bitmap = || view.bitmapImageRepForCachingDisplayInRect(bounds).ok_or("AppKit couldn't make a bitmap for the window");
    let views = bitmap()?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &views);
    let rep = bitmap()?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep).ok_or("AppKit couldn't draw into the bitmap")?;
    context.setImageInterpolation(NSImageInterpolation::None);
    let background = view.window().map(|w| w.backgroundColor()).unwrap_or_else(NSColor::windowBackgroundColor);
    let materials: Vec<NSRect> = view
        .subviews()
        .iter()
        .filter(|v| v.downcast_ref::<NSVisualEffectView>().is_some())
        .map(|v| v.frame())
        .collect();
    let draw = StackBlock::new(|| {
        background.setFill();
        NSBezierPath::fillRect(bounds);
        NSColor::underPageBackgroundColor().setFill();
        for r in &materials {
            NSBezierPath::fillRect(*r);
        }
        unsafe {
            views.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                bounds,
                NSRect::new(NSPoint::ZERO, views.size()),
                NSCompositingOperation::SourceOver,
                1.0,
                false,
                None,
            )
        };
    });
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    // Colors resolve for the window's appearance, as the views' did.
    view.effectiveAppearance().performAsCurrentDrawingAppearance(&draw);
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .ok_or("AppKit couldn't encode the PNG")?;
    fs::write(path, png.to_vec()).map_err(|e| format!("{}: {e}", path.display()))
}

/// States also captured at the window's smallest size: the review, whose
/// sentences must be read in full before they are ticked.
const SMALLEST: [(Scenario, &str); 1] = [(Scenario::Normal, "review-ready")];

/// The states worth looking at, per scenario: (name, how to get there).
fn states(scenario: Scenario) -> Vec<(&'static str, Vec<Msg>)> {
    let review = || vec![Msg::StartDownload];
    let install = || {
        vec![
            Msg::StartDownload,
            Msg::SetBackup(true),
            Msg::SetAcknowledged(sekka_mac_contract::Acknowledgement::KnownMacosIssue, true),
            Msg::SetAcknowledged(sekka_mac_contract::Acknowledgement::DevelopmentRelease, true),
            Msg::Install,
        ]
    };
    let authorize = || Msg::Authorize {
        owner: "chris".into(),
        password: crate::env::Secret::new(b"simulated".to_vec()),
    };
    // Installed until it stops.
    let stop = || {
        let mut m = install();
        m.push(authorize());
        m
    };
    match scenario {
        Scenario::Normal => vec![
            ("checking", vec![]),
            ("plan", vec![]),
            ("downloading", vec![Msg::StartDownload]),
            ("review", review()),
            ("review-ready", {
                let mut m = review();
                m.extend([
                    Msg::SetBackup(true),
                    Msg::SetAcknowledged(sekka_mac_contract::Acknowledgement::KnownMacosIssue, true),
                    Msg::SetAcknowledged(sekka_mac_contract::Acknowledgement::DevelopmentRelease, true),
                ]);
                m
            }),
            ("authorize", install()),
            ("installing", install()),
            ("recovery", {
                let mut m = install();
                m.push(Msg::Authorize {
                    owner: "chris".into(),
                    password: crate::env::Secret::new(b"simulated".to_vec()),
                });
                m
            }),
        ],
        Scenario::WrongPassword => vec![("wrong-password", {
            let mut m = install();
            m.push(Msg::Authorize {
                owner: "chris".into(),
                password: crate::env::Secret::new(b"wrong".to_vec()),
            });
            m
        })],
        Scenario::DownloadFails => vec![("download-failed", vec![Msg::StartDownload])],
        Scenario::NeedsApproval => vec![("approval", install())],
        Scenario::ResizeFails | Scenario::InstallFails | Scenario::StartupDiskFails => vec![("stopped", stop())],
        // What a retry of the startup disk came to.
        Scenario::StartupDiskRefused | Scenario::StartupDiskFailsAgain => vec![("stopped", stop()), ("retried", {
            let mut m = stop();
            m.push(Msg::RetryStartupDisk);
            m.push(authorize());
            m
        })],
        _ => vec![("settled", vec![])],
    }
}

pub fn run(mtm: MainThreadMarker, dir: &Path) -> Result<usize, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut written = 0;
    for scenario in Scenario::ALL {
        for (name, msgs) in states(scenario) {
            for (appearance, label) in [(unsafe { NSAppearanceNameAqua }, "light"), (unsafe { NSAppearanceNameDarkAqua }, "dark")] {
                let env = Arc::new(Simulation::new(scenario, 0.0));
                let (mut model, effects) = Model::new("0.1.0".parse().unwrap(), env.user());
                if name != "checking" {
                    settle(&env, &mut model, effects);
                }
                for msg in msgs.iter().map(clone_msg) {
                    let effects = model.update(msg);
                    if name == "installing" {
                        break;
                    }
                    settle(&env, &mut model, effects);
                }
                match name {
                    "downloading" => {
                        model.downloading = true;
                        model.update(Msg::DownloadProgress(DownloadProgress {
                            role: ArtifactRole::Payload,
                            done: 4_100_000_000,
                            total: 12_900_000_000,
                            retrying: None,
                            verifying: false,
                        }));
                    }
                    "installing" => {
                        let effects = model.update(Msg::Authorize {
                            owner: "chris".into(),
                            password: crate::env::Secret::new(b"simulated".to_vec()),
                        });
                        drop(effects);
                        model.update(Msg::Begun(Ok(sekka_mac_contract::Uuid::from_bytes([0x5e; 16]))));
                        model.progress = Some(Progress {
                            stage: Stage::Root,
                            done: 62,
                            total: 100,
                        });
                    }
                    _ => {}
                }
                let ui = Ui::new(mtm, env.clone() as Arc<dyn Environment>, true, model, Vec::new());
                let u = ui.borrow();
                // Every capture at the window's first size, whatever size
                // the app was last left at, and without saving one.
                u.window.setFrameAutosaveName(&ui::ns(""));
                u.window.setContentSize(NSSize::new(860.0, 600.0));
                let named = NSAppearance::appearanceNamed(appearance);
                u.window.setAppearance(named.as_deref());
                let content = u.window.contentView().ok_or("no content view")?;
                let file = dir.join(format!("{}-{name}-{label}.png", scenario.name()));
                capture(&content, &file)?;
                written += 1;
                if SMALLEST.contains(&(scenario, name)) {
                    // The smallest window still shows everything, wrapped
                    // rather than cut off: its top, and its end.
                    u.window.setContentSize(u.window.contentMinSize());
                    capture(&content, &dir.join(format!("{}-{name}-smallest-{label}.png", scenario.name())))?;
                    u.scroll_to_end();
                    capture(&content, &dir.join(format!("{}-{name}-smallest-end-{label}.png", scenario.name())))?;
                    written += 2;
                }
                if let Some(sheet) = u.sheet_window() {
                    sheet.setAppearance(named.as_deref());
                    if let Some(c) = sheet.contentView() {
                        capture(&c, &dir.join(format!("{}-{name}-sheet-{label}.png", scenario.name())))?;
                        written += 1;
                    }
                }
                u.window.close();
            }
        }
    }
    Ok(written)
}

fn clone_msg(m: &Msg) -> Msg {
    match m {
        Msg::StartDownload => Msg::StartDownload,
        Msg::SetBackup(b) => Msg::SetBackup(*b),
        Msg::SetAcknowledged(a, b) => Msg::SetAcknowledged(*a, *b),
        Msg::Install => Msg::Install,
        Msg::RetryStartupDisk => Msg::RetryStartupDisk,
        Msg::Authorize { owner, password } => Msg::Authorize {
            owner: owner.clone(),
            password: password.clone(),
        },
        _ => Msg::Tick,
    }
}
