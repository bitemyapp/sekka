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
//! - `follow UID`: as the account with that user id starts, if it's the TV
//!   account, have it use the displays and the sound output of the account
//!   that changed them last. systemd runs it before the account's own
//!   systemd (`programs.aizome.chanoma.enable`); Settings doesn't.
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
        ["follow", uid] => follow(uid),
        _ => {
            Err("usage: aizome-tv-account set none|pin|password | remove [--files] | forget-chanoma-pin | share-drives | follow UID"
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

// The TV account following another's displays and sound.

/// An account in /etc/passwd.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Account {
    name: String,
    uid: u32,
    gid: u32,
    home: PathBuf,
}

/// The people's accounts in `passwd` (user ids 1000 to 59999, as
/// useradd gives them).
fn accounts(passwd: &str) -> Vec<Account> {
    passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let (uid, gid) = (f.get(2)?.parse::<u32>().ok()?, f.get(3)?.parse::<u32>().ok()?);
            ((1000..60000).contains(&uid) && !f.get(5)?.is_empty()).then(|| Account {
                name: f[0].to_owned(),
                uid,
                gid,
                home: PathBuf::from(f[5]),
            })
        })
        .collect()
}

/// `work` done with `a`'s file-system identity: what it opens, it opens as
/// the account would, so a link it made leads nowhere it couldn't go
/// itself, and what it makes is its own.
fn as_account<T>(a: &Account, work: impl FnOnce() -> T) -> T {
    // SAFETY: setfsuid and setfsgid take any id and change only this
    // thread's file-system identity (this helper has one thread).
    unsafe {
        libc::setfsgid(a.gid);
        libc::setfsuid(a.uid);
    }
    let out = work();
    // SAFETY: as above; back to root's (a no-op where it isn't root).
    unsafe {
        libc::setfsuid(0);
        libc::setfsgid(0);
    }
    out
}

/// A file read as it is: a regular file, never through a link, never
/// waited on (a pipe), and not a large one. Not there is `NotFound`.
fn read_plain(path: &Path) -> std::io::Result<(Vec<u8>, std::time::SystemTime)> {
    let file = File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > 1 << 20 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a regular file of 1 MiB or less"));
    }
    let mut bytes = Vec::new();
    file.take(1 << 20).read_to_end(&mut bytes)?;
    Ok((bytes, meta.modified()?))
}

/// When the file was changed, if it's a regular file.
fn changed(path: &Path) -> Option<std::time::SystemTime> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.is_file().then(|| meta.modified().ok()).flatten()
}

/// `bytes` written to `path` in place of what's there (its folders made),
/// dated `when` if given.
fn write_plain(path: &Path, bytes: &[u8], when: Option<std::time::SystemTime>) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let dir = path.parent().ok_or("no folder")?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(err)?;
    let tmp = dir.join(format!(".{}.aizome-tv", path.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    let _ = std::fs::remove_file(&tmp);
    let mut f = File::options()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .map_err(err)?;
    f.write_all(bytes).map_err(err)?;
    if let Some(t) = when {
        f.set_modified(t).map_err(err)?;
    }
    drop(f);
    std::fs::rename(&tmp, path).map_err(err)
}

/// Where WirePlumber keeps an account's sound settings, and the files of
/// them: the output and input chosen, each sound card's profile, each
/// device's port and volume. Not each app's volume.
const SOUND_DIR: &str = ".local/state/wireplumber";
const SOUND_FILES: [&str; 3] = ["default-nodes", "default-profile", "default-routes"];
/// An account's Aizome settings, whose `outputs` are its displays'.
const SETTINGS: &str = ".config/aizome/settings.toml";

/// `follow UID`: as the account of that user id starts (systemd runs this
/// before its own systemd), if it's the TV account: its sound settings
/// become those of the account that changed its own last, if that was
/// after the TV account's own changed (in Chanoma's Sound); and its
/// displays those of the account that set its own up anew last. Nothing of
/// any other account changes.
fn follow(uid: &str) -> Result<(), String> {
    let Some(record) = TvAccount::read() else { return Ok(()) };
    let passwd = std::fs::read_to_string("/etc/passwd").map_err(|e| format!("/etc/passwd: {e}"))?;
    let all = accounts(&passwd);
    let Some(tv) = all.iter().find(|a| a.name == record.user) else { return Ok(()) };
    if uid.parse::<u32>().ok() != Some(tv.uid) {
        return Ok(());
    }
    // Nothing of root's own groups.
    // SAFETY: setgroups with no groups; this helper has one thread.
    unsafe { libc::setgroups(0, std::ptr::null()) };
    let others: Vec<&Account> = all.iter().filter(|a| a.uid != tv.uid).collect();
    let sound = follow_sound(tv, &others);
    let displays = follow_displays(tv, &others);
    sound.and(displays)
}

/// The newest change to any of an account's sound files.
fn sound_changed(a: &Account) -> Option<std::time::SystemTime> {
    as_account(a, || SOUND_FILES.iter().filter_map(|f| changed(&a.home.join(SOUND_DIR).join(f))).max())
}

fn follow_sound(tv: &Account, others: &[&Account]) -> Result<(), String> {
    let Some((from, when)) = others.iter().filter_map(|a| Some((*a, sound_changed(a)?))).max_by_key(|(_, t)| *t) else {
        return Ok(());
    };
    if sound_changed(tv).is_some_and(|own| own >= when) {
        return Ok(());
    }
    for name in SOUND_FILES {
        let read = as_account(from, || read_plain(&from.home.join(SOUND_DIR).join(name))).ok();
        let to = tv.home.join(SOUND_DIR).join(name);
        as_account(tv, || match read {
            // Its date kept: the TV account's is as new as it, no newer.
            Some((bytes, t)) => write_plain(&to, &bytes, Some(t)),
            None => match std::fs::remove_file(&to) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", to.display())),
                _ => Ok(()),
            },
        })?;
    }
    eprintln!("aizome-tv-account: {} follows the sound {} chose", tv.name, from.name);
    Ok(())
}

/// An account's settings: none if it has no file, an error if it has one
/// that can't be read as it is (a link, a pipe, not TOML).
fn settings_of(a: &Account) -> Result<Option<(toml::Table, std::time::SystemTime)>, String> {
    let path = a.home.join(SETTINGS);
    let (bytes, t) = match as_account(a, || read_plain(&path)) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some((text.parse::<toml::Table>().map_err(|e| format!("{}: {e}", path.display()))?, t)))
}

fn follow_displays(tv: &Account, others: &[&Account]) -> Result<(), String> {
    // Each account's displays as they were set up when last followed: one
    // whose are as they were hasn't set them up since, whatever else of its
    // settings changed.
    let seen = as_account(tv, || read_plain(&tv.home.join(FOLLOWED)))
        .ok()
        .and_then(|(bytes, _)| String::from_utf8(bytes).ok()?.parse::<toml::Table>().ok())
        .and_then(|t| t.get("accounts")?.as_table().cloned())
        .unwrap_or_default();
    let mut now = toml::Table::new();
    let mut newest: Option<(&Account, toml::Value, std::time::SystemTime)> = None;
    for a in others {
        let Some((table, t)) = settings_of(a).ok().flatten() else { continue };
        let Some(outputs) = table.get("outputs").and_then(toml::Value::as_array).filter(|o| !o.is_empty()) else {
            continue;
        };
        let outputs = toml::Value::Array(outputs.clone());
        let kept = toml::Value::String(digest(&outputs)?);
        if seen.get(&a.name) != Some(&kept) && newest.as_ref().is_none_or(|(_, _, n)| t >= *n) {
            newest = Some((*a, outputs, t));
        }
        now.insert(a.name.clone(), kept);
    }
    // Set up anew since by none: what the TV account chose (Chanoma's
    // Settings › Display) stays.
    if let Some((from, outputs, _)) = newest {
        // Settings there but not to be read stay as they are, and the
        // displays are followed once they can be.
        let mut own = settings_of(tv)
            .map_err(|e| format!("{e}: left as it is, without the displays {} set up", from.name))?
            .map(|(t, _)| t)
            .unwrap_or_default();
        if own.get("outputs") != Some(&outputs) {
            own.insert("outputs".into(), outputs);
            let text = toml::to_string(&own).map_err(|e| e.to_string())?;
            as_account(tv, || write_plain(&tv.home.join(SETTINGS), text.as_bytes(), None))?;
            eprintln!("aizome-tv-account: {} follows the displays {} set up", tv.name, from.name);
        }
    } else if now == seen {
        return Ok(());
    }
    let record = toml::to_string(&toml::Table::from_iter([("accounts".to_owned(), toml::Value::Table(now))]))
        .map_err(|e| e.to_string())?;
    as_account(tv, || write_plain(&tv.home.join(FOLLOWED), record.as_bytes(), None))
}

/// What the TV account keeps of an account's displays: a SHA-256 of them,
/// enough to tell whether they were set up anew, so another account's
/// displays (their names and serial numbers) aren't copied into its home.
fn digest(outputs: &toml::Value) -> Result<String, String> {
    use sha2::Digest;
    let text = toml::to_string(&toml::Table::from_iter([("outputs".to_owned(), outputs.clone())]))
        .map_err(|e| e.to_string())?;
    Ok(sha2::Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect())
}

/// Each regular account's displays, as a digest, as it had them set up when
/// the TV account last followed (`[accounts]`, by name).
const FOLLOWED: &str = ".local/state/aizome/tv-followed-outputs.toml";
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
    fn the_tv_account_follows_the_displays_and_sound_chosen_last() {
        use std::time::{Duration, SystemTime};
        let root = std::env::temp_dir().join(format!("tv-account-follow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // SAFETY: getuid and getgid have no preconditions.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let account = |name: &str| Account { name: name.into(), uid, gid, home: root.join(name) };
        let (tv, alice, bob) = (account("tv"), account("alice"), account("bob"));
        let at = |secs: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs));
        let sound = |a: &Account, name: &str| a.home.join(SOUND_DIR).join(name);
        write_plain(&sound(&alice, "default-nodes"), b"[default-nodes]\nsink=analog\n", at(10)).unwrap();
        write_plain(&sound(&bob, "default-nodes"), b"[default-nodes]\nsink=hdmi\n", at(20)).unwrap();
        write_plain(&sound(&bob, "default-profile"), b"[default-profile]\n", at(5)).unwrap();
        write_plain(&sound(&tv, "default-routes"), b"[default-routes]\n", at(1)).unwrap();
        let settings = |outputs: &str| format!("[appearance]\ntheme = \"night\"\n{outputs}");
        let tv_output = "[[outputs]]\nmake = \"GSM\"\nmodel = \"LG TV SSCR2\"\nscale = 2.0\n";
        write_plain(&alice.home.join(SETTINGS), settings(tv_output).as_bytes(), at(30)).unwrap();
        write_plain(&bob.home.join(SETTINGS), settings("").as_bytes(), at(40)).unwrap();
        write_plain(&tv.home.join(SETTINGS), b"[appearance]\ntheme = \"day\"\n", at(0)).unwrap();

        follow_sound(&tv, &[&alice, &bob]).unwrap();
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        // Bob's sound, changed last; its files alone, dated as his.
        assert_eq!(std::fs::read(sound(&tv, "default-nodes")).unwrap(), b"[default-nodes]\nsink=hdmi\n");
        assert!(sound(&tv, "default-profile").exists() && !sound(&tv, "default-routes").exists());
        assert_eq!(changed(&sound(&tv, "default-nodes")), at(20));
        // Alice's displays: Bob's settings are newer but set none up. The
        // TV account's own settings stay.
        let own: toml::Table = std::fs::read_to_string(tv.home.join(SETTINGS)).unwrap().parse().unwrap();
        assert_eq!(own["appearance"]["theme"].as_str(), Some("day"));
        assert_eq!(own["outputs"][0]["model"].as_str(), Some("LG TV SSCR2"));
        // Chosen in Chanoma since, and Alice's set up as they were: the TV
        // account's own stay, whatever else of Alice's changed.
        let chosen = |scale: &str| format!("[[outputs]]\nmake = \"GSM\"\nmodel = \"LG TV SSCR2\"\nscale = {scale}\n");
        let outputs_of = |a: &Account| {
            let t: toml::Table = std::fs::read_to_string(a.home.join(SETTINGS)).unwrap().parse().unwrap();
            t["outputs"][0]["scale"].as_float()
        };
        write_plain(&tv.home.join(SETTINGS), chosen("1.5").as_bytes(), at(45)).unwrap();
        write_plain(
            &alice.home.join(SETTINGS),
            format!("[appearance]\ntheme = \"day\"\n{tv_output}").as_bytes(),
            at(46),
        )
        .unwrap();
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        assert_eq!(outputs_of(&tv), Some(1.5));
        // Alice sets them up anew: hers again.
        write_plain(&alice.home.join(SETTINGS), settings(&chosen("3.0")).as_bytes(), at(47)).unwrap();
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        assert_eq!(outputs_of(&tv), Some(3.0));

        // Chosen in Chanoma since: the TV account's own stays.
        write_plain(&sound(&tv, "default-nodes"), b"[default-nodes]\nsink=usb\n", at(50)).unwrap();
        follow_sound(&tv, &[&alice, &bob]).unwrap();
        assert_eq!(std::fs::read(sound(&tv, "default-nodes")).unwrap(), b"[default-nodes]\nsink=usb\n");
        // A link in an account's files is never followed.
        std::fs::remove_file(sound(&bob, "default-nodes")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", sound(&bob, "default-nodes")).unwrap();
        write_plain(&sound(&bob, "default-profile"), b"[default-profile]\n", at(60)).unwrap();
        follow_sound(&tv, &[&alice, &bob]).unwrap();
        assert!(!sound(&tv, "default-nodes").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The TV account's settings, there but not to be read as they are (a
    /// link, or not TOML), stay as they are, and nothing is followed.
    #[test]
    fn the_tv_accounts_settings_stay_when_they_cant_be_read() {
        use std::time::{Duration, SystemTime};
        let root = std::env::temp_dir().join(format!("tv-account-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // SAFETY: getuid and getgid have no preconditions.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let account = |name: &str| Account { name: name.into(), uid, gid, home: root.join(name) };
        let (tv, admin) = (account("tv"), account("admin"));
        let at = |secs: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs));
        let hdmi = b"[[outputs]]\nname = \"HDMI-A-1\"\nscale = 2.0\n";
        write_plain(&admin.home.join(SETTINGS), hdmi, at(10)).unwrap();
        let own = tv.home.join(SETTINGS);
        let marker = tv.home.join(FOLLOWED);
        let day = "[appearance]\ntheme = \"day\"\n";

        // A link to settings that are fine.
        let target = root.join("tv-settings.toml");
        std::fs::write(&target, day).unwrap();
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &own).unwrap();
        let result = follow_displays(&tv, &[&admin]);
        let link = std::fs::symlink_metadata(&own).unwrap().file_type().is_symlink();
        eprintln!(
            "link: {result:?}; still a link: {link}; it reads {:?}; the target {:?}; marker there: {}",
            std::fs::read_to_string(&own).ok(),
            std::fs::read_to_string(&target).ok(),
            marker.exists()
        );
        assert!(link, "the link stays");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), day);
        assert!(!marker.exists(), "nothing followed");
        assert!(result.is_err());

        // Not TOML.
        std::fs::remove_file(&own).unwrap();
        let broken = "[appearance\ntheme = \"day\"\n";
        std::fs::write(&own, broken).unwrap();
        let result = follow_displays(&tv, &[&admin]);
        eprintln!(
            "malformed: {result:?}; it reads {:?}; marker there: {}",
            std::fs::read_to_string(&own).ok(),
            marker.exists()
        );
        assert_eq!(std::fs::read_to_string(&own).unwrap(), broken);
        assert!(!marker.exists(), "nothing followed");
        assert!(result.is_err());

        // None at all: made, with the displays followed.
        std::fs::remove_file(&own).unwrap();
        follow_displays(&tv, &[&admin]).unwrap();
        let made: toml::Table = std::fs::read_to_string(&own).unwrap().parse().unwrap();
        assert_eq!(made["outputs"][0]["scale"].as_float(), Some(2.0));
        assert!(marker.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Only a change to an account's displays is followed: another change
    /// of its settings leaves the TV account's own choice.
    #[test]
    fn the_tv_account_follows_only_a_change_to_the_displays() {
        use std::time::{Duration, SystemTime};
        let root = std::env::temp_dir().join(format!("tv-account-unrelated-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // SAFETY: getuid and getgid have no preconditions.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let account = |name: &str| Account { name: name.into(), uid, gid, home: root.join(name) };
        let (tv, alice, bob) = (account("tv"), account("alice"), account("bob"));
        let at = |secs: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs));
        let set = |a: &Account, theme: &str, scale: &str, t: u64| {
            let text =
                format!("[appearance]\ntheme = \"{theme}\"\n[[outputs]]\nname = \"HDMI-A-1\"\nscale = {scale}\n");
            write_plain(&a.home.join(SETTINGS), text.as_bytes(), at(t)).unwrap();
        };
        let scale_of = |a: &Account| {
            let t: toml::Table = std::fs::read_to_string(a.home.join(SETTINGS)).unwrap().parse().unwrap();
            t["outputs"][0]["scale"].as_float()
        };
        set(&bob, "night", "1.0", 10);
        set(&alice, "night", "2.0", 20);
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        assert_eq!(scale_of(&tv), Some(2.0), "Alice's, set up last");
        // What's kept of each account's displays is a digest, not them.
        let kept = std::fs::read_to_string(tv.home.join(FOLLOWED)).unwrap();
        assert!(kept.contains("alice") && kept.contains("bob") && !kept.contains("HDMI-A-1"), "{kept}");
        // Chosen on the TV, then only Bob's theme changes.
        set(&tv, "day", "1.5", 30);
        set(&bob, "day", "1.0", 40);
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        eprintln!("after Bob's theme: the TV's scale {:?}", scale_of(&tv));
        assert_eq!(scale_of(&tv), Some(1.5), "the TV's own stays");
        // Bob sets his displays up anew: his, though Alice's settings change after.
        set(&bob, "day", "3.0", 50);
        set(&alice, "day", "2.0", 60);
        follow_displays(&tv, &[&alice, &bob]).unwrap();
        assert_eq!(scale_of(&tv), Some(3.0), "Bob's, the displays set up last");
        // The TV's theme stays its own throughout.
        let own: toml::Table = std::fs::read_to_string(tv.home.join(SETTINGS)).unwrap().parse().unwrap();
        assert_eq!(own["appearance"]["theme"].as_str(), Some("day"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A pipe among an account's files isn't waited on: it isn't a file to
    /// follow (and the TV account's systemd waits for this, without end).
    #[test]
    fn a_pipe_in_an_accounts_files_is_passed_over() {
        use std::os::unix::ffi::OsStrExt;
        use std::time::Duration;
        let root = std::env::temp_dir().join(format!("tv-account-pipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // SAFETY: getuid and getgid have no preconditions.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let account = |name: &str| Account { name: name.into(), uid, gid, home: root.join(name) };
        let (tv, bob) = (account("tv"), account("bob"));
        let pipe = bob.home.join(SETTINGS);
        std::fs::create_dir_all(pipe.parent().unwrap()).unwrap();
        let c = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(follow_displays(&tv, &[&bob]));
        });
        let done = rx.recv_timeout(Duration::from_secs(5));
        if done.is_err() {
            // Let a reader that's waiting go.
            let _ = File::options().write(true).custom_flags(libc::O_NONBLOCK).open(&pipe);
        }
        assert!(matches!(done, Ok(Ok(()))), "{done:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_accounts_are_the_peoples() {
        let passwd = "root:x:0:0::/root:/bin/sh\nalice:x:1000:100:Alice:/home/alice:/bin/sh\n\
                      tv:x:1001:1001:TV:/home/tv:/sbin/nologin\nnobody:x:65534:65534::/var/empty:/bin/false\n";
        let names: Vec<String> = accounts(passwd).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["alice", "tv"]);
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
