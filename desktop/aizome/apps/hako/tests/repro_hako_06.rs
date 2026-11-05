//! HAKO-06: in a move across filesystems, a nested folder whose copy
//! "couldn't be written safely" (its `fsync` failed) must keep its
//! source files: the module's promise is that a move deletes only what
//! arrived safely.
//!
//! Two faults are injected into this test binary only, by defining the
//! C symbols it links against (the executable's own definitions win
//! over libc's):
//! - `renameat2` fails with EXDEV for the moved folder, so the move is
//!   a copy across "filesystems" while everything stays in TMPDIR;
//! - `fsync` fails with EIO on the copied nested folder `inner`, as a
//!   flaky USB stick would.
//!
//! Everything else goes to the real system calls.

use std::ffi::{CStr, OsStr};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hako::fsops::transfer::{Mode, transfer};
use hako::fsops::trash::Trash;
use hako::fsops::{Always, Control, Env, ErrorChoice, Resolution};

/// The source whose rename is refused as crossing filesystems.
static EXDEV_FROM: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Fail `fsync` on folders of this name inside a partial copy.
static EIO_DIR: Mutex<Option<String>> = Mutex::new(None);
static EIO_HITS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn set_errno(e: libc::c_int) {
    unsafe { *libc::__errno_location() = e };
}

/// # Safety
///
/// As `renameat2(2)`: both paths are NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renameat2(
    olddirfd: libc::c_int,
    oldpath: *const libc::c_char,
    newdirfd: libc::c_int,
    newpath: *const libc::c_char,
    flags: libc::c_uint,
) -> libc::c_int {
    let old = unsafe { CStr::from_ptr(oldpath) };
    if let Some(from) = EXDEV_FROM.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && Path::new(OsStr::from_bytes(old.to_bytes())) == from
    {
        set_errno(libc::EXDEV);
        return -1;
    }
    unsafe { libc::syscall(libc::SYS_renameat2, olddirfd, oldpath, newdirfd, newpath, flags) as libc::c_int }
}

#[unsafe(no_mangle)]
pub extern "C" fn fsync(fd: libc::c_int) -> libc::c_int {
    if let Some(name) = EIO_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
        && let Ok(p) = fs::read_link(format!("/proc/self/fd/{fd}"))
        && p.file_name() == Some(OsStr::new(&name))
        && p.parent().and_then(Path::file_name).is_some_and(|n| n.as_bytes().windows(11).any(|w| w == b".hako-part-"))
    {
        EIO_HITS.lock().unwrap_or_else(|e| e.into_inner()).push(p);
        set_errno(libc::EIO);
        return -1;
    }
    unsafe { libc::syscall(libc::SYS_fsync, fd) as libc::c_int }
}

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_nested_folder_not_written_safely_keeps_its_sources() {
    let root = std::env::temp_dir().join(format!("hako-repro-06-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let t = Tmp(root.clone());
    let src = root.join("from/src");
    fs::create_dir_all(src.join("inner")).unwrap();
    fs::write(src.join("top.txt"), "top").unwrap();
    fs::write(src.join("inner/x.txt"), "x").unwrap();
    fs::write(src.join("inner/y.txt"), "y").unwrap();
    let dest = root.join("to");
    fs::create_dir_all(&dest).unwrap();
    let (src, dest) = (fs::canonicalize(&src).unwrap(), fs::canonicalize(&dest).unwrap());

    *EXDEV_FROM.lock().unwrap() = Some(src.clone());
    *EIO_DIR.lock().unwrap() = Some("inner".into());
    let control = Control::new();
    let mut asker = Always(Resolution::Cancel, ErrorChoice::Skip);
    let r = transfer(
        std::slice::from_ref(&src),
        &dest,
        Mode::Move,
        &mut Env::new(&control, &mut asker, &mut ()),
        &Trash {
            home: root.join("data/Trash"),
            home_dir: None,
            uid: unsafe { libc::getuid() },
            topdirs: Some(Vec::new()),
        },
    );
    *EXDEV_FROM.lock().unwrap() = None;
    *EIO_DIR.lock().unwrap() = None;

    eprintln!("report: {r:?}");
    eprintln!("fsync failed on: {:?}", EIO_HITS.lock().unwrap());
    assert!(!EIO_HITS.lock().unwrap().is_empty(), "the fault was injected (fsync of the copied inner folder)");
    let unsafe_inner = r.problems.iter().any(|(p, why)| p == &src.join("inner") && why.contains("written safely"));
    assert!(unsafe_inner, "the report says the inner folder couldn't be written safely: {r:?}");
    let left: Vec<String> = fs::read_dir(src.join("inner"))
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    assert!(
        src.join("inner/x.txt").exists() && src.join("inner/y.txt").exists(),
        "the source files of a folder reported as not written safely must stay; src/inner now holds {left:?}, \
         dest holds x.txt: {}",
        dest.join("src/inner/x.txt").exists()
    );
    // What did arrive safely moved; the unsafe copy didn't come.
    assert!(dest.join("src/top.txt").exists() && !src.join("top.txt").exists(), "top.txt moved");
    assert!(!dest.join("src/inner").exists(), "the copy that couldn't be written safely was taken away");
    drop(t);
}
