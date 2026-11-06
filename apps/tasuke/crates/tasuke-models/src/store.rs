//! Downloaded models: `~/.local/share/tasuke/models/<model-id>/<file>`.
//!
//! A file has its final name only once its sha256 has matched the catalog
//! (downloads write `<file>.partial` and rename it), so a file with its final
//! name and the catalog's size is a verified one. [`Installed`] is the proof
//! that every file of an entry is there; only [`Store::installed`] makes one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{Entry, FileKind, ModelFile};
use crate::config::Paths;
use crate::units::{Bytes, ModelId};

pub const PARTIAL_SUFFIX: &str = ".partial";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum InstallState {
    /// Nothing downloaded.
    Absent,
    /// Some bytes are here: `have` of `total`.
    Partial { have: Bytes, total: Bytes },
    /// Every file is here and verified.
    Installed { size: Bytes },
}

/// Every file of a model is in the store, verified. Made only by
/// [`Store::installed`] (and by a finished download).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Installed {
    id: ModelId,
    weights: Vec<PathBuf>,
    mmproj: Option<PathBuf>,
    draft: Option<PathBuf>,
}

impl Installed {
    pub fn id(&self) -> &ModelId {
        &self.id
    }

    /// The first (or only) weights file: what llama-server's `-m` takes.
    pub fn weights(&self) -> &Path {
        &self.weights[0]
    }

    pub fn mmproj(&self) -> Option<&Path> {
        self.mmproj.as_deref()
    }

    /// The draft head, when it is here too.
    pub fn draft(&self) -> Option<&Path> {
        self.draft.as_deref()
    }
}

/// The files a model can't run without: all but its draft head, which only
/// makes it faster. A model downloaded before its draft was catalogued runs
/// as it did; downloading it again fetches the draft.
fn required(entry: &Entry) -> impl Iterator<Item = &ModelFile> {
    entry.files.iter().filter(|f| f.kind != FileKind::Draft)
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    pub fn from_paths(paths: &Paths) -> Store {
        Store::new(paths.models_dir())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn model_dir(&self, id: &ModelId) -> PathBuf {
        assert!(id.is_valid(), "catalog ids are validated");
        self.root.join(id.as_str())
    }

    pub fn file_path(&self, id: &ModelId, file: &ModelFile) -> PathBuf {
        self.model_dir(id).join(file.file_name())
    }

    pub fn partial_path(&self, id: &ModelId, file: &ModelFile) -> PathBuf {
        let mut p = self.file_path(id, file).into_os_string();
        p.push(PARTIAL_SUFFIX);
        PathBuf::from(p)
    }

    /// Whether `file` is complete (final name, catalog size).
    pub fn has_file(&self, id: &ModelId, file: &ModelFile) -> bool {
        fs::metadata(self.file_path(id, file)).is_ok_and(|m| m.is_file() && m.len() == file.size.0)
    }

    /// Whether every file the catalog lists is here, the draft head included:
    /// what a download has left to do, where [`Store::state`] answers whether
    /// the model runs.
    pub fn complete(&self, entry: &Entry) -> bool {
        entry.files.iter().all(|f| self.has_file(&entry.id, f))
    }

    pub fn state(&self, entry: &Entry) -> InstallState {
        let total = entry.download_size();
        let mut have = Bytes::ZERO;
        for f in &entry.files {
            if self.has_file(&entry.id, f) {
                have += f.size;
            } else if let Ok(m) = fs::metadata(self.partial_path(&entry.id, f)) {
                have += Bytes(m.len().min(f.size.0));
            }
        }
        let complete = required(entry).all(|f| self.has_file(&entry.id, f));
        let on_disk = || entry.files.iter().filter(|f| self.has_file(&entry.id, f)).map(|f| f.size).sum();
        match (complete, have) {
            (true, _) => InstallState::Installed { size: on_disk() },
            (false, Bytes::ZERO) => InstallState::Absent,
            (false, have) => InstallState::Partial { have, total },
        }
    }

    /// The proof that `entry` is installed, or `None`.
    pub fn installed(&self, entry: &Entry) -> Option<Installed> {
        if !required(entry).all(|f| self.has_file(&entry.id, f)) {
            return None;
        }
        let paths = |kind| entry.files.iter().filter(move |f| f.kind == kind).map(|f| self.file_path(&entry.id, f));
        Some(Installed {
            id: entry.id.clone(),
            weights: paths(FileKind::Weights).collect(),
            mmproj: paths(FileKind::Mmproj).next(),
            draft: entry.draft_file().filter(|f| self.has_file(&entry.id, f)).map(|f| self.file_path(&entry.id, f)),
        })
    }

    /// Delete a model's files, finished and partial. Removing an absent model
    /// is not an error.
    pub fn remove(&self, id: &ModelId) -> io::Result<()> {
        match fs::remove_dir_all(self.model_dir(id)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }

    /// Free space where models are stored (or the nearest existing parent).
    pub fn free_space(&self) -> io::Result<Bytes> {
        let mut dir = self.root.as_path();
        while !dir.exists() {
            dir = dir.parent().unwrap_or(Path::new("/"));
        }
        free_space(dir)
    }
}

pub(crate) fn free_space(dir: &Path) -> io::Result<Bytes> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c is a valid NUL-terminated path and st a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Bytes((st.f_bavail as u64).saturating_mul(st.f_frsize as u64)))
}
