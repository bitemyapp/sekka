//! The desktop's part of the shell: its icons from the system and the
//! desktop folder, its wallpaper, and what clicks, drags and drops on it
//! do.

use std::path::PathBuf;

use aizome_kit::prelude::*;
use aizome_kit::{Image, MenuStyle};

use crate::actions::{Action, DiskVerb};
use crate::desktop::{self, DeskIcon, DesktopMsg, IconKind};
use crate::menus::model::{Entry, Menu as ShellMenu};
use crate::shell::{Msg, Shell, at_point};
use crate::wallpaper::WallKey;
use crate::widgets::drawn::DragPhase;

impl Shell {
    pub fn desktop_icons(&self) -> Vec<DeskIcon> {
        let home = aizome_config::home_dir();
        let mut volumes =
            self.sys.disks.as_ref().map(|d| d.snapshot().desktop_volumes(&home.to_string_lossy())).unwrap_or_default();
        // The system's disk from the mounts, where UDisks didn't say.
        if !volumes.iter().any(|v| v.system)
            && let Some(v) = &self.system_disk_fallback
        {
            volumes.insert(0, v.clone());
        }
        let volumes = if volumes.is_empty() { self.volumes_fixture.clone() } else { volumes };
        let cfg = &self.desktop_cfg;
        // The home folder, disks and Trash as `[desktop]` says.
        desktop::icons(&self.user.0, &volumes, self.trash_full, &self.desktop.items)
            .into_iter()
            .filter(|i| match i.kind {
                IconKind::Home(_) => cfg.show_home,
                IconKind::Volume { .. } => cfg.show_disks,
                IconKind::Trash { .. } => cfg.show_trash,
                IconKind::Item { .. } => true,
            })
            .collect()
    }

    /// Which wallpapers there are, Night's and Day's (the settings', else
    /// `$AIZOME_WALLPAPER_NIGHT` and `_DAY`).
    pub fn refresh_wall_paths(&mut self) {
        let paths = [desktop::wallpaper_path(&self.settings, false), desktop::wallpaper_path(&self.settings, true)];
        if paths != self.wall_paths {
            self.wall_failed.clear();
        }
        self.wall_paths = paths;
    }

    /// The wallpaper output `output` wants now: the theme's, at its size
    /// in pixels.
    fn wall_key(&self, screen: &crate::shell::Screen) -> Option<WallKey> {
        let path = self.wall_paths[self.theme_day as usize].clone()?;
        let o = &screen.output;
        let scale = o.scale.max(0.25);
        Some(WallKey {
            path,
            width: (o.frame.width() * scale).round() as u32,
            height: (o.frame.height() * scale).round() as u32,
        })
    }

    /// Output `output`'s wallpaper, if it's ready (until it is, the desktop
    /// shows its colour).
    pub fn wallpaper_for(&self, output: u32) -> Option<(Image, u64)> {
        let screen = self.screens.iter().find(|s| s.output.id == output)?;
        let key = self.wall_key(screen)?;
        let id = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut h);
            h.finish()
        };
        Some((self.wallpapers.get(&key)?.clone(), id))
    }

    /// Whether every output's wallpaper is ready.
    pub fn wallpapers_ready(&self) -> bool {
        self.screens
            .iter()
            .filter_map(|s| self.wall_key(s))
            .all(|k| self.wallpapers.contains_key(&k) || self.wall_failed.contains(&k.path))
    }

    /// Scale the wallpapers the outputs want, on threads, and let go of
    /// the ones nothing wants.
    pub fn prepare_wallpapers(&mut self, cx: &mut Cx<Msg>) {
        let wanted: Vec<(WallKey, f64)> =
            self.screens.iter().filter_map(|s| Some((self.wall_key(s)?, s.output.scale))).collect();
        self.wallpapers.retain(|k, _| wanted.iter().any(|(w, _)| w == k));
        for (key, scale) in wanted {
            if self.wallpapers.contains_key(&key)
                || self.wall_pending.contains(&key)
                || self.wall_failed.contains(&key.path)
            {
                continue;
            }
            self.wall_pending.insert(key.clone());
            cx.spawn(move || {
                let image = crate::wallpaper::prepare(&key.path, key.width, key.height)
                    .and_then(|rgba| crate::wallpaper::image(&rgba, key.width, key.height, scale));
                Msg::Wallpaper(key, image)
            });
        }
    }

    /// A wallpaper was scaled (or didn't read).
    pub fn wallpaper_ready(&mut self, key: WallKey, image: Option<Image>) {
        self.wall_pending.remove(&key);
        match image {
            Some(i) => {
                self.wallpapers.insert(key, i);
            }
            None => {
                log::warn!("the wallpaper {} doesn't read", key.path.display());
                self.wall_failed.insert(key.path);
            }
        }
    }

    /// Scale the outputs' wallpapers now, on this thread (tests).
    pub fn load_wallpapers(&mut self) {
        self.refresh_wall_paths();
        let wanted: Vec<(WallKey, f64)> =
            self.screens.iter().filter_map(|s| Some((self.wall_key(s)?, s.output.scale))).collect();
        for (key, scale) in wanted {
            if !self.wallpapers.contains_key(&key) {
                let image = crate::wallpaper::prepare(&key.path, key.width, key.height)
                    .and_then(|rgba| crate::wallpaper::image(&rgba, key.width, key.height, scale));
                self.wallpaper_ready(key, image);
            }
        }
    }

    /// How far in from the output's left the icons line up: past the
    /// Deskbar when it stands there (one that hides itself overlays them
    /// while it shows, as it does windows).
    pub fn desktop_left(&self, output: u32) -> f64 {
        let primary = self.primary().is_some_and(|s| s.output.id == output);
        if primary
            && self.deskbar.is_some()
            && !self.deskbar_cfg.auto_hide
            && self.deskbar_cfg.side == aizome_config::DeskbarSide::Left
        {
            self.deskbar_width() + crate::deskbar::SHADOW
        } else {
            0.0
        }
    }

    pub fn desktop_view(&self, output: u32) -> Element<Msg> {
        let Some(screen) = self.screens.iter().find(|s| s.output.id == output) else { return aizome_kit::space(0.0) };
        let size = screen.output.frame.size();
        let icons = self.desktop_icons();
        let left = self.desktop_left(output);
        desktop::view(&icons, &self.desktop, self.wallpaper_for(output), size, left)
            .map(move |m| Msg::Desktop(output, m))
    }

    fn icon(&self, key: &str) -> Option<DeskIcon> {
        self.desktop_icons().into_iter().find(|i| i.key == key)
    }

    pub fn desktop_msg(&mut self, output: u32, m: DesktopMsg, cx: &mut Cx<Msg>) {
        let height = self.screens.iter().find(|s| s.output.id == output).map_or(900.0, |s| s.output.frame.height());
        let left = self.desktop_left(output);
        match m {
            DesktopMsg::Select(key, add) => {
                if add {
                    if let Some(i) = self.desktop.selected.iter().position(|k| *k == key) {
                        self.desktop.selected.remove(i);
                    } else {
                        self.desktop.selected.push(key);
                    }
                } else {
                    self.desktop.selected = vec![key];
                }
            }
            DesktopMsg::Clear => self.desktop.selected.clear(),
            DesktopMsg::Open(key) => self.open_icon(&key, cx),
            DesktopMsg::IconMenu(key, at) => {
                let Some(icon) = self.icon(&key) else { return };
                let Some(w) = self.screens.iter().find(|s| s.output.id == output).and_then(|s| s.bar) else { return };
                let entries = self.icon_menu(&icon);
                let menu = ShellMenu::new(icon.label.clone(), entries);
                self.show_menu(w, at_point(at), &menu, MenuStyle::at_point(), cx);
            }
            DesktopMsg::IconDrag(key, phase) => {
                let icons = self.desktop_icons();
                let points = desktop::layout(&icons, &self.desktop.positions, height, left);
                let origin = icons.iter().zip(points).find(|(i, _)| i.key == key).map(|(_, p)| p).unwrap_or_default();
                match phase {
                    DragPhase::Start(p) => {
                        self.drag_grab = p - origin;
                        self.desktop.dragging = Some((key, origin));
                    }
                    DragPhase::Move(p) => self.desktop.dragging = Some((key, p - self.drag_grab)),
                    DragPhase::End(p) => {
                        self.desktop.dragging = None;
                        let target =
                            desktop::icon_at(&icons, &self.desktop.positions, height, left, p).filter(|t| *t != key);
                        let dragged = icons.iter().find(|i| i.key == key).cloned();
                        match (target.and_then(|t| icons.iter().find(|i| i.key == t).cloned()), dragged) {
                            (Some(t), Some(DeskIcon { kind: IconKind::Item { path, .. }, .. }))
                                if matches!(t.kind, IconKind::Trash { .. }) =>
                            {
                                self.run(Action::MoveToTrash { path }, cx);
                                self.refresh_desktop_items(cx);
                            }
                            _ => {
                                let to = p - self.drag_grab;
                                let snapped = Point::new((to.x / 4.0).round() * 4.0, (to.y / 4.0).round() * 4.0);
                                self.desktop.positions.insert(key, snapped);
                                if self.opts.live {
                                    self.desktop.save_positions(&desktop::DesktopState::path());
                                }
                            }
                        }
                    }
                }
            }
            DesktopMsg::Band(phase) => match phase {
                DragPhase::Start(p) => self.desktop.band = Some((p, p)),
                DragPhase::Move(p) => {
                    if let Some((a, _)) = self.desktop.band {
                        self.desktop.band = Some((a, p));
                        let icons = self.desktop_icons();
                        self.desktop.selected = desktop::in_band(&icons, &self.desktop.positions, height, left, a, p);
                    }
                }
                DragPhase::End(_) => self.desktop.band = None,
            },
            DesktopMsg::Menu(at) => {
                let origin = self
                    .screens
                    .iter()
                    .find(|s| s.output.id == output)
                    .map(|s| s.output.frame.origin())
                    .unwrap_or_default();
                self.pointer_menu_soon(origin.x + at.x, origin.y + at.y, cx);
            }
            DesktopMsg::DropOn(key, urls) => {
                let Some(icon) = self.icon(&key) else { return };
                let paths: Vec<PathBuf> = urls
                    .iter()
                    .filter_map(|u| u.strip_prefix("file://").map(|p| PathBuf::from(crate::shelf::percent_decode(p))))
                    .collect();
                if let IconKind::Trash { .. } = icon.kind {
                    for p in paths {
                        self.run(Action::MoveToTrash { path: p }, cx);
                    }
                    self.refresh_desktop_items(cx);
                }
            }
        }
    }

    /// What an icon's menu offers.
    pub fn icon_menu(&self, icon: &DeskIcon) -> Vec<Entry> {
        match &icon.kind {
            IconKind::Home(p) => vec![Entry::action("Open", Action::Open { path: p.clone() })],
            IconKind::Volume { block, mount, drive, system: true, .. } => {
                // The system's disk: opened and looked into, never
                // unmounted or ejected.
                let at = PathBuf::from(mount.clone().unwrap_or_else(|| "/".into()));
                vec![
                    Entry::action("Open", Action::Open { path: PathBuf::from("/") }),
                    Entry::separator(),
                    Entry::action(
                        "Show in Disks",
                        Action::Disk {
                            device: drive.clone().unwrap_or_else(|| block.clone()),
                            verb: DiskVerb::OpenInTansu,
                        },
                    ),
                    Entry::action(
                        "Disk Usage",
                        Action::Spawn { argv: vec!["aizome-kura".into(), at.to_string_lossy().into_owned()] },
                    ),
                ]
            }
            IconKind::Volume { block, mount, ejectable, drive, .. } => {
                let mut v = vec![
                    Entry::action("Open", Action::Disk { device: block.clone(), verb: DiskVerb::Mount }),
                    Entry::separator(),
                    Entry::action("Unmount", Action::Disk { device: block.clone(), verb: DiskVerb::Unmount })
                        .enabled(mount.is_some()),
                ];
                if *ejectable && let Some(d) = drive {
                    v.push(Entry::action("Eject", Action::Disk { device: d.clone(), verb: DiskVerb::Eject }));
                }
                v.push(Entry::separator());
                v.push(Entry::action(
                    "Show in Disks",
                    Action::Disk { device: block.clone(), verb: DiskVerb::OpenInTansu },
                ));
                v
            }
            IconKind::Trash { full } => vec![
                Entry::action("Open", Action::Open { path: crate::launch::trash_dir().join("files") }),
                Entry::separator(),
                Entry::action("Empty Trash", Action::EmptyTrash).enabled(*full),
            ],
            IconKind::Item { path, .. } => vec![
                Entry::action("Open", Action::Open { path: path.clone() }),
                Entry::action("Show in Hako", Action::Reveal { path: path.clone() }),
                Entry::action("Copy Path", Action::Copy { text: path.to_string_lossy().into_owned() }),
                Entry::separator(),
                Entry::action("Move to Trash", Action::MoveToTrash { path: path.clone() }),
            ],
        }
    }

    fn open_icon(&mut self, key: &str, cx: &mut Cx<Msg>) {
        let Some(icon) = self.icon(key) else { return };
        match icon.kind {
            IconKind::Home(p) => self.run(Action::Open { path: p }, cx),
            IconKind::Trash { .. } => self.run(Action::Open { path: crate::launch::trash_dir().join("files") }, cx),
            IconKind::Item { path, .. } => self.run(Action::Open { path }, cx),
            IconKind::Volume { system: true, .. } => self.run(Action::Open { path: PathBuf::from("/") }, cx),
            IconKind::Volume { block, mount: Some(m), .. } => {
                let _ = block;
                self.run(Action::Open { path: PathBuf::from(m) }, cx);
            }
            IconKind::Volume { block, mount: None, .. } => {
                self.run(Action::Disk { device: block, verb: DiskVerb::Mount }, cx)
            }
        }
    }

    /// Mount, unmount, eject, or open a disk in Tansu.
    pub fn disk(&mut self, device: String, verb: DiskVerb, cx: &mut Cx<Msg>) {
        match verb {
            // Tansu's arguments are images to attach; `--show=` picks the
            // disk holding a device (or a drive) instead.
            DiskVerb::OpenInTansu => {
                self.launch(crate::launch::Launch::Argv(vec!["aizome-tansu".into(), format!("--show={device}")]), cx)
            }
            DiskVerb::Mount => {
                let mounted = self
                    .sys
                    .disks
                    .as_ref()
                    .and_then(|d| d.snapshot().volumes().into_iter().find(|v| v.block == device))
                    .and_then(|v| v.mount_point);
                match mounted {
                    Some(m) => self.run(Action::Open { path: PathBuf::from(m) }, cx),
                    None => {
                        if let Some(d) = &self.sys.disks {
                            let task = d.mount(&device);
                            // Opened once it's mounted.
                            cx.spawn(move || match task.wait() {
                                Ok(point) => Msg::Run(Action::Open { path: PathBuf::from(point) }),
                                Err(e) => {
                                    log::warn!("mounting {device}: {e}");
                                    Msg::Nothing
                                }
                            });
                        }
                    }
                }
            }
            // Never the system's own disk, whoever asks.
            DiskVerb::Unmount | DiskVerb::Eject if self.is_system_disk(&device) => {
                log::warn!("not unmounting or ejecting the system's disk ({device})");
            }
            DiskVerb::Unmount => {
                if let Some(d) = &self.sys.disks {
                    let _ = d.unmount(&device, false);
                }
            }
            DiskVerb::Eject => {
                if let Some(d) = &self.sys.disks {
                    let _ = d.eject(&device);
                }
            }
        }
    }

    /// Whether a block device or drive is (or holds) the system's disk.
    fn is_system_disk(&self, device: &str) -> bool {
        self.desktop_icons().iter().any(|i| match &i.kind {
            IconKind::Volume { block, drive, system: true, .. } => block == device || drive.as_deref() == Some(device),
            _ => false,
        })
    }

    /// Read `~/Desktop` again (and whether the Trash is full), on a thread.
    pub fn refresh_desktop_items(&mut self, cx: &mut Cx<Msg>) {
        cx.spawn(|| {
            let items = desktop::read_items(&crate::launch::desktop_dir());
            Msg::DesktopItems(items, crate::launch::trash_full())
        });
    }

    /// Watch `~/Desktop` and the Trash (every two seconds, by their
    /// modification times).
    pub fn watch_desktop(&mut self) {
        let Some(tx) = self.sender() else { return };
        std::thread::Builder::new()
            .name("desktop-watch".into())
            .spawn(move || {
                let stamp = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
                let mut last = (None, None);
                loop {
                    let dir = crate::launch::desktop_dir();
                    let trash = crate::launch::trash_dir().join("files");
                    let now = (stamp(&dir), stamp(&trash));
                    if now != last {
                        last = now;
                        if !tx.send(Msg::DesktopItems(desktop::read_items(&dir), crate::launch::trash_full())) {
                            return;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            })
            .ok();
    }
}
