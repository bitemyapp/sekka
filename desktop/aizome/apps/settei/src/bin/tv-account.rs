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
//! - `remove [--files]`: remove the account, its home folder too.
//! - `forget-chanoma-pin`: forget the PIN of the account's Chanoma (its
//!   grown-ups' PIN), as `aizome-chanoma --reset-pin` run as the account.
//!
//! It runs only what's named here, found in the system's own folders, and
//! writes only the account's record (`/var/lib/aizome/tv-account.toml`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use aizome_sys::tv::{self, Auth, TvAccount};

fn main() {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        fail("this runs as root, through pkexec");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["set", auth] => match Auth::parse(auth) {
            Some(auth) => set(auth, &secret()),
            None => Err(format!("no way to sign in called {auth}")),
        },
        ["remove"] => remove(false),
        ["remove", "--files"] => remove(true),
        ["forget-chanoma-pin"] => forget_chanoma_pin(),
        _ => Err("usage: aizome-tv-account set none|pin|password | remove [--files] | forget-chanoma-pin".into()),
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
fn exists(name: &str) -> bool {
    std::fs::read_to_string("/etc/passwd").unwrap_or_default().lines().any(|l| l.split(':').next() == Some(name))
}

fn set(auth: Auth, secret: &str) -> Result<(), String> {
    let change = tv::Change::Set { auth, secret: secret.to_owned() };
    change.check().map_err(|e| e.to_string())?;
    let current = TvAccount::read();
    let user = current.as_ref().map_or(tv::NAME, |t| t.user.as_str()).to_owned();
    if !exists(&user) {
        let nologin = tool("nologin")?;
        run(Command::new(tool("useradd")?)
            .args(["--create-home", "--user-group", "--comment", tv::REAL_NAME, "--shell"])
            .arg(nologin)
            .arg(&user))?;
    } else if current.is_none() {
        return Err(format!("There's already an account called {user}, and it isn't the TV account"));
    }
    let hash = match auth {
        Auth::None => String::new(),
        Auth::Pin | Auth::Password => aizome_sys::users::crypt_password(secret).map_err(|e| e.to_string())?,
    };
    // Unlocked too, should it have been locked.
    run(Command::new(tool("usermod")?).arg("--password").arg(&hash).arg(&user))?;
    let record = TvAccount { user, auth, pin_length: (auth == Auth::Pin).then_some(secret.len() as u8) };
    write(&record)
}

/// The record, whole: a new file renamed over the old, readable by all.
fn write(record: &TvAccount) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let path = Path::new(tv::PATH);
    let dir = path.parent().ok_or("no folder for the record")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    let new = path.with_extension("new");
    std::fs::write(&new, record.to_toml()).map_err(|e| format!("{}: {e}", new.display()))?;
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    std::fs::rename(&new, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn remove(files: bool) -> Result<(), String> {
    let Some(record) = TvAccount::read() else { return Ok(()) };
    if exists(&record.user) {
        let mut cmd = Command::new(tool("userdel")?);
        if files {
            cmd.arg("--remove");
        }
        run(cmd.arg(&record.user))?;
    }
    std::fs::remove_file(tv::PATH).map_err(|e| format!("{}: {e}", tv::PATH))
}

fn forget_chanoma_pin() -> Result<(), String> {
    let record = TvAccount::read().ok_or("There's no TV account")?;
    run(Command::new(tool("runuser")?).args(["-u", &record.user, "--"]).arg(tool("aizome-chanoma")?).arg("--reset-pin"))
}
