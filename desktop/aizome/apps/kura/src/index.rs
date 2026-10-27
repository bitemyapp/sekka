//! The index for agents: `$XDG_CACHE_HOME/aizome/kura/index.json`, written
//! after every scan, in a stable, versioned format (`docs/kura-index.md`).
//!
//! Version 1. Fields are only ever added within a version; a change that
//! would break a reader bumps `version`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hints::{self, Hint, Risk};
use crate::kinds::{self, FileKind};
use crate::mounts;
use crate::nix;
use crate::scan::Scan;
use crate::time;
use crate::tree::{Kind, NodeId, Tree, flags};

pub const SCHEMA: &str = "org.aizome.kura.index";
pub const VERSION: u32 = 1;

/// How much of the tree the index keeps.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeLimits {
    /// Levels below the root.
    pub max_depth: u32,
    /// Children listed per folder at most (largest first).
    pub max_children: u32,
    /// Children smaller than this aren't listed (they're summed in `omitted`).
    pub min_bytes: u64,
}

impl TreeLimits {
    /// Four levels, 25 children each, nothing under 0.1% of the root or 1 MB.
    pub fn for_total(total: u64) -> TreeLimits {
        TreeLimits { max_depth: 4, max_children: 25, min_bytes: (total / 1000).max(1_000_000) }
    }
}

/// The whole index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    pub schema: String,
    pub version: u32,
    pub generator: String,
    /// When the scan started, RFC 3339 (UTC) and Unix time.
    pub scanned_at: String,
    pub scanned_at_unix: i64,
    pub duration_ms: u64,
    pub root: String,
    pub options: ScanInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<FilesystemInfo>,
    pub totals: Totals,
    /// What can be freed, by risk (bytes), and in all.
    pub reclaimable: Reclaimable,
    pub hints: Vec<Hint>,
    pub largest_files: Vec<Item>,
    pub largest_directories: Vec<Item>,
    pub kinds: Vec<KindTotal>,
    pub tree_limits: TreeLimits,
    pub tree: TreeNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nix: Option<NixInfo>,
    /// Folders on other filesystems the scan didn't enter.
    pub other_filesystems: Vec<String>,
    /// The first errors (all are counted in `totals.errors`).
    pub errors: Vec<ErrorInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanInfo {
    pub one_filesystem: bool,
    pub threads: usize,
    /// What `allocated_bytes` means: `st_blocks × 512`, what removing frees.
    pub size_measure: String,
    pub hard_links: String,
    pub symlinks: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilesystemInfo {
    pub mount_point: String,
    pub source: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Totals {
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
    pub files: u64,
    pub directories: u64,
    pub symlinks: u64,
    pub other: u64,
    /// Files with more than one name, counted once.
    pub hard_links_counted_once: u64,
    pub errors: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Reclaimable {
    pub total_bytes: u64,
    /// `safe`, `low`, `medium`, `high` → bytes.
    pub by_risk: BTreeMap<String, u64>,
}

/// A file or folder in the largest lists.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
    /// For a folder, the newest modification inside it.
    pub modified: String,
    /// A file's kind (`video`, `disk-image`…); `directory` for folders.
    pub kind: String,
    /// Files and folders inside (folders only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KindTotal {
    pub kind: FileKind,
    pub title: String,
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
    pub files: u64,
}

/// A node of the tree, to `tree_limits.max_depth`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    /// `directory`, `file`, `symlink` or `other`.
    #[serde(rename = "type")]
    pub node_type: String,
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
    pub modified: String,
    /// Files and folders inside (folders only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
    /// `unreadable`, `other-filesystem`, `hard-link`, `skipped`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
    /// Children not listed (too small, too many, or too deep).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<Omitted>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Omitted {
    pub count: u64,
    pub allocated_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NixInfo {
    pub store: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_paths: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_measured_with: Option<String>,
    pub profiles: Vec<nix::Profile>,
    /// What to run instead of deleting store paths.
    pub cleanup_command: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub path: String,
    pub error: String,
}

fn type_name(k: Kind) -> &'static str {
    match k {
        Kind::Dir => "directory",
        Kind::File => "file",
        Kind::Symlink => "symlink",
        Kind::Other => "other",
    }
}

fn flag_names(f: u8) -> Vec<String> {
    let mut v = Vec::new();
    for (bit, name) in [
        (flags::ERROR, "unreadable"),
        (flags::MOUNT, "other-filesystem"),
        (flags::HARDLINK, "hard-link"),
        (flags::SKIPPED, "skipped"),
    ] {
        if f & bit != 0 {
            v.push(name.to_owned());
        }
    }
    v
}

/// Per-file facts the index needs that the tree doesn't keep: whether each
/// node is inside a `.git` folder.
fn in_git(tree: &Tree) -> Vec<bool> {
    let mut v = vec![false; tree.len()];
    for i in 1..tree.len() {
        let id = NodeId(i as u32);
        let p = tree.node(id).parent as usize;
        v[i] = v[p] || (tree.node(id).is_dir() && tree.name_bytes(id) == b".git");
    }
    v
}

fn tree_node(tree: &Tree, id: NodeId, depth: u32, limits: &TreeLimits) -> TreeNode {
    let n = tree.node(id);
    let mut children = Vec::new();
    let mut omitted = None;
    if n.is_dir() && n.count > 0 {
        let mut shown = 0u32;
        let (mut skipped, mut skipped_bytes) = (0u64, 0u64);
        for c in tree.sorted_children(id) {
            let cn = tree.node(c);
            if depth < limits.max_depth && shown < limits.max_children && cn.alloc >= limits.min_bytes {
                children.push(tree_node(tree, c, depth + 1, limits));
                shown += 1;
            } else {
                skipped += 1;
                skipped_bytes += cn.alloc;
            }
        }
        if skipped > 0 {
            omitted = Some(Omitted { count: skipped, allocated_bytes: skipped_bytes });
        }
    }
    TreeNode {
        name: if id == NodeId::ROOT { tree.root_path().display().to_string() } else { tree.name(id).into_owned() },
        path: tree.path(id).display().to_string(),
        node_type: type_name(n.kind).into(),
        allocated_bytes: n.alloc,
        apparent_bytes: n.apparent,
        modified: time::rfc3339(n.mtime),
        items: n.is_dir().then_some(u64::from(n.items)),
        flags: flag_names(n.flags),
        children,
        omitted,
    }
}

/// The `n` largest nodes `keep` accepts, by allocated size.
fn largest(tree: &Tree, n: usize, keep: impl Fn(NodeId) -> bool) -> Vec<NodeId> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(n + 1);
    for (i, node) in tree.nodes().iter().enumerate() {
        if heap.len() == n && heap.peek().is_some_and(|Reverse((s, _))| *s >= node.alloc) {
            continue;
        }
        let id = NodeId(i as u32);
        if !keep(id) {
            continue;
        }
        heap.push(Reverse((node.alloc, i as u32)));
        if heap.len() > n {
            heap.pop();
        }
    }
    let mut v: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(x)| x).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|(_, i)| NodeId(i)).collect()
}

/// Build the index of `scan`, with `hints` already found.
pub fn build(scan: &Scan, hints: Vec<Hint>, cx: &hints::Context, limits: Option<TreeLimits>) -> Index {
    let tree = &scan.tree;
    let root = tree.node(NodeId::ROOT);
    let limits = limits.unwrap_or_else(|| TreeLimits::for_total(root.alloc));
    let git = in_git(tree);
    let gone = tree.gone();

    let mut totals = Totals {
        allocated_bytes: root.alloc,
        apparent_bytes: root.apparent,
        hard_links_counted_once: scan.hard_links,
        errors: scan.error_count,
        ..Totals::default()
    };
    let mut kinds: BTreeMap<FileKind, KindTotal> = BTreeMap::new();
    for (i, n) in tree.nodes().iter().enumerate() {
        if gone[i] {
            continue;
        }
        match n.kind {
            Kind::Dir => totals.directories += 1,
            Kind::Symlink => totals.symlinks += 1,
            Kind::Other => totals.other += 1,
            Kind::File => {
                totals.files += 1;
                let k = kinds::classify(tree.name_bytes(NodeId(i as u32)), git[i]);
                let e = kinds.entry(k).or_insert_with(|| KindTotal {
                    kind: k,
                    title: k.title().into(),
                    allocated_bytes: 0,
                    apparent_bytes: 0,
                    files: 0,
                });
                e.allocated_bytes += n.alloc;
                e.apparent_bytes += n.apparent;
                e.files += 1;
            }
        }
    }
    let mut kinds: Vec<KindTotal> = kinds.into_values().collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(k.allocated_bytes));

    let item = |id: NodeId| {
        let n = tree.node(id);
        Item {
            path: tree.path(id).display().to_string(),
            allocated_bytes: n.alloc,
            apparent_bytes: n.apparent,
            modified: time::rfc3339(n.mtime),
            kind: if n.is_dir() {
                "directory".into()
            } else {
                kinds::classify(tree.name_bytes(id), git[id.index()]).id().into()
            },
            items: n.is_dir().then_some(u64::from(n.items)),
        }
    };
    let largest_files: Vec<Item> =
        largest(tree, 500, |id| tree.node(id).kind == Kind::File && !gone[id.index()]).into_iter().map(item).collect();
    let largest_directories: Vec<Item> =
        largest(tree, 500, |id| id != NodeId::ROOT && tree.node(id).is_dir() && !gone[id.index()])
            .into_iter()
            .map(item)
            .collect();

    let by = hints::by_risk(&hints);
    let reclaimable = Reclaimable {
        total_bytes: by.values().sum(),
        by_risk: [Risk::Safe, Risk::Low, Risk::Medium, Risk::High]
            .into_iter()
            .map(|r| (r.id().to_owned(), by.get(&r).copied().unwrap_or(0)))
            .collect(),
    };

    let filesystem = mounts::mount_of(tree.root_path()).map(|m| FilesystemInfo {
        mount_point: m.mount_point.display().to_string(),
        used_bytes: m.used_bytes(),
        source: m.source,
        fs_type: m.fs_type,
        total_bytes: m.total_bytes,
        free_bytes: m.free_bytes,
        available_bytes: m.available_bytes,
    });

    let nix = nix::has_store().then(|| NixInfo {
        store: nix::STORE.into(),
        store_bytes: cx.nix_store.as_ref().map(|s| s.bytes),
        store_paths: cx.nix_store.as_ref().map(|s| s.paths),
        store_measured_with: cx.nix_store.as_ref().map(|s| s.source.clone()),
        profiles: cx.nix_profiles.clone(),
        cleanup_command: nix::yukimi_command_line(),
        note: "Never delete paths in /nix/store directly: remove old generations and collect garbage through Yukimi's Storage view.".into(),
    });

    Index {
        schema: SCHEMA.into(),
        version: VERSION,
        generator: format!("aizome-kura {}", env!("CARGO_PKG_VERSION")),
        scanned_at: time::rfc3339(scan.started),
        scanned_at_unix: scan.started,
        duration_ms: scan.duration.as_millis() as u64,
        root: tree.root_path().display().to_string(),
        options: ScanInfo {
            one_filesystem: !scan.options.cross_filesystems,
            threads: scan.threads,
            size_measure: "allocated_bytes is st_blocks × 512 (what removing frees); apparent_bytes is st_size".into(),
            hard_links: "a file with several names is counted once, at the first name seen".into(),
            symlinks: "never followed; a symlink counts its own size".into(),
        },
        filesystem,
        totals,
        reclaimable,
        hints,
        largest_files,
        largest_directories,
        kinds,
        tree_limits: limits,
        tree: tree_node(tree, NodeId::ROOT, 0, &limits),
        nix,
        other_filesystems: scan.other_filesystems.iter().map(|p| p.display().to_string()).collect(),
        errors: scan
            .errors
            .iter()
            .take(50)
            .map(|e| ErrorInfo { path: e.path.display().to_string(), error: e.error.clone() })
            .collect(),
    }
}

/// Where the index goes: `$XDG_CACHE_HOME/aizome/kura`.
pub fn default_dir() -> Option<PathBuf> {
    crate::xdg_dir("XDG_CACHE_HOME", ".cache").map(|d| d.join("aizome/kura"))
}

/// Write `index.json` and `index.md` into `dir`, each replaced atomically.
pub fn write(index: &Index, dir: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let json = dir.join("index.json");
    let md = dir.join("index.md");
    let text = serde_json::to_string_pretty(index).map_err(std::io::Error::other)?;
    crate::write_atomic(&json, text.as_bytes())?;
    crate::write_atomic(&md, crate::report::markdown(index).as_bytes())?;
    Ok((json, md))
}

/// Read an index back.
pub fn read(path: &Path) -> std::io::Result<Index> {
    let text = std::fs::read_to_string(path)?;
    let index: Index = serde_json::from_str(&text).map_err(std::io::Error::other)?;
    if index.schema != SCHEMA || index.version > VERSION {
        return Err(std::io::Error::other(format!(
            "{} is {} version {}, which this Kura doesn't read",
            path.display(),
            index.schema,
            index.version
        )));
    }
    Ok(index)
}
