//! Hints: what in a scan could be freed, how much, how risky that is, and
//! the safe way to do it. They're what an agent (or a person) reads first.
//!
//! Each hint claims the folders it covers, so no byte is counted by two
//! hints: the Trash, then browser caches, package managers' caches, the
//! rest of `~/.cache`, folders tagged as caches, build outputs, logs, old
//! downloads, and duplicates among what's left. Nix generations, the
//! store, and container storage are reported with the way to clean them
//! (Yukimi, `podman`/`docker`), never with commands that touch the store.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dupes;
use crate::format::{self, shell_quote};
use crate::kinds::{self, FileKind};
use crate::nix;
use crate::scan::Scan;
use crate::time::{self, DAY};
use crate::tree::{Kind, NodeId, Tree, flags};

/// How risky acting on a hint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// Regenerated automatically when needed; no one's data.
    Safe,
    /// Rebuilt or downloaded again on demand, at some cost in time.
    Low,
    /// Someone has to decide: it may be data, or irreversible.
    Medium,
    /// Likely to lose data or break something; only on explicit request.
    High,
}

impl Risk {
    pub fn id(self) -> &'static str {
        match self {
            Risk::Safe => "safe",
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

/// What to do about a hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    /// Run `command` (or each path's own `command`).
    Command,
    /// Move the paths to the Trash (`aizome-kura trash PATH…`), reversibly.
    Trash,
    /// Open an app: `command` starts it.
    OpenApp,
    /// Look first: nothing to run until a person decides.
    Review,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub kind: ActionKind,
    /// A shell command, for the whole hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub description: String,
}

/// One path a hint covers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HintPath {
    pub path: String,
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
    /// The newest modification inside, RFC 3339.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// What it is, when more than the hint says ("Rust build output of
    /// ~/src/foo").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A command for this path alone, when better than the hint's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// A set of identical files (the duplicates hint).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub bytes_each: u64,
    /// xxh3-128 of the contents (of both ends only when not verified).
    pub hash: String,
    /// `full` when every byte was hashed; `partial` when only the size and
    /// the first and last 64 KiB were compared.
    pub verified: String,
    pub reclaimable_bytes: u64,
    /// Oldest first: the first is the one a person would usually keep.
    pub paths: Vec<String>,
}

/// One hint. See `docs/kura-index.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hint {
    /// Stable: `trash`, `browser-caches`, `package-caches`, `large-caches`, `caches`,
    /// `tagged-caches`, `build-outputs`, `logs`, `old-downloads`,
    /// `duplicates`, `nix-generations`, `nix-result-links`, `containers`,
    /// `flatpak`.
    pub id: String,
    pub title: String,
    pub summary: String,
    /// What acting on it would free, in allocated bytes (0 when unknown,
    /// as for Nix generations: see `count`).
    pub reclaimable_bytes: u64,
    pub risk: Risk,
    pub action: Action,
    /// Largest first; at most [`MAX_PATHS`].
    pub paths: Vec<HintPath>,
    /// Paths covered but not listed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more_paths: u64,
    /// A count, for hints measured in things rather than bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// What `count` counts: `generations`, `links`, `sets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_unit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<DuplicateGroup>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Hint {
    /// What it frees, for people: "12.3 GB", "4 generations", "size unknown".
    pub fn frees(&self) -> String {
        match (self.reclaimable_bytes, self.count) {
            (0, Some(c)) => format!("{} {}", format::count(c), self.count_unit.as_deref().unwrap_or("items")),
            (0, None) => "size unknown".into(),
            (b, _) => format::size(b),
        }
    }
}

/// Paths a hint lists at most.
pub const MAX_PATHS: usize = 100;

/// Hints that would free less than this aren't given.
pub const MIN_BYTES: u64 = 1_000_000;

/// Where things are, and how far to go.
#[derive(Clone, Debug)]
pub struct Context {
    pub home: Option<PathBuf>,
    pub cache_home: Option<PathBuf>,
    pub data_home: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub trash: Option<PathBuf>,
    pub now: i64,
    /// Downloads untouched this long are "old".
    pub old_after_days: i64,
    /// Look for duplicates (reads files).
    pub duplicates: bool,
    pub dupe_limits: dupes::Limits,
    /// Nix's profiles (counted, never touched).
    pub nix_profiles: Vec<nix::Profile>,
    pub nix_store: Option<nix::StoreSize>,
}

impl Context {
    /// This user's places, now.
    pub fn from_env() -> Context {
        let home = crate::home_dir();
        Context {
            cache_home: crate::xdg_dir("XDG_CACHE_HOME", ".cache"),
            data_home: crate::xdg_dir("XDG_DATA_HOME", ".local/share"),
            config_home: crate::xdg_dir("XDG_CONFIG_HOME", ".config"),
            downloads: crate::downloads_dir(),
            trash: crate::trash::home_trash(),
            home,
            now: time::now_unix(),
            old_after_days: 90,
            duplicates: true,
            dupe_limits: dupes::Limits::default(),
            nix_profiles: Vec::new(),
            nix_store: None,
        }
    }
}

/// The hints for a scan, most reclaimable first (counted ones last).
pub fn detect(scan: &Scan, cx: &Context) -> Vec<Hint> {
    let mut d = Detector::new(&scan.tree, cx);
    d.trash();
    // Containers' and Flatpak's storage first: they hold caches and logs
    // of their own that aren't the person's to clear by hand.
    d.containers();
    d.flatpak();
    d.browser_caches();
    d.package_caches();
    d.caches();
    d.build_outputs();
    d.tagged_caches();
    d.logs();
    d.old_downloads();
    if cx.duplicates {
        d.duplicates();
    }
    d.nix();
    let mut hints = d.hints;
    // Under a megabyte isn't worth anyone's attention.
    hints.retain(|h| {
        h.reclaimable_bytes >= MIN_BYTES || h.count.is_some_and(|c| c > 0) || h.id == "containers" || h.id == "flatpak"
    });
    hints.sort_by(|a, b| b.reclaimable_bytes.cmp(&a.reclaimable_bytes).then(a.risk.cmp(&b.risk)));
    hints
}

struct Detector<'a> {
    tree: &'a Tree,
    cx: &'a Context,
    /// Nodes a hint covers (with everything inside them).
    claimed: Vec<bool>,
    hints: Vec<Hint>,
}

/// What a build output is, by its folder's name and its neighbours.
struct BuildRule {
    name: &'static str,
    /// The folder's parent must hold one of these (empty: no condition).
    sibling: &'static [&'static str],
    /// Or the folder itself must hold one of these.
    inside: &'static [&'static str],
    what: &'static str,
    command: fn(&Path) -> String,
}

fn rm(p: &Path) -> String {
    format!("rm -rf -- {}", shell_quote(p))
}

fn cargo_clean(p: &Path) -> String {
    match p.parent() {
        Some(project) => format!("cargo clean --manifest-path {}", shell_quote(&project.join("Cargo.toml"))),
        None => rm(p),
    }
}

/// A Cargo target folder found by its contents ([`Detector::is_cargo_target`]).
const CARGO_TARGET: BuildRule =
    BuildRule { name: "", sibling: &[], inside: &[], what: "Rust (Cargo) build output", command: rm };

const BUILD_RULES: &[BuildRule] = &[
    BuildRule {
        name: "target",
        sibling: &["Cargo.toml"],
        inside: &[".rustc_info.json"],
        what: "Rust (Cargo) build output",
        command: cargo_clean,
    },
    BuildRule {
        name: "node_modules",
        sibling: &["package.json"],
        inside: &[".package-lock.json", ".yarn-integrity", ".modules.yaml"],
        what: "npm packages (npm install brings them back)",
        command: rm,
    },
    BuildRule { name: ".venv", sibling: &[], inside: &["pyvenv.cfg"], what: "Python virtual environment", command: rm },
    BuildRule { name: "venv", sibling: &[], inside: &["pyvenv.cfg"], what: "Python virtual environment", command: rm },
    BuildRule { name: ".tox", sibling: &[], inside: &[], what: "tox environments", command: rm },
    BuildRule { name: ".nox", sibling: &[], inside: &[], what: "nox environments", command: rm },
    BuildRule { name: ".mypy_cache", sibling: &[], inside: &[], what: "mypy cache", command: rm },
    BuildRule { name: ".pytest_cache", sibling: &[], inside: &[], what: "pytest cache", command: rm },
    BuildRule { name: ".ruff_cache", sibling: &[], inside: &[], what: "Ruff cache", command: rm },
    BuildRule {
        name: "dist",
        sibling: &["package.json", "pyproject.toml", "setup.py", "Cargo.toml"],
        inside: &[],
        what: "built distribution (release artefacts may be kept here)",
        command: rm,
    },
    BuildRule {
        name: "build",
        sibling: &[
            "package.json",
            "pyproject.toml",
            "setup.py",
            "CMakeLists.txt",
            "meson.build",
            "build.gradle",
            "build.gradle.kts",
            "Makefile",
        ],
        inside: &[],
        what: "build output",
        command: rm,
    },
    BuildRule { name: ".next", sibling: &["package.json"], inside: &[], what: "Next.js build output", command: rm },
    BuildRule { name: ".nuxt", sibling: &["package.json"], inside: &[], what: "Nuxt build output", command: rm },
    BuildRule {
        name: ".svelte-kit",
        sibling: &["package.json"],
        inside: &[],
        what: "SvelteKit build output",
        command: rm,
    },
    BuildRule { name: ".parcel-cache", sibling: &[], inside: &[], what: "Parcel cache", command: rm },
    BuildRule { name: ".turbo", sibling: &[], inside: &[], what: "Turborepo cache", command: rm },
    BuildRule { name: ".angular", sibling: &["angular.json"], inside: &[], what: "Angular cache", command: rm },
    BuildRule {
        name: ".gradle",
        sibling: &["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"],
        inside: &[],
        what: "Gradle project cache",
        command: rm,
    },
    BuildRule { name: "zig-cache", sibling: &["build.zig"], inside: &[], what: "Zig cache", command: rm },
    BuildRule { name: ".zig-cache", sibling: &["build.zig"], inside: &[], what: "Zig cache", command: rm },
    BuildRule { name: "zig-out", sibling: &["build.zig"], inside: &[], what: "Zig build output", command: rm },
    BuildRule { name: "dist-newstyle", sibling: &[], inside: &["cache"], what: "Cabal build output", command: rm },
    BuildRule { name: ".stack-work", sibling: &[], inside: &[], what: "Stack build output", command: rm },
    BuildRule {
        name: "_build",
        sibling: &["dune-project", "mix.exs", "rebar.config"],
        inside: &[],
        what: "dune/Mix build output",
        command: rm,
    },
    BuildRule {
        name: ".direnv",
        sibling: &[".envrc"],
        inside: &[],
        what: "direnv's dev-shell cache; it holds Nix GC roots, so removing it lets Yukimi's Storage free the shell's store paths",
        command: rm,
    },
    BuildRule {
        name: ".devenv",
        sibling: &["devenv.nix"],
        inside: &[],
        what: "devenv state; it holds Nix GC roots",
        command: rm,
    },
];

/// Package managers' download caches outside `~/.cache` (or with a better
/// command than `rm`): (path under home, what, command, risk).
const PACKAGE_CACHES: &[(&str, &str, &str, Risk)] = &[
    (".cargo/registry/cache", "Cargo's downloaded crates", "rm -rf -- ~/.cargo/registry/cache", Risk::Safe),
    (".cargo/registry/src", "Cargo's unpacked crate sources", "rm -rf -- ~/.cargo/registry/src", Risk::Safe),
    (".cargo/git/checkouts", "Cargo's git checkouts", "rm -rf -- ~/.cargo/git/checkouts", Risk::Safe),
    (".npm/_cacache", "npm's cache", "npm cache clean --force", Risk::Safe),
    (".cache/pip", "pip's cache", "pip cache purge", Risk::Safe),
    (".cache/uv", "uv's cache", "uv cache clean", Risk::Safe),
    (".cache/yarn", "Yarn's cache", "yarn cache clean", Risk::Safe),
    (".yarn/berry/cache", "Yarn Berry's cache", "yarn cache clean --all", Risk::Safe),
    (".local/share/pnpm/store", "pnpm's store", "pnpm store prune", Risk::Low),
    (".cache/go-build", "Go's build cache", "go clean -cache", Risk::Safe),
    ("go/pkg/mod", "Go's module cache", "go clean -modcache", Risk::Low),
    (".gradle/caches", "Gradle's caches", "rm -rf -- ~/.gradle/caches", Risk::Low),
    (".m2/repository", "Maven's repository", "rm -rf -- ~/.m2/repository", Risk::Low),
    (
        ".cache/bazel",
        "Bazel's cache",
        "bazel clean --expunge (in each workspace), or rm -rf -- ~/.cache/bazel",
        Risk::Low,
    ),
    (
        ".cache/huggingface",
        "Hugging Face models and datasets (large to download again)",
        "huggingface-cli delete-cache",
        Risk::Medium,
    ),
    (".cache/ms-playwright", "Playwright's browsers", "rm -rf -- ~/.cache/ms-playwright", Risk::Low),
    (".cache/nix", "Nix's evaluation and download caches", "rm -rf -- ~/.cache/nix", Risk::Safe),
    (".cache/thumbnails", "thumbnails", "rm -rf -- ~/.cache/thumbnails", Risk::Safe),
    (".cache/electron", "Electron's downloads", "rm -rf -- ~/.cache/electron", Risk::Safe),
    (".cache/JetBrains", "JetBrains IDEs' caches", "rm -rf -- ~/.cache/JetBrains", Risk::Low),
    (".stack/pantry", "Stack's package index", "rm -rf -- ~/.stack/pantry", Risk::Low),
    (".cabal/packages", "Cabal's downloaded packages", "rm -rf -- ~/.cabal/packages", Risk::Low),
    (".ghcup/cache", "GHCup's downloads", "ghcup gc --cache", Risk::Safe),
    (".rustup/downloads", "rustup's downloads", "rm -rf -- ~/.rustup/downloads", Risk::Safe),
    (".cache/mesa_shader_cache", "Mesa's shader cache", "rm -rf -- ~/.cache/mesa_shader_cache", Risk::Safe),
    (".cache/mesa_shader_cache_db", "Mesa's shader cache", "rm -rf -- ~/.cache/mesa_shader_cache_db", Risk::Safe),
    (".cache/nvidia", "NVIDIA's shader cache", "rm -rf -- ~/.cache/nvidia", Risk::Safe),
];

/// Browser caches under home: (glob-ish path with `*` for one level, browser).
const BROWSER_CACHES: &[(&str, &str)] = &[
    (".cache/mozilla/firefox/*/cache2", "Firefox"),
    (".cache/librewolf/*/cache2", "LibreWolf"),
    (".cache/zen/*/cache2", "Zen"),
    (".cache/google-chrome", "Chrome"),
    (".cache/chromium", "Chromium"),
    (".cache/BraveSoftware", "Brave"),
    (".cache/vivaldi", "Vivaldi"),
    (".cache/microsoft-edge", "Edge"),
    (".cache/epiphany", "Web"),
    (".cache/qutebrowser", "qutebrowser"),
    (".config/google-chrome/*/Service Worker/CacheStorage", "Chrome"),
    (".config/chromium/*/Service Worker/CacheStorage", "Chromium"),
    (".config/BraveSoftware/Brave-Browser/*/Service Worker/CacheStorage", "Brave"),
    (".config/google-chrome/*/GPUCache", "Chrome"),
    (".config/chromium/*/GPUCache", "Chromium"),
    (".config/Code/Cache", "VS Code"),
    (".config/Code/CachedData", "VS Code"),
    (".config/Code/Service Worker/CacheStorage", "VS Code"),
    (".config/discord/Cache", "Discord"),
    (".config/Slack/Cache", "Slack"),
    (".config/Slack/Service Worker/CacheStorage", "Slack"),
    (".config/obsidian/Cache", "Obsidian"),
    (".config/spotify/Data", "Spotify"),
    (".cache/spotify", "Spotify"),
];

impl<'a> Detector<'a> {
    fn new(tree: &'a Tree, cx: &'a Context) -> Detector<'a> {
        // What went to the Trash since the scan counts as claimed.
        Detector { tree, cx, claimed: tree.gone(), hints: Vec::new() }
    }

    /// Whether `id` or a folder above it is claimed.
    fn is_claimed(&self, id: NodeId) -> bool {
        let mut at = id;
        loop {
            if self.claimed[at.index()] {
                return true;
            }
            match self.tree.parent(at) {
                Some(p) => at = p,
                None => return false,
            }
        }
    }

    /// Bytes inside `id` that no hint claimed yet.
    fn unclaimed_bytes(&self, id: NodeId) -> (u64, u64) {
        if self.is_claimed(id) {
            return (0, 0);
        }
        let n = self.tree.node(id);
        let (mut alloc, mut apparent) = (n.alloc, n.apparent);
        let mut stack: Vec<NodeId> = self.tree.children(id).collect();
        while let Some(c) = stack.pop() {
            if self.claimed[c.index()] {
                let cn = self.tree.node(c);
                alloc -= cn.alloc;
                apparent -= cn.apparent;
            } else if self.tree.node(c).is_dir() {
                stack.extend(self.tree.children(c));
            }
        }
        (alloc, apparent)
    }

    fn claim(&mut self, id: NodeId) {
        self.claimed[id.index()] = true;
    }

    /// The node at `rel` under home (with `*` matching one level), as scanned.
    fn under_home(&self, rel: &str) -> Vec<NodeId> {
        let Some(home) = &self.cx.home else { return Vec::new() };
        self.glob(&home.join(rel))
    }

    fn glob(&self, pattern: &Path) -> Vec<NodeId> {
        let root = self.tree.root_path();
        // The pattern's part under the root; patterns above the root match
        // nothing (the root itself is matched whole).
        let rest = match pattern.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut at = vec![NodeId::ROOT];
        for part in rest.components() {
            let part = part.as_os_str().as_encoded_bytes();
            let mut next = Vec::new();
            for &n in &at {
                for c in self.tree.children(n) {
                    if part == b"*" || self.tree.name_bytes(c) == part {
                        next.push(c);
                    }
                }
            }
            at = next;
        }
        at
    }

    fn hint_path(&self, id: NodeId, note: Option<String>, command: Option<String>) -> HintPath {
        let n = self.tree.node(id);
        let (alloc, apparent) = self.unclaimed_bytes(id);
        HintPath {
            path: self.tree.path(id).display().to_string(),
            allocated_bytes: alloc,
            apparent_bytes: apparent,
            modified: (n.mtime > 0).then(|| time::rfc3339(n.mtime)),
            note,
            command,
        }
    }

    /// Add a hint over `paths` (claiming them), if it frees anything.
    fn push(
        &mut self,
        id: &str,
        title: String,
        summary: String,
        risk: Risk,
        action: Action,
        mut paths: Vec<(NodeId, HintPath)>,
    ) {
        paths.retain(|(_, p)| p.allocated_bytes > 0);
        // Inner paths of an outer one in the same hint count once.
        paths.sort_by_key(|p| self.tree.depth(p.0));
        let mut kept: Vec<(NodeId, HintPath)> = Vec::new();
        for (n, p) in paths {
            if kept.iter().any(|(k, _)| self.tree.is_within(n, *k)) {
                continue;
            }
            kept.push((n, p));
        }
        if kept.is_empty() {
            return;
        }
        for (n, _) in &kept {
            self.claim(*n);
        }
        kept.sort_by_key(|k| std::cmp::Reverse(k.1.allocated_bytes));
        let total: u64 = kept.iter().map(|(_, p)| p.allocated_bytes).sum();
        let more = kept.len().saturating_sub(MAX_PATHS) as u64;
        let paths: Vec<HintPath> = kept.into_iter().take(MAX_PATHS).map(|(_, p)| p).collect();
        self.hints.push(Hint {
            id: id.into(),
            title,
            summary,
            reclaimable_bytes: total,
            risk,
            action,
            paths,
            more_paths: more,
            count: None,
            count_unit: None,
            groups: Vec::new(),
        });
    }

    fn trash(&mut self) {
        let Some(trash) = self.cx.trash.clone() else { return };
        let mut paths = Vec::new();
        let mut command = None;
        if let Some(id) = self.tree.find(&trash) {
            for part in ["files", "info", "expunged"] {
                if let Some(c) = self.tree.child_named(id, part) {
                    paths.push((c, self.hint_path(c, None, None)));
                }
            }
            command = Some(format!(
                "gio trash --empty  # or: rm -rf -- {0}/files {0}/info {0}/directorysizes",
                shell_quote(&trash)
            ));
        }
        // Other filesystems' trash folders inside the scan (`.Trash-1000`).
        let uid = unsafe { libc::getuid() };
        let other = format!(".Trash-{uid}");
        for (i, n) in self.tree.nodes().iter().enumerate() {
            if n.is_dir() && self.tree.name_bytes(NodeId(i as u32)) == other.as_bytes() {
                let id = NodeId(i as u32);
                paths.push((
                    id,
                    self.hint_path(
                        id,
                        Some("trash of the filesystem it's on".into()),
                        Some(format!(
                            "rm -rf -- {}/files {}/info",
                            shell_quote(&self.tree.path(id)),
                            shell_quote(&self.tree.path(id))
                        )),
                    ),
                ));
            }
        }
        let n = crate::trash::items(&trash).len();
        self.push(
            "trash",
            "Trash".into(),
            format!(
                "{} already moved to the Trash still take space until it's emptied. Emptying it can't be undone.",
                format::plural(n as u64, "item", "items")
            ),
            Risk::Medium,
            Action {
                kind: ActionKind::Command,
                command,
                description: "Empty the Trash (after the person agrees: it is permanent).".into(),
            },
            paths,
        );
    }

    fn browser_caches(&mut self) {
        let mut paths = Vec::new();
        for (rel, browser) in BROWSER_CACHES {
            for id in self.under_home(rel) {
                if self.is_claimed(id) {
                    continue;
                }
                let p = self.tree.path(id);
                paths.push((id, self.hint_path(id, Some(format!("{browser}'s cache")), Some(rm(&p)))));
            }
        }
        self.push(
            "browser-caches",
            "Browser and app caches".into(),
            "Web browsers and Electron apps keep copies of pages, scripts and media. They download them again as needed; quit the app before clearing its cache.".into(),
            Risk::Low,
            Action { kind: ActionKind::Command, command: None, description: "Quit the browser or app, then run each path's command.".into() },
            paths,
        );
    }

    fn package_caches(&mut self) {
        let mut paths = Vec::new();
        for (rel, what, command, risk) in PACKAGE_CACHES {
            if *risk == Risk::Medium {
                continue;
            }
            for id in self.under_home(rel) {
                if !self.is_claimed(id) {
                    paths.push((id, self.hint_path(id, Some((*what).into()), Some((*command).into()))));
                }
            }
        }
        self.push(
            "package-caches",
            "Package managers' caches".into(),
            "Downloads that Cargo, npm, pip, Go and other tools keep so the next build needn't fetch them. They're fetched again when needed.".into(),
            Risk::Safe,
            Action { kind: ActionKind::Command, command: None, description: "Run each path's command (the tool's own cleanup where it has one).".into() },
            paths,
        );
        // Large ones that are expensive to fetch again stand alone.
        for (rel, what, command, risk) in PACKAGE_CACHES {
            if *risk != Risk::Medium {
                continue;
            }
            let found: Vec<_> = self.under_home(rel).into_iter().filter(|&id| !self.is_claimed(id)).collect();
            let paths: Vec<_> = found
                .iter()
                .map(|&id| (id, self.hint_path(id, Some((*what).into()), Some((*command).into()))))
                .collect();
            self.push(
                "large-caches",
                format!("{}{}", what[..1].to_uppercase(), &what[1..]),
                "Fetched again when needed, but large and slow to download.".into(),
                *risk,
                Action {
                    kind: ActionKind::Command,
                    command: Some((*command).into()),
                    description: "Ask before clearing: it may take long to download again.".into(),
                },
                paths,
            );
        }
    }

    fn caches(&mut self) {
        // The rest of ~/.cache, one path per app.
        if let Some(cache) = self.cx.cache_home.clone()
            && let Some(id) = self.tree.find(&cache)
        {
            let kids: Vec<NodeId> = self.tree.children(id).collect();
            let paths: Vec<_> = kids
                .into_iter()
                .filter(|&c| !self.is_claimed(c))
                .map(|c| {
                    let p = self.tree.path(c);
                    (c, self.hint_path(c, None, Some(rm(&p))))
                })
                .collect();
            self.push(
                "caches",
                format!("Caches in {}", format::path(&cache)),
                "Apps keep what they can make again here, by the XDG convention. Clearing an app's cache while it runs can confuse it, so quit it first.".into(),
                Risk::Safe,
                Action { kind: ActionKind::Command, command: None, description: "Quit the app, then run the path's command.".into() },
                paths,
            );
        }
    }

    /// Folders that say they're caches (CACHEDIR.TAG, bford.info/cachedir).
    fn tagged_caches(&mut self) {
        let mut tagged = Vec::new();
        for (i, n) in self.tree.nodes().iter().enumerate() {
            let id = NodeId(i as u32);
            if n.kind != Kind::File || self.tree.name_bytes(id) != b"CACHEDIR.TAG" {
                continue;
            }
            let Some(dir) = self.tree.parent(id) else { continue };
            if self.is_claimed(dir) || dir == NodeId::ROOT {
                continue;
            }
            let path = self.tree.path(dir);
            if crate::nix::in_store(&path) || !cachedir_tag_valid(&path.join("CACHEDIR.TAG")) {
                continue;
            }
            tagged.push((dir, self.hint_path(dir, Some("tagged as a cache (CACHEDIR.TAG)".into()), Some(rm(&path)))));
        }
        self.push(
            "tagged-caches",
            "Folders tagged as caches".into(),
            "These folders carry a CACHEDIR.TAG, the convention by which a program says a folder holds only what it can make again.".into(),
            Risk::Safe,
            Action { kind: ActionKind::Command, command: None, description: "Run each path's command.".into() },
            tagged,
        );
    }

    fn build_outputs(&mut self) {
        let tree = self.tree;
        let mut found: Vec<(NodeId, HintPath)> = Vec::new();
        let mut stale = 0u64;
        let mut stack = vec![NodeId::ROOT];
        while let Some(id) = stack.pop() {
            let n = tree.node(id);
            if !n.is_dir() || self.claimed[id.index()] {
                continue;
            }
            let name = tree.name_bytes(id);
            if id != NodeId::ROOT && name == b".git" {
                continue;
            }
            let rule = (id != NodeId::ROOT)
                .then(|| {
                    BUILD_RULES
                        .iter()
                        .find(|r| r.name.as_bytes() == name && self.rule_holds(r, id))
                        .or_else(|| self.is_cargo_target(id).then_some(&CARGO_TARGET))
                })
                .flatten();
            if let Some(rule) = rule {
                let path = tree.path(id);
                if crate::nix::in_store(&path) {
                    continue;
                }
                let project = path.parent().map(format::path).unwrap_or_default();
                let age = self.cx.now - n.mtime;
                let mut note = if rule.name.is_empty() {
                    format!("{} (a shared CARGO_TARGET_DIR)", rule.what)
                } else {
                    format!("{} of {project}", rule.what)
                };
                if age > 90 * DAY {
                    note.push_str(&format!(", last changed {}", time::ago(n.mtime, self.cx.now)));
                }
                let hp = self.hint_path(id, Some(note), Some((rule.command)(&path)));
                if age > 90 * DAY {
                    stale += hp.allocated_bytes;
                }
                found.push((id, hp));
                continue;
            }
            stack.extend(tree.children(id).filter(|&c| tree.node(c).is_dir()));
        }
        let total: u64 = found.iter().map(|(_, p)| p.allocated_bytes).sum();
        let mut summary =
            "Build tools make these again from the project's sources; the next build takes longer.".to_owned();
        if stale > 0 && total > 0 {
            summary.push_str(&format!(" {} of it is in projects not built for 90 days or more.", format::size(stale)));
        }
        self.push(
            "build-outputs",
            "Build outputs and dependencies".into(),
            summary,
            Risk::Low,
            Action {
                kind: ActionKind::Command,
                command: None,
                description: "Run each path's command; prefer projects not worked on lately.".into(),
            },
            found,
        );
    }

    /// A Cargo target folder by its contents, whatever its name (a shared
    /// `CARGO_TARGET_DIR`): Cargo's `.rustc_info.json`, or its cache tag
    /// beside a `debug` or `release` folder.
    fn is_cargo_target(&self, id: NodeId) -> bool {
        let t = self.tree;
        t.child_named(id, ".rustc_info.json").is_some()
            || (t.child_named(id, "CACHEDIR.TAG").is_some()
                && (t.child_named(id, "debug").is_some() || t.child_named(id, "release").is_some()))
    }

    fn rule_holds(&self, r: &BuildRule, id: NodeId) -> bool {
        if r.sibling.is_empty() && r.inside.is_empty() {
            return true;
        }
        let parent = self.tree.parent(id);
        let sib = parent.is_some_and(|p| r.sibling.iter().any(|s| self.tree.child_named(p, s).is_some()));
        let ins = r.inside.iter().any(|s| self.tree.child_named(id, s).is_some());
        sib || ins
    }

    fn logs(&mut self) {
        let tree = self.tree;
        let mut found = Vec::new();
        for (i, n) in tree.nodes().iter().enumerate() {
            let id = NodeId(i as u32);
            let big_file = n.kind == Kind::File
                && n.alloc >= 1 << 20
                && kinds::classify(tree.name_bytes(id), false) == FileKind::Log;
            let name = tree.name_bytes(id);
            let log_dir =
                n.is_dir() && n.alloc >= 10 << 20 && (name == b"logs" || name == b"log") && id != NodeId::ROOT;
            if !(big_file || log_dir) || self.is_claimed(id) {
                continue;
            }
            // LevelDB and SQLite keep their write-ahead data in `NNNNNN.log`
            // beside `CURRENT` and `LOCK`: that's data, not a log.
            if big_file
                && tree.parent(id).is_some_and(|p| ["CURRENT", "LOCK"].iter().any(|f| tree.child_named(p, f).is_some()))
            {
                continue;
            }
            let path = tree.path(id);
            if crate::nix::in_store(&path) || path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }
            let command = if n.is_dir() {
                format!("find {} -type f -mtime +30 -delete", shell_quote(&path))
            } else {
                format!("truncate -s 0 -- {}", shell_quote(&path))
            };
            let note = if n.is_dir() {
                "log folder: the command removes logs older than 30 days"
            } else {
                "log file: the command empties it"
            };
            found.push((id, self.hint_path(id, Some(note.into()), Some(command))));
        }
        let mut summary = "Programs' logs. Old ones are rarely read again; a program still writing a log keeps writing to the emptied file.".to_owned();
        if Path::new("/var/log/journal").is_dir() {
            summary.push_str(" The system journal is separate: `journalctl --disk-usage` shows it, and `sudo journalctl --vacuum-time=4weeks` trims it.");
        }
        self.push(
            "logs",
            "Logs".into(),
            summary,
            Risk::Low,
            Action { kind: ActionKind::Command, command: None, description: "Run each path's command.".into() },
            found,
        );
    }

    fn old_downloads(&mut self) {
        let Some(dl) = self.cx.downloads.clone() else { return };
        let Some(id) = self.tree.find(&dl) else { return };
        let cutoff = self.cx.now - self.cx.old_after_days * DAY;
        let kids: Vec<NodeId> = self.tree.children(id).collect();
        let found: Vec<_> = kids
            .into_iter()
            .filter(|&c| self.tree.node(c).mtime < cutoff && self.tree.node(c).mtime > 0 && !self.is_claimed(c))
            .map(|c| {
                let n = self.tree.node(c);
                let note = format!(
                    "{}, untouched {}",
                    kinds::classify(self.tree.name_bytes(c), false).title(),
                    time::ago(n.mtime, self.cx.now)
                );
                (c, self.hint_path(c, Some(note), None))
            })
            .collect();
        let paths: Vec<PathBuf> = found.iter().map(|(c, _)| self.tree.path(*c)).collect();
        let command = (!paths.is_empty()).then(|| {
            let mut s = "aizome-kura trash --".to_owned();
            for p in paths.iter().take(5) {
                s.push(' ');
                s.push_str(&shell_quote(p));
            }
            if paths.len() > 5 {
                s.push_str(" …");
            }
            s
        });
        self.push(
            "old-downloads",
            format!("Old downloads in {}", format::path(&dl)),
            format!(
                "Files and folders in Downloads not changed for {} days. Installers, archives and disk images are often done with, but these are the person's files: ask before removing any.",
                self.cx.old_after_days
            ),
            Risk::Medium,
            Action {
                kind: ActionKind::Trash,
                command,
                description: "Show the list to the person; move what they choose to the Trash (reversible).".into(),
            },
            found,
        );
    }

    fn duplicates(&mut self) {
        let tree = self.tree;
        // Files not under a claimed folder, .git, or the store.
        let mut excluded = vec![false; tree.len()];
        let mut files = Vec::new();
        for (i, n) in tree.nodes().iter().enumerate() {
            let id = NodeId(i as u32);
            let parent_out = tree.parent(id).is_some_and(|p| excluded[p.index()]);
            // Files in a Git working tree are the repository's, and files in
            // hidden folders (`~/.config`, `~/.local`, a tool's own) are the
            // programs' that keep them: not copies for a person to tidy.
            let repo = n.is_dir() && tree.child_named(id, ".git").is_some();
            let hidden = n.is_dir() && id != NodeId::ROOT && tree.name_bytes(id).starts_with(b".");
            let out = parent_out || self.claimed[i] || repo || hidden;
            excluded[i] = out;
            if out || n.kind != Kind::File || n.has(flags::HARDLINK) || n.apparent < self.cx.dupe_limits.min_bytes {
                continue;
            }
            let path = tree.path(id);
            if crate::nix::in_store(&path) {
                continue;
            }
            files.push(dupes::Candidate { path, size: n.apparent, alloc: n.alloc, mtime: n.mtime });
        }
        let sets = dupes::find(files, self.cx.dupe_limits);
        if sets.is_empty() {
            return;
        }
        let total: u64 = sets.iter().map(|s| s.reclaimable()).sum();
        let mut paths = Vec::new();
        let mut groups = Vec::new();
        let mut listed = 0;
        for s in &sets {
            for f in s.files.iter().skip(1) {
                if listed < MAX_PATHS {
                    paths.push(HintPath {
                        path: f.path.display().to_string(),
                        allocated_bytes: f.alloc,
                        apparent_bytes: f.size,
                        modified: Some(time::rfc3339(f.mtime)),
                        note: Some(format!("a copy of {}", format::path(&s.files[0].path))),
                        command: None,
                    });
                }
                listed += 1;
            }
            if groups.len() < MAX_PATHS {
                groups.push(DuplicateGroup {
                    bytes_each: s.size,
                    hash: s.hash.clone(),
                    verified: if s.verified { "full" } else { "partial" }.into(),
                    reclaimable_bytes: s.reclaimable(),
                    paths: s.files.iter().map(|f| f.path.display().to_string()).collect(),
                });
            }
        }
        let partial = sets.iter().filter(|s| !s.verified).count();
        let mut summary = format!(
            "{} of files with identical contents, each {} or larger. Keeping one copy of each would free the rest.",
            format::plural(sets.len() as u64, "set", "sets"),
            format::size(self.cx.dupe_limits.min_bytes)
        );
        if partial > 0 {
            summary.push_str(&format!(
                " {} matched on size and both ends only (not every byte was read): compare them before removing one.",
                format::plural(partial as u64, "set", "sets")
            ));
        }
        self.hints.push(Hint {
            id: "duplicates".into(),
            title: "Duplicate files".into(),
            summary,
            reclaimable_bytes: total,
            risk: Risk::Medium,
            action: Action {
                kind: ActionKind::Trash,
                command: None,
                description: "Ask which copy to keep (the first of each group is the oldest); move the others to the Trash with `aizome-kura trash`.".into(),
            },
            paths,
            more_paths: (listed as u64).saturating_sub(MAX_PATHS as u64),
            count: Some(sets.len() as u64),
            count_unit: Some("sets".into()),
            groups,
        });
    }

    fn nix(&mut self) {
        let old: u64 = self.cx.nix_profiles.iter().map(|p| u64::from(p.old())).sum();
        if old > 0 || self.cx.nix_store.is_some() {
            let list = self
                .cx
                .nix_profiles
                .iter()
                .map(|p| format!("{} {}", format::count(u64::from(p.old())), p.name))
                .collect::<Vec<_>>()
                .join(", ");
            let store = self
                .cx
                .nix_store
                .as_ref()
                .map(|s| format!(" The store holds {} in {} paths.", format::size(s.bytes), format::count(s.paths)))
                .unwrap_or_default();
            self.hints.push(Hint {
                id: "nix-generations".into(),
                title: "Old Nix generations".into(),
                summary: format!(
                    "{} kept besides the current ones ({list}). Each keeps its packages in the Nix store.{store} Never delete store paths by hand: Yukimi's Storage removes old generations and collects the garbage safely.",
                    format::plural(old, "old generation is", "old generations are")
                ),
                reclaimable_bytes: 0,
                risk: Risk::Low,
                action: Action {
                    kind: ActionKind::OpenApp,
                    command: Some(nix::yukimi_command_line()),
                    description: "Open Yukimi's Storage view and let the person choose what to remove.".into(),
                },
                paths: self
                    .cx
                    .nix_profiles
                    .iter()
                    .map(|p| HintPath {
                        path: p.path.display().to_string(),
                        allocated_bytes: 0,
                        apparent_bytes: 0,
                        modified: None,
                        note: Some(format!("{} generations, current {}", p.generations, p.current.map_or("unknown".into(), |c| c.to_string()))),
                        command: None,
                    })
                    .collect(),
                more_paths: 0,
                count: Some(old),
                count_unit: Some("generations".into()),
                groups: Vec::new(),
            });
        }
        // `result` links from `nix build` are GC roots.
        let tree = self.tree;
        let mut links = Vec::new();
        for (i, n) in tree.nodes().iter().enumerate() {
            let id = NodeId(i as u32);
            if n.kind != Kind::Symlink {
                continue;
            }
            let name = tree.name_bytes(id);
            if !(name == b"result" || name.starts_with(b"result-")) {
                continue;
            }
            let path = tree.path(id);
            if crate::nix::in_store(&path) || !nix::points_into_store(&path) {
                continue;
            }
            let target = std::fs::read_link(&path).map(|t| t.display().to_string()).unwrap_or_default();
            links.push(HintPath {
                path: path.display().to_string(),
                allocated_bytes: 0,
                apparent_bytes: 0,
                modified: Some(time::rfc3339(n.mtime)),
                note: Some(format!("keeps {target} in the store")),
                command: Some(format!("rm -- {}", shell_quote(&path))),
            });
        }
        if !links.is_empty() {
            let count = links.len() as u64;
            links.truncate(MAX_PATHS);
            self.hints.push(Hint {
                id: "nix-result-links".into(),
                title: "Nix build results".into(),
                summary: format!(
                    "{} left by `nix build` keep their outputs in the Nix store as garbage-collector roots. Removing a link frees nothing by itself; Yukimi's Storage collects the garbage afterwards.",
                    format::plural(count, "`result` link", "`result` links")
                ),
                reclaimable_bytes: 0,
                risk: Risk::Low,
                action: Action {
                    kind: ActionKind::Command,
                    command: Some(nix::yukimi_command_line()),
                    description: "Remove the links no longer wanted (each path's command), then collect garbage in Yukimi's Storage.".into(),
                },
                paths: links,
                more_paths: count.saturating_sub(MAX_PATHS as u64),
                count: Some(count),
                count_unit: Some("links".into()),
                groups: Vec::new(),
            });
        }
    }

    fn containers(&mut self) {
        let mut paths = Vec::new();
        let mut extra = Vec::new();
        let mut tools: Vec<&str> = Vec::new();
        let mut candidates: Vec<(PathBuf, &str)> = vec![
            (PathBuf::from("/var/lib/docker"), "docker"),
            (PathBuf::from("/var/lib/containers/storage"), "podman"),
        ];
        if let Some(d) = &self.cx.data_home {
            candidates.push((d.join("containers/storage"), "podman"));
            candidates.push((d.join("docker"), "docker"));
        }
        for (path, tool) in candidates {
            if !path.is_dir() {
                continue;
            }
            if !tools.contains(&tool) {
                tools.push(tool);
            }
            match self.tree.find(&path) {
                Some(id) if !self.is_claimed(id) => {
                    paths.push((id, self.hint_path(id, Some(format!("{tool} storage")), None)))
                }
                Some(_) => {}
                // Outside the scan (or unreadable): listed, size unknown.
                None => extra.push(HintPath {
                    path: path.display().to_string(),
                    allocated_bytes: 0,
                    apparent_bytes: 0,
                    modified: None,
                    note: Some(format!("{tool} storage, outside this scan: `{tool} system df` measures it")),
                    command: None,
                }),
            }
        }
        if tools.is_empty() {
            return;
        }
        let command = tools.iter().map(|t| format!("{t} image prune")).collect::<Vec<_>>().join("; ");
        let summary = format!(
            "Container images and layers. `{0} system df` shows what's in use; `{0} image prune` removes dangling images (low risk), `{0} system prune -a` also removes stopped containers and unused images (they'd be pulled again).",
            tools[0]
        );
        self.push(
            "containers",
            "Container storage".into(),
            summary.clone(),
            Risk::Medium,
            Action {
                kind: ActionKind::Command,
                command: Some(command.clone()),
                description: "Ask before pruning: stopped containers and unused images go.".into(),
            },
            paths,
        );
        if !extra.is_empty() {
            match self.hints.iter_mut().find(|h| h.id == "containers") {
                Some(h) => h.paths.extend(extra),
                None => self.hints.push(Hint {
                    id: "containers".into(),
                    title: "Container storage".into(),
                    summary,
                    reclaimable_bytes: 0,
                    risk: Risk::Medium,
                    action: Action {
                        kind: ActionKind::Command,
                        command: Some(command),
                        description: "Measure with `system df` first; ask before pruning.".into(),
                    },
                    paths: extra,
                    more_paths: 0,
                    count: None,
                    count_unit: None,
                    groups: Vec::new(),
                }),
            }
        }
    }

    fn flatpak(&mut self) {
        let mut paths = Vec::new();
        if let Some(d) = &self.cx.data_home {
            for id in self.glob(&d.join("flatpak")) {
                if !self.is_claimed(id) && self.tree.node(id).alloc >= 100 << 20 {
                    paths.push((id, self.hint_path(id, Some("your Flatpak apps and runtimes".into()), None)));
                }
            }
        }
        if paths.is_empty() {
            return;
        }
        self.push(
            "flatpak",
            "Flatpak runtimes".into(),
            "Flatpak keeps runtimes that no installed app may need any more. `flatpak uninstall --unused` removes only those; this size is all of Flatpak's, not what that frees.".into(),
            Risk::Low,
            Action { kind: ActionKind::Command, command: Some("flatpak uninstall --unused".into()), description: "Run the command; it lists what it removes and asks.".into() },
            paths,
        );
        // Not all of it is reclaimable: say so in the number too.
        if let Some(h) = self.hints.last_mut()
            && h.id == "flatpak"
        {
            h.reclaimable_bytes = 0;
        }
    }
}

/// Whether a CACHEDIR.TAG starts with the convention's signature.
fn cachedir_tag_valid(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 43];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut buf)).is_ok()
        && &buf == b"Signature: 8a477f597d28d172789f06886806bc55"
}

/// Reclaimable bytes by risk, for the index's summary.
pub fn by_risk(hints: &[Hint]) -> HashMap<Risk, u64> {
    let mut m = HashMap::new();
    for h in hints {
        *m.entry(h.risk).or_insert(0) += h.reclaimable_bytes;
    }
    m
}
