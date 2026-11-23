//! `aizome-tv-account`: the TV account's changes, as root. Settings runs it
//! through pkexec under its own polkit action (an administrator's
//! password), with what to do in its arguments and a PIN or password on
//! its input (`aizome_sys::tv`):
//!
//! - `set none|pin|password`: make the account if there's none (`tv`,
//!   "TV", a home folder, nologin for a shell, so only the login screen
//!   lets it in), then have it sign in with that. Its password is set as a
//!   hash (no PAM stack runs), or emptied for none, which the login
//!   screen's PAM service accepts (`nullok`).
//! - `remove [--files]`: remove the account, its home folder too, and the
//!   group made with it, so it can be made again.
//! - `forget-chanoma-pin`: forget the PIN of the account's Chanoma (its
//!   grown-ups' PIN), as `aizome-chanoma --reset-pin` run as the account.
//! - `share-drives`: have the folder the account's drives are mounted in
//!   (`/run/media/tv`) there for every account to look in while there is a
//!   TV account, and gone while there's none. systemd runs it, at boot and
//!   whenever the record changes, where the drives are shared
//!   (`programs.aizome.chanoma.shareTvDrives`); Settings doesn't.
//!
//! It runs only what's named here, found in the system's own folders, and
//! keeps the account's record at `/var/lib/aizome/tv-account.toml`.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use aizome_sys::tv::{self, Auth, TvAccount};

// A shadow password beginning with '!' is locked. This particular value
// identifies a useradd completed by this helper before its record was saved.
const PENDING: &str = "!aizome-tv-account-pending";
const SSH_GROUP: &str = "aizome-tv-kiosk";

fn main() {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        fail("this runs as root, through pkexec");
    }
    // Two Settings windows must not interleave account, credential and record changes.
    let lock = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open("/run/aizome-tv-account.lock")
        .unwrap_or_else(|e| fail(&e.to_string()));
    lock.lock().unwrap_or_else(|e| fail(&e.to_string()));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["set", auth] => match Auth::parse(auth) {
            Some(auth) => set(auth, &secret()),
            None => Err(format!("no way to sign in called {auth}")),
        },
        ["remove"] => remove(false),
        ["remove", "--files"] => remove(true),
        ["forget-chanoma-pin"] => forget_chanoma_pin(),
        ["share-drives"] => share_drives(Path::new(MEDIA)),
        _ => {
            Err("usage: aizome-tv-account set none|pin|password | remove [--files] | forget-chanoma-pin | share-drives"
                .into())
        }
    };
    if let Err(e) = result {
        fail(&e);
    }
}

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(1);
}

/// The PIN or password, from the input (not the arguments, which anyone
/// can read).
fn secret() -> String {
    let mut s = String::new();
    let _ = std::io::stdin().take(4096).read_to_string(&mut s);
    s
}

/// A system program, from the system's folders only (pkexec's PATH is
/// root's, but nothing here relies on it).
fn tool(name: &str) -> Result<PathBuf, String> {
    ["/run/current-system/sw/bin", "/run/current-system/sw/sbin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{name} isn't installed"))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(())
    } else {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if why.is_empty() { format!("{:?} failed ({})", cmd.get_program(), out.status) } else { why })
    }
}

/// Whether an account `name` exists.
fn exists(name: &str) -> Result<bool, String> {
    let passwd = std::fs::read_to_string("/etc/passwd").map_err(|e| format!("/etc/passwd: {e}"))?;
    Ok(passwd.lines().any(|l| l.split(':').next() == Some(name)))
}

fn pending(name: &str) -> Result<bool, String> {
    let shadow = std::fs::read_to_string("/etc/shadow").map_err(|e| format!("/etc/shadow: {e}"))?;
    Ok(shadow.lines().any(|line| {
        let mut fields = line.split(':');
        fields.next() == Some(name) && fields.next() == Some(PENDING)
    }))
}

fn set(auth: Auth, secret: &str) -> Result<(), String> {
    let change = tv::Change::Set { auth, secret: secret.to_owned() };
    change.check().map_err(|e| e.to_string())?;
    let current = TvAccount::read();
    let user = current.as_ref().map_or(tv::NAME, |t| t.user.as_str()).to_owned();
    let hash = match auth {
        Auth::None => String::new(),
        Auth::Pin | Auth::Password => aizome_sys::users::crypt_password(secret).map_err(|e| e.to_string())?,
    };
    let usermod = tool("usermod")?;
    if !exists(&user)? {
        let nologin = tool("nologin")?;
        run(Command::new(tool("useradd")?)
            .args(["--create-home", "--user-group", "--groups", SSH_GROUP, "--password", PENDING])
            .args(["--comment", tv::REAL_NAME, "--shell"])
            .arg(nologin)
            .arg(&user))?;
    } else if current.is_none() && !pending(&user)? {
        return Err(format!("There's already an account called {user}, and it isn't the TV account"));
    }

    // Publish the kiosk identity before installing any usable credential.
    // If writing it fails on creation, the locked marker lets the next
    // attempt finish without claiming an unrelated account named tv.
    let record = TvAccount { user, auth, pin_length: (auth == Auth::Pin).then_some(secret.len() as u8) };
    write(&record)?;
    let install =
        run(Command::new(usermod).args(["--append", "--groups", SSH_GROUP, "--password", &hash, &record.user]));
    if install.is_err() {
        // An existing account still authenticates with its former credential.
        if let Some(previous) = current {
            write(&previous)?;
        }
    }
    install
}

/// The record, whole: a new file renamed over the old, readable by all.
fn write(record: &TvAccount) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let path = Path::new(tv::PATH);
    let dir = path.parent().ok_or("no folder for the record")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    let new = path.with_extension("new");
    let mut file = File::create(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    file.write_all(record.to_toml().as_bytes()).map_err(|e| e.to_string())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&new, path).map_err(|e| format!("{}: {e}", path.display()))?;
    File::open(dir).and_then(|d| d.sync_all()).map_err(|e| format!("{}: {e}", dir.display()))
}

fn remove(files: bool) -> Result<(), String> {
    let Some(record) = TvAccount::read() else { return Ok(()) };
    if exists(&record.user)? {
        let mut cmd = Command::new(tool("userdel")?);
        if files {
            cmd.arg("--remove");
        }
        run(cmd.arg(&record.user))?;
    }
    // userdel leaves the group made with the account (`--user-group`)
    // unless login.defs says USERGROUPS_ENAB, which NixOS's doesn't, and
    // useradd won't make an account whose group is there already: left,
    // the TV account could be removed once and never made again.
    let read = |path: &str| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    if nobodys_group(&read("/etc/group")?, &read("/etc/passwd")?, &record.user) {
        run(Command::new(tool("groupdel")?).arg(&record.user))?;
    }
    std::fs::remove_file(tv::PATH).map_err(|e| format!("{}: {e}", tv::PATH))
}

/// Whether `group` (the text of `/etc/group`) has a group `name` that
/// nobody is in: no members, and no account in `passwd` has it for its own.
fn nobodys_group(group: &str, passwd: &str, name: &str) -> bool {
    let Some(line) = group.lines().find(|l| l.split(':').next() == Some(name)) else { return false };
    let mut fields = line.split(':').skip(2);
    let (Some(gid), members) = (fields.next(), fields.next().unwrap_or("")) else { return false };
    members.trim().is_empty() && !passwd.lines().any(|l| l.split(':').nth(3) == Some(gid))
}

/// Where UDisks mounts an account's drives, each account's in a folder of
/// its name.
const MEDIA: &str = "/run/media";

/// The TV account's folder in `media`, there for every account to look in.
/// UDisks keeps to a folder that's there already, and makes one only its
/// account may open otherwise.
///
/// For the account in the record and no other: an ordinary account that
/// happens to be called `tv` keeps its drives to itself. So with no TV
/// account the folder made for one goes, unless something is still mounted
/// in it.
fn share_drives(media: &Path) -> Result<(), String> {
    let account = match TvAccount::read() {
        Some(record) if exists(&record.user)? => Some(record.user),
        _ => None,
    };
    share_drives_of(media, account.as_deref())
}

fn share_drives_of(media: &Path, account: Option<&str>) -> Result<(), String> {
    use std::io::ErrorKind;
    use std::os::unix::fs::PermissionsExt;
    let Some(user) = account else {
        return match std::fs::remove_dir(media.join(tv::NAME)) {
            Err(e) if !matches!(e.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) => {
                Err(format!("{}: {e}", media.join(tv::NAME).display()))
            }
            _ => Ok(()),
        };
    };
    for dir in [media.to_owned(), media.join(user)] {
        match std::fs::create_dir(&dir) {
            Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(format!("{}: {e}", dir.display())),
            _ => {}
        }
        // Whatever made it, and whatever this process's mask is.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

fn forget_chanoma_pin() -> Result<(), String> {
    let record = TvAccount::read().ok_or("There's no TV account")?;
    run(Command::new(tool("runuser")?).args(["-u", &record.user, "--"]).arg(tool("aizome-chanoma")?).arg("--reset-pin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_drives_folder_is_everyones_only_while_there_is_a_tv_account() {
        let media = std::env::temp_dir().join(format!("tv-account-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&media);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        let tv = media.join(tv::NAME);
        // No TV account: nothing is made.
        share_drives_of(&media, None).unwrap();
        assert!(!media.exists());
        // One: its folder, for every account to look in, whatever made it.
        share_drives_of(&media, Some(tv::NAME)).unwrap();
        assert_eq!((mode(&media), mode(&tv)), (0o755, 0o755));
        std::fs::set_permissions(&tv, std::fs::Permissions::from_mode(0o700)).unwrap();
        share_drives_of(&media, Some(tv::NAME)).unwrap();
        assert_eq!(mode(&tv), 0o755);
        // None again: the folder goes, but not with a drive still in it.
        std::fs::create_dir(tv.join("STICK")).unwrap();
        share_drives_of(&media, None).unwrap();
        assert!(tv.join("STICK").exists());
        std::fs::remove_dir(tv.join("STICK")).unwrap();
        share_drives_of(&media, None).unwrap();
        assert!(!tv.exists() && media.exists());
        std::fs::remove_dir_all(&media).unwrap();
    }

    #[test]
    fn the_group_left_by_the_removed_account_is_nobodys() {
        let passwd = "root:x:0:0::/root:/bin/sh\nalice:x:1000:100:Alice:/home/alice:/bin/sh\n";
        let group = "root:x:0:\nusers:x:100:\ntv:x:1001:\nwheel:x:1:alice\n";
        assert!(nobodys_group(group, passwd, "tv"));
        // Not one with a member, an account's own, or one that isn't there.
        assert!(!nobodys_group(group, passwd, "wheel"));
        assert!(!nobodys_group(group, passwd, "users"));
        assert!(!nobodys_group(group, passwd, "television"));
        assert!(!nobodys_group("tv:x:1001:alice\n", passwd, "tv"));
        assert!(!nobodys_group(group, "tv:x:1001:1001:TV:/home/tv:/bin/nologin\n", "tv"));
    }
}
