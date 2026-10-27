//! Storage: a bar of what fills the disk (from Kura's index when there is
//! one, else used and free), and Kura and Yukimi to free some.

use std::path::{Path, PathBuf};

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use super::Msg as G;
use crate::form;
use crate::pane::{PaneCx, SettingEntry, ViewCx};
use crate::widgets::{Segment, bar};

/// One filesystem's use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volume {
    pub mount: String,
    pub total: u64,
    pub available: u64,
    /// What fills it, largest first: a name and bytes.
    pub parts: Vec<(String, u64)>,
    /// When Kura last looked, if its index was read.
    pub scanned: Option<String>,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

#[derive(Default)]
pub struct State {
    volumes: Option<Vec<Volume>>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Vec<Volume>),
    Kura,
    Yukimi,
}

pub fn entries() -> Vec<SettingEntry> {
    vec![
        SettingEntry::new("disk-usage", "Disk usage").under("Storage").keywords(&[
            "space",
            "full",
            "kura",
            "what fills",
        ]),
        SettingEntry::new("nix-store", "Nix store").under("Storage").keywords(&[
            "garbage",
            "clean up",
            "generations",
            "nix-collect-garbage",
        ]),
    ]
}

/// Size, available space and the mount point of the filesystem at `path`.
pub fn statvfs(path: &Path) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    // SAFETY: statvfs writes only into the struct given.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        let f = s.f_frsize as u64;
        Some((s.f_blocks as u64 * f, s.f_bavail as u64 * f))
    }
}

/// The device a path is on, to tell filesystems apart.
fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

/// What Kura's index says fills `root`'s filesystem: the Nix store and the
/// kinds of files, largest first.
/// The root Kura scanned, what fills it, and when it looked.
pub type KuraParts = (String, Vec<(String, u64)>, String);

pub fn kura_parts(index: &Path) -> Option<KuraParts> {
    let text = std::fs::read_to_string(index).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let root = v.get("root")?.as_str()?.to_owned();
    let mut parts: Vec<(String, u64)> = Vec::new();
    if let Some(n) = v.get("nix").and_then(|n| n.get("store_bytes")).and_then(|b| b.as_u64()) {
        parts.push(("Nix store".into(), n));
    }
    for k in v.get("kinds")?.as_array()? {
        let title = k.get("title").and_then(|t| t.as_str()).unwrap_or("Other").to_owned();
        let bytes = k.get("allocated_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
        parts.push((title, bytes));
    }
    parts.sort_by_key(|p| std::cmp::Reverse(p.1));
    let scanned = v.get("scanned_at").and_then(|s| s.as_str()).unwrap_or("").to_owned();
    Some((root, parts, scanned))
}

/// The filesystems of `/` and the home folder.
pub fn read() -> Vec<Volume> {
    let home = aizome_config::home_dir();
    let index = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("aizome/kura/index.json");
    let kura = kura_parts(&index);
    let mut out: Vec<Volume> = Vec::new();
    let mut seen = Vec::new();
    for (path, name) in [(PathBuf::from("/"), "System"), (home.clone(), "Home")] {
        let Some(dev) = device(&path) else { continue };
        if seen.contains(&dev) {
            continue;
        }
        seen.push(dev);
        let Some((total, available)) = statvfs(&path) else { continue };
        let mut vol = Volume { mount: name.into(), total, available, parts: Vec::new(), scanned: None };
        if let Some((root, parts, scanned)) = &kura
            && device(Path::new(root)) == Some(dev)
        {
            // Kura counts what it scanned; the rest of what's used is other
            // people's files, the system's, and what it couldn't read.
            let known: u64 = parts.iter().map(|p| p.1).sum();
            vol.parts = parts.iter().filter(|p| p.1 > 0).take(6).cloned().collect();
            let listed: u64 = vol.parts.iter().map(|p| p.1).sum();
            let rest = vol.used().saturating_sub(listed.min(known));
            if rest > 0 {
                vol.parts.push(("Other".into(), rest));
            }
            vol.scanned = Some(scanned.clone());
        }
        out.push(vol);
    }
    out
}

pub fn sample() -> Vec<Volume> {
    let g = 1_000_000_000u64;
    vec![Volume {
        mount: "System".into(),
        total: 1_000 * g,
        available: 412 * g,
        parts: vec![
            ("Nix store".into(), 148 * g),
            ("Video".into(), 121 * g),
            ("Code".into(), 64 * g),
            ("Images".into(), 38 * g),
            ("Documents".into(), 12 * g),
            ("Other".into(), 205 * g),
        ],
        scanned: Some("2026-10-16T21:07:20Z".into()),
    }]
}

/// "412 GB".
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e12 {
        let t = b / 1e12;
        if (t - t.round()).abs() < 0.005 { format!("{:.0} TB", t) } else { format!("{t:.2} TB") }
    } else if b >= 1e9 {
        format!("{:.0} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

const DYES: [Segment; 7] =
    [Segment::Ai, Segment::Hanada, Segment::Violet, Segment::Cyan, Segment::Green, Segment::Kohaku, Segment::Slate];

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if cx.sample() {
            self.volumes = Some(sample());
        } else {
            cx.spawn(|| G::Storage(Msg::Read(read())));
        }
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Read(v) => self.volumes = Some(v),
            Msg::Kura => cx.launch(&["aizome-kura"]),
            Msg::Yukimi => cx.launch(&["aizome-yukimi", "--page", "storage"]),
        }
    }

    fn volume(&self, v: &Volume) -> Element<Msg> {
        let head = row![
            text(v.mount.clone()).weight(650.0),
            spacer(),
            text(format!("{} free of {}", size(v.available), size(v.total))).mono().size(12.0).role(Role::Text2),
        ]
        .width(Fill);
        let mut segments: Vec<(f64, Segment)> = Vec::new();
        let mut legend = Vec::new();
        if v.parts.is_empty() {
            segments.push((v.used() as f64, Segment::Ai));
            legend.push((Segment::Ai, "Used".to_owned(), v.used()));
        } else {
            for (i, (name, bytes)) in v.parts.iter().enumerate() {
                let d = DYES[i.min(DYES.len() - 1)];
                segments.push((*bytes as f64, d));
                legend.push((d, name.clone(), *bytes));
            }
        }
        segments.push((v.available as f64, Segment::Free));
        let keys = crate::widgets::flow(
            legend
                .into_iter()
                .map(|(d, name, bytes)| {
                    row![
                        crate::widgets::dot(d),
                        text(name).size(11.0).role(Role::Text2),
                        text(size(bytes)).mono().size(11.0).role(Role::Text3)
                    ]
                    .spacing(5.0)
                    .align(Align::Center)
                    .into()
                })
                .collect(),
            14.0,
            6.0,
        );
        let mut col = column![head, bar(segments, v.total as f64, 14.0).width(Fill), keys].spacing(10.0).width(Fill);
        if let Some(s) = &v.scanned {
            col = col.push(
                text(format!("From Kura's last look, {}", s.get(..10).unwrap_or(s))).size(11.0).role(Role::Text3),
            );
        }
        container(col).padding([14.0, form::INSET]).width(Fill).into()
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let mut rows: Vec<form::Row<Msg>> = match &self.volumes {
            None => vec![form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET]))],
            Some(vs) => vs.iter().map(|v| form::whole(self.volume(v))).collect(),
        };
        if !rows.is_empty() {
            let first = rows.remove(0);
            rows.insert(0, first.entry("disk-usage"));
        }
        rows.push(
            form::row("See what fills it", button("Open Kura").icon(Icon::ChartPie).on_press(Msg::Kura))
                .help("Kura draws the disk as rings by size, finds caches and old downloads, and gathers what you choose for the Trash."),
        );
        form::page(vec![
            vcx.section("Disks", rows),
            vcx.section(
                "Software",
                vec![
                    form::row("Nix store", button("Clean Up in Yukimi").on_press(Msg::Yukimi))
                        .entry("nix-store")
                        .help("Old versions of the system and of your apps stay in the store until they're cleaned up. Yukimi does it safely, keeping what you can go back to."),
                ],
            ),
        ])
    }
}
