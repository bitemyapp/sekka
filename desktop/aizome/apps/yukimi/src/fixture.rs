//! A sample NixOS system, for tests, golden images and `--sample`: a
//! Sekka flake on a computer called ganymede, five generations, a store of
//! a realistic size, a few packages chosen every way Yukimi knows, a check
//! for updates that found two newer sources, and a small package index.
//!
//! Nothing here touches the machine it runs on.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use yukimi_config::edit::ListedPackage;
use yukimi_config::lock::{Input, Locked};
use yukimi_config::packages::Choices;
use yukimi_store::db::PathInfo;
use yukimi_store::{Graph, Root, RootKind, StorePath};
use yukimi_system::catalog;
use yukimi_system::generations::Generation;
use yukimi_system::index::{Package, PackageIndex};
use yukimi_system::info::SystemInfo;
use yukimi_system::profile::Element;
use yukimi_system::setup::{Choice, ChoiceOption, ChoiceValue, Facts, Kind, Setup};
use yukimi_system::updates::Release;

use crate::checks::{self, UpdateCheck};
use crate::model::{Composition, Configured, Heavy, Model, Store, SystemPackage};

/// The sample's "now": Saturday 17 October 2026, 21:42 UTC.
pub const NOW: i64 = 1_792_273_320;
/// The hour the sample's greeting is for.
pub const HOUR: u32 = 21;

const DAY: i64 = 86_400;
const GB: u64 = 1_000_000_000;
const MB: u64 = 1_000_000;

/// A store path with a made-up hash for `n` and `name`.
pub fn store_path(n: usize, name: &str) -> String {
    const ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";
    let mut x = (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5851_f42d_4c95_7f2d;
    let hash: String = (0..32)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ALPHABET[(x % 32) as usize] as char
        })
        .collect();
    format!("/nix/store/{hash}-{name}")
}

/// (name-version, size) of what every generation has, and what changed
/// between them: generation 27 had `before`, 28 has `after`.
const COMMON: [(&str, u64); 22] = [
    ("glibc-2.42-30", 32 * MB),
    ("systemd-258.3", 96 * MB),
    ("mesa-26.1.4", 612 * MB),
    ("llvm-21.1.2-lib", 418 * MB),
    ("gtk4-4.20.2", 58 * MB),
    ("qtbase-6.10.1", 71 * MB),
    ("ghc-9.10.3", 1840 * MB),
    ("rustc-1.95.0", 812 * MB),
    ("python3-3.13.8", 112 * MB),
    ("git-2.51.1", 64 * MB),
    ("vim-9.1.1790", 38 * MB),
    ("wget-1.25.0", 4 * MB),
    ("ripgrep-15.1.0", 6 * MB),
    ("inkscape-1.4.2", 214 * MB),
    ("thunderbird-144.0", 312 * MB),
    ("steam-1.0.0.85", 9 * MB),
    ("pipewire-1.4.9", 22 * MB),
    ("noto-fonts-cjk-sans-2.004", 280 * MB),
    ("aizome-0.1.0", 46 * MB),
    ("sidestep-0.1.0", 31 * MB),
    ("nix-2.31.2", 41 * MB),
    ("foot-1.24.0", 3 * MB),
];
const BEFORE: [(&str, u64); 5] = [
    ("linux-7.2.8", 148 * MB),
    ("firefox-143.0", 289 * MB),
    ("libreoffice-25.8.1.1", 1210 * MB),
    ("vlc-3.0.21", 98 * MB),
    ("nodejs-24.9.0", 72 * MB),
];
const AFTER: [(&str, u64); 5] = [
    ("linux-7.2.9", 149 * MB),
    ("firefox-143.0.1", 290 * MB),
    ("libreoffice-25.8.2.2", 1214 * MB),
    ("gimp-3.0.6", 324 * MB),
    ("nodejs-22.20.0", 70 * MB),
];

/// The generations: 24 to 28, 28 running.
fn generations(paths: &[String]) -> Vec<Generation> {
    let ages = [11, 9, 6, 4, 2];
    let kernels = ["7.2.8", "7.2.8", "7.2.8", "7.2.8", "7.2.9"];
    (0..5)
        .map(|i| {
            let number = 24 + i as u32;
            Generation {
                number,
                link: PathBuf::from(format!("/nix/var/nix/profiles/system-{number}-link")),
                target: StorePath::parse(&paths[i]).expect("a store path"),
                created: NOW - ages[i] * DAY - 3_600 * (i as i64 + 1),
                nixos_version: Some(format!("26.11.202610{:02}.7fd4004", 4 + i * 2)),
                kernel_version: Some(kernels[i].to_owned()),
                current: number == 28,
                active: number == 28,
                booted: number == 28,
            }
        })
        .collect()
}

/// The store: a graph of five systems sharing most of their packages, so
/// what each generation changed can be worked out.
fn graph() -> (Graph, Vec<String>) {
    let mut paths: Vec<PathInfo> = Vec::new();
    let mut add = |name: &str, size: u64| {
        paths.push(PathInfo {
            path: store_path(paths.len() + 100, name),
            nar_size: size,
            registered: NOW - 30 * DAY,
            deriver: None,
        });
        paths.len() - 1
    };
    let common: Vec<usize> = COMMON.iter().map(|(n, s)| add(n, *s)).collect();
    let before: Vec<usize> = BEFORE.iter().map(|(n, s)| add(n, *s)).collect();
    let after: Vec<usize> = AFTER.iter().map(|(n, s)| add(n, *s)).collect();
    let mut edges = Vec::new();
    let mut systems = Vec::new();
    for i in 0..5usize {
        let system = add(&format!("nixos-system-ganymede-26.11.202610{:02}.7fd4004", 4 + i * 2), MB);
        systems.push(system);
        let newest = i == 4;
        // Older systems lack a little of what newer ones have.
        for (k, &c) in common.iter().enumerate() {
            if k < 18 + i {
                edges.push((system, c));
            }
        }
        for &p in if newest { &after } else { &before } {
            edges.push((system, p));
        }
    }
    let names: Vec<String> = systems.iter().map(|&s| paths[s].path.clone()).collect();
    (Graph::from_parts(paths, &edges), names)
}

fn locked(kind: &str, owner: &str, repo: &str, rev: &str, age_days: i64, reference: Option<&str>) -> (Locked, Locked) {
    let locked = Locked {
        kind: kind.into(),
        owner: Some(owner.into()),
        repo: Some(repo.into()),
        rev: Some(rev.into()),
        last_modified: Some(NOW - age_days * DAY),
        nar_hash: Some(format!("sha256-{rev}")),
        ..Locked::default()
    };
    let original = Locked {
        kind: kind.into(),
        owner: Some(owner.into()),
        repo: Some(repo.into()),
        reference: reference.map(Into::into),
        ..Locked::default()
    };
    (locked, original)
}

fn input(name: &str, (locked, original): (Locked, Locked), follows_inputs: &[&str]) -> Input {
    Input {
        name: name.into(),
        locked: Some(locked),
        original: Some(original),
        follows: None,
        follows_inputs: follows_inputs.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// The flake's inputs, as its lock file has them.
fn inputs() -> Vec<Input> {
    vec![
        input(
            "aizome",
            locked("github", "bitemyapp", "aizome", "a1c3e5f7b9d2468ace013579bdf2468ace013579", 1, Some("main")),
            &["nixpkgs"],
        ),
        input(
            "home-manager",
            locked("github", "nix-community", "home-manager", "3b7f1e9c2d4a6b8c0e2f4a6c8e0b2d4f6a8c0e2f", 5, None),
            &["nixpkgs"],
        ),
        input(
            "nixos-hardware",
            locked("github", "NixOS", "nixos-hardware", "9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e0d", 18, Some("master")),
            &[],
        ),
        input(
            "nixpkgs",
            locked("github", "NixOS", "nixpkgs", "7fd4004d0511991fff3a0524856a9be09e28c5f0", 6, Some("nixos-unstable")),
            &[],
        ),
        input(
            "sekka",
            locked("github", "bitemyapp", "sekka", "65b011c2e8f4a6b8d0f2e4c6a8b0d2f4e6a8c0b2", 3, Some("nightly")),
            &["nixpkgs"],
        ),
        Input {
            name: "sops-nix".into(),
            locked: None,
            original: None,
            follows: Some(vec!["sekka".into(), "sops-nix".into()]),
            follows_inputs: Vec::new(),
        },
    ]
}

fn package(attr: &str, version: &str, description: &str) -> Package {
    Package {
        attr: attr.into(),
        pname: attr.into(),
        version: version.into(),
        description: description.into(),
        available: true,
        ..Package::default()
    }
}

/// A small package index: what the sample's searches find.
pub fn index() -> PackageIndex {
    let mut packages = vec![
        package("gimp", "3.0.6", "GNU Image Manipulation Program"),
        package("gimp2", "2.10.38", "GNU Image Manipulation Program, the 2.10 series"),
        package("inkscape", "1.4.2", "Vector graphics editor"),
        package("krita", "5.2.11", "Free and open source painting application"),
        package("darktable", "5.2.1", "Virtual lighttable and darkroom for photographers"),
        package("imagemagick", "7.1.2-3", "Software suite to create, edit, compose, or convert bitmap images"),
        package("pinta", "3.0.4", "Drawing/editing program modeled after Paint.NET"),
        package("feh", "3.11.1", "Light-weight image viewer"),
        package("loupe", "49.0", "Simple image viewer application written with GTK4 and Rust"),
        package("gthumb", "3.12.8", "Image browser and viewer for GNOME"),
        package("htop", "3.4.1", "Interactive process viewer"),
        package("btop", "1.4.5", "Monitor of resources"),
        package(
            "ripgrep",
            "15.1.0",
            "Utility that combines the usability of The Silver Searcher with the raw speed of grep",
        ),
        package("firefox", "143.0.1", "Web browser built from Firefox source tree"),
        package("thunderbird", "144.0", "Full-featured e-mail client"),
        package("yt-dlp", "2025.10.14", "Command-line tool to download videos from YouTube.com and other sites"),
        package("git", "2.51.1", "Distributed version control system"),
        package("vim", "9.1.1790", "Most popular clone of the VI editor"),
        package("wget", "1.25.0", "Tool for retrieving files using HTTP, HTTPS, and FTP"),
    ];
    let mut steam = package("steam", "1.0.0.85", "Digital distribution platform");
    steam.unfree = true;
    steam.license = "unfree".into();
    packages.push(steam);
    let mut photoshop = package("image-analyzer", "0.9.1", "Analyse images for metadata and hidden content");
    photoshop.broken = true;
    packages.push(photoshop);
    for p in &mut packages {
        if p.attr == "gimp" {
            p.long_description = "GIMP is a cross-platform image editor: retouch photographs, compose images, \
                                  and draw with a wide set of tools and plug-ins."
                .into();
            p.homepage = "https://www.gimp.org/".into();
            p.main_program = "gimp".into();
            p.license = "GPL-3.0-or-later".into();
        }
    }
    PackageIndex::new(packages)
}

/// The sample's store, read.
pub fn store(generations: &[Generation], graph: Graph) -> Store {
    let latest = match generations {
        [.., before, after] => {
            let closure =
                |g: &Generation| graph.lookup(g.target.as_str()).map(|id| graph.closure([id])).unwrap_or_default();
            Some(yukimi_system::diff::diff(&graph, &closure(before), &closure(after)))
        }
        _ => None,
    };
    let sizes = [17.4, 17.9, 18.3, 18.6, 18.9];
    let mut system_packages: Vec<SystemPackage> = COMMON
        .iter()
        .chain(AFTER.iter())
        .map(|(full, size)| {
            let (name, version) = yukimi_store::path::split_name(full);
            SystemPackage { name: name.to_owned(), version: version.to_owned(), size: *size }
        })
        .collect();
    for (i, name) in [
        "bash-interactive",
        "coreutils-full",
        "findutils",
        "gnugrep",
        "gnused",
        "less",
        "man-db",
        "nano",
        "openssh",
        "procps",
        "shadow",
        "su",
        "util-linux",
        "which",
        "xz",
        "zstd",
        "curl",
        "dbus",
        "fish",
        "fontconfig",
    ]
    .iter()
    .enumerate()
    {
        system_packages.push(SystemPackage {
            name: (*name).to_owned(),
            version: format!("{}.{}", 1 + i % 9, i),
            size: (1 + i as u64 % 7) * 3 * MB,
        });
    }
    system_packages.sort_by(|a, b| a.name.cmp(&b.name));
    let mut heaviest: Vec<Heavy> = COMMON
        .iter()
        .chain(AFTER.iter())
        .map(|(full, size)| {
            let (name, version) = yukimi_store::path::split_name(full);
            Heavy { name: name.to_owned(), version: version.to_owned(), size: *size }
        })
        .collect();
    heaviest.sort_by_key(|h| std::cmp::Reverse(h.size));
    heaviest.truncate(12);
    let root = |link: &str, n: usize, kind: RootKind| Root {
        link: PathBuf::from(link),
        target: StorePath::parse(&store_path(n, "x")).expect("a store path"),
        kind,
    };
    let roots = vec![
        root("/run/current-system", 1, RootKind::CurrentSystem),
        root("/run/booted-system", 1, RootKind::BootedSystem),
        root("/nix/var/nix/profiles/system-27-link", 2, RootKind::SystemGeneration(27)),
        root("/nix/var/nix/profiles/system-26-link", 3, RootKind::SystemGeneration(26)),
        root(
            "/nix/var/nix/profiles/per-user/callen/profile-12-link",
            4,
            RootKind::Profile { name: "profile".into(), user: Some("callen".into()), generation: Some(12) },
        ),
        root("/home/callen/work/aizome/result", 5, RootKind::BuildResult),
        root("/home/callen/work/sekka/.direnv/flake-profile", 6, RootKind::DevShell),
    ];
    Store {
        graph,
        roots,
        composition: Composition {
            system: (18.9 * GB as f64) as u64,
            old_generations: (6.2 * GB as f64) as u64,
            profiles: (1.4 * GB as f64) as u64,
            projects: (22.7 * GB as f64) as u64,
            garbage: (12.1 * GB as f64) as u64,
            total: (61.3 * GB as f64) as u64,
            paths: 58_214,
            garbage_paths: 9_377,
        },
        system_packages,
        heaviest,
        latest,
        generation_sizes: generations.iter().zip(sizes).map(|(g, s)| (g.number, (s * GB as f64) as u64)).collect(),
    }
}

/// Sekka's choices, as the sample system declares them: three desktops,
/// Aizome by default, and the login screen chosen for it.
pub fn choices() -> Vec<Choice> {
    let option = |id: &str, title: &str, detail: Option<&str>, needs: Option<&str>| ChoiceOption {
        id: id.into(),
        title: title.into(),
        detail: detail.map(Into::into),
        needs: needs.map(Into::into),
    };
    let desktops: Vec<ChoiceOption> =
        [("aizome", "Aizome"), ("tatami", "Tatami"), ("plasma", "Plasma"), ("gnome", "GNOME"), ("xfce", "Xfce")]
            .into_iter()
            .map(|(id, title)| option(id, title, None, None))
            .collect();
    vec![
        Choice {
            setting: "calamares.desktops".into(),
            title: "Desktops".into(),
            many: true,
            boolean: false,
            value: ChoiceValue::Many(vec!["aizome".into(), "tatami".into(), "gnome".into()]),
            options: desktops.clone(),
            within: None,
        },
        Choice {
            setting: "calamares.defaultDesktop".into(),
            title: "Default desktop".into(),
            many: false,
            boolean: false,
            value: ChoiceValue::One("aizome".into()),
            options: desktops,
            within: Some("calamares.desktops".into()),
        },
        Choice {
            setting: "calamares.loginScreen".into(),
            title: "Login screen".into(),
            many: false,
            boolean: false,
            value: ChoiceValue::One("auto".into()),
            options: vec![
                option(
                    "auto",
                    "Automatic",
                    Some(
                        "GDM when GNOME is installed (GNOME locks its screen only under GDM), else Aizome's own when \
                         Aizome is, else Plasma Login.",
                    ),
                    None,
                ),
                option(
                    "aizome",
                    "Aizome",
                    Some("Aizome's own, listing every desktop. GNOME sessions have no lock screen under it."),
                    Some("calamares.desktops=aizome"),
                ),
                option("gdm", "GDM", Some("GNOME's."), None),
                option("plasma-login", "Plasma Login", Some("KDE's."), None),
            ],
            within: None,
        },
        Choice {
            setting: "calamares.bootSplash.enable".into(),
            title: "Boot splash".into(),
            many: false,
            boolean: true,
            value: ChoiceValue::One("false".into()),
            options: vec![
                option(
                    "true",
                    "On",
                    Some("Sekka's crest while the computer starts and stops. Esc shows the messages."),
                    None,
                ),
                option("false", "Off", Some("The computer's messages as it starts and stops."), None),
            ],
            within: None,
        },
    ]
}

/// The sample system, read in full.
pub fn model() -> Model {
    let (graph, systems) = graph();
    let generations = generations(&systems);
    let store = store(&generations, graph);
    let nixpkgs = store_path(1, "source");
    Model {
        setup: Setup {
            kind: Kind::Flake,
            dir: PathBuf::from("/etc/nixos"),
            main: Some(PathBuf::from("/etc/nixos/configuration.nix")),
            host: "ganymede".into(),
            git: false,
            facts: Facts {
                nixpkgs: Some(nixpkgs.clone()),
                allow_unfree: Some(true),
                own_catalog: None,
                configuration: None,
                catalogs: Vec::new(),
                choices: choices(),
            },
        },
        info: SystemInfo {
            hostname: "ganymede".into(),
            nixos_version: "26.11.20261012.7fd4004".into(),
            kernel: "7.2.9".into(),
        },
        completion: Default::default(),
        sekka_url: Some(yukimi_config::sekka::NIGHTLY.into()),
        generations,
        user_packages: vec![
            Element {
                name: "btop".into(),
                attr_path: Some("legacyPackages.x86_64-linux.btop".into()),
                original_url: Some(format!("path:{nixpkgs}")),
                store_paths: vec![StorePath::parse(&store_path(7, "btop-1.4.5")).expect("a store path")],
                active: true,
            },
            Element {
                name: "yt-dlp".into(),
                attr_path: Some("legacyPackages.x86_64-linux.yt-dlp".into()),
                original_url: Some(format!("path:{nixpkgs}")),
                store_paths: vec![StorePath::parse(&store_path(8, "yt-dlp-2025.10.14")).expect("a store path")],
                active: true,
            },
        ],
        configured: Configured {
            yukimi: Choices {
                packages: vec!["gimp".into(), "inkscape".into(), "ripgrep".into()],
                programs: vec!["steam".into()],
            },
            system: vec![
                ListedPackage { attr: Some("git".into()), text: "git".into() },
                ListedPackage { attr: Some("vim".into()), text: "vim".into() },
                ListedPackage { attr: Some("wget".into()), text: "wget".into() },
                ListedPackage { attr: None, text: "(python3.withPackages (ps: [ ps.numpy ps.requests ]))".into() },
            ],
            user: vec![ListedPackage { attr: Some("thunderbird".into()), text: "thunderbird".into() }],
            settings: BTreeMap::new(),
            choices: choices().into_iter().map(|c| (c.setting, c.value.values())).collect(),
        },
        installer_applications: Default::default(),
        user: "callen".into(),
        allow_unfree: true,
        catalog: catalog::own(),
        inputs: inputs(),
        update_history: Default::default(),
        channels: Vec::new(),
        nixpkgs: Some(nixpkgs),
        store: Some(Arc::new(store)),
        problems: Vec::new(),
        paths: crate::paths::Paths::default(),
    }
}

/// What the sample's last check for updates found, an hour ago: newer
/// Nixpkgs and Sekka, the rest current, and one source that couldn't be
/// asked.
pub fn check(model: &Model) -> UpdateCheck {
    let release = |rev: &str, age_days: i64| Release {
        rev: Some(rev.into()),
        last_modified: Some(NOW - age_days * DAY),
        count: None,
        nar_hash: Some(format!("sha256-{rev}")),
    };
    let current = |name: &str| {
        let locked = model.inputs.iter().find(|i| i.name == name).and_then(|i| i.locked.clone()).expect("an input");
        Release { rev: locked.rev, last_modified: locked.last_modified, count: None, nar_hash: locked.nar_hash }
    };
    let answers = BTreeMap::from([
        ("nixpkgs".to_owned(), Ok(release("c59305b8e1f2a3b4c5d6e7f8091a2b3c4d5e6f70", 0))),
        ("sekka".to_owned(), Ok(release("f537632a1b2c3d4e5f60718293a4b5c6d7e8f901", 0))),
        ("home-manager".to_owned(), Ok(current("home-manager"))),
        ("aizome".to_owned(), Ok(current("aizome"))),
        ("nixos-hardware".to_owned(), Err("GitHub's limit of 60 checks an hour was reached".to_owned())),
    ]);
    checks::gather(model, NOW - 3_600, answers).expect("answers")
}

/// A source update whose substantive changes are inside a nested input.
pub fn comparison() -> crate::comparison::Details {
    use crate::comparison::{Delta, Details};
    let model = model();
    let source = |name: &str| model.inputs.iter().find(|i| i.name == name).unwrap().locked.clone().unwrap();
    let changed = |name: &str| {
        let after = source(name);
        let mut before = after.clone();
        before.rev = Some("1111111111111111111111111111111111111111".into());
        Delta { name: name.into(), before: Some(before), after: Some(after), children: vec![] }
    };
    let mut root = changed("sekka");
    let mut aizome = changed("aizome");
    aizome.children.push(changed("nixpkgs"));
    root.children = vec![aizome];
    Details {
        root,
        source_note: "Inputs: generation 27 → generation 28.".into(),
        source_error: None,
        packages: Ok(model.store.as_ref().unwrap().latest.as_ref().unwrap().changes.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yukimi_system::diff::ChangeKind;

    #[test]
    fn the_sample_hangs_together() {
        let model = model();
        let store = model.store.as_ref().unwrap();
        let latest = store.latest.as_ref().expect("what 28 changed");
        assert_eq!(latest.count(ChangeKind::Upgraded), 3, "{latest:?}");
        assert_eq!(latest.count(ChangeKind::Downgraded), 1);
        assert_eq!(latest.count(ChangeKind::Added), 2);
        assert_eq!(latest.count(ChangeKind::Removed), 1);
        assert_eq!(model.current_generation().map(|g| g.number), Some(28));
        let check = check(&model);
        let moves = checks::movements(&model, &check);
        assert_eq!(checks::newer_count(&moves), 2);
        assert!(index().get("gimp").is_some());
    }
}
