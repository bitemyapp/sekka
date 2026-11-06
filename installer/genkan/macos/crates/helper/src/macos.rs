// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper on macOS: launchd entry, XPC, PAM, `bless`, and the engine's
//! process. Everything it runs is an absolute path under `/usr` or the
//! helper's own engine directory, with a fixed argument list and a cleared
//! environment.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sekka_mac_contract::{
    APP_IDENTIFIER, Digest, EngineBody, EngineOutcome, Fact, Failure, HELPER_SERVICE, Hasher,
    Inspection, Timestamp, Transcript, Uuid,
};
use sekka_mac_trust::TrustConfig;

use crate::engine::{ChildEngine, EngineProcess};
use crate::{EngineInstall, Helper, Paths, Platform, PlatformError, Secret};

/// The team that signs release builds, fixed at build time.
const TEAM: Option<&str> = option_env!("SEKKA_TEAM_ID");
#[cfg(feature = "production")]
const _: () = assert!(
    option_env!("SEKKA_TEAM_ID").is_some(),
    "production helpers name the team whose app they accept (SEKKA_TEAM_ID)"
);

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

pub fn main() {
    // SAFETY: getuid/umask have no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("sekka-installer-helper is started by launchd as root, for Sekka Installer.");
        std::process::exit(2);
    }
    unsafe { libc::umask(0o077) };
    let paths = Paths::system();
    let platform = match MacPlatform::new(paths.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sekka-installer-helper: {e}");
            std::process::exit(1);
        }
    };
    let helper = match Helper::new(
        platform,
        paths,
        TrustConfig::for_this_build(),
        env!("CARGO_PKG_VERSION").parse().expect("the package version"),
    ) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("sekka-installer-helper: its state folder: {e}");
            std::process::exit(1);
        }
    };
    let requirement = sekka_mac_xpc::requirement(APP_IDENTIFIER, TEAM);
    sekka_mac_xpc::listen(HELPER_SERVICE, &requirement, move |peer, request, password| {
        helper.handle(peer.uid, request, password)
    })
}

pub struct MacPlatform {
    paths: Paths,
    /// `Sekka Installer.app/Contents`, found from the helper's own path
    /// (`Contents/MacOS/sekka-installer-helper`, as the launchd plist in
    /// `Contents/Library/LaunchDaemons` names it).
    contents: PathBuf,
}

impl MacPlatform {
    pub fn new(paths: Paths) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let contents = exe
            .parent()
            .and_then(Path::parent)
            .filter(|c| c.file_name().is_some_and(|n| n == "Contents"))
            .ok_or("the helper must run from inside Sekka Installer.app")?
            .to_path_buf();
        Ok(Self { paths, contents })
    }

    fn engine_archive(&self) -> PathBuf {
        self.contents.join("Resources/Engine/engine.tar.gz")
    }
}

fn run(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", PATH)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
}

fn plain_account_name(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 255
        && !user.starts_with('-')
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The engine's own description of how to start it, inside its archive.
#[derive(serde::Deserialize)]
struct EngineEntry {
    /// The Python to run, relative to the engine's root.
    python: String,
}

fn engine_python(engine: &EngineInstall) -> Result<PathBuf, PlatformError> {
    let bytes = fs::read(engine.root.join("engine-lock.json"))
        .map_err(|e| PlatformError::Engine(format!("The installer's engine is incomplete: {e}")))?;
    let entry: EngineEntry = serde_json::from_slice(&bytes)
        .map_err(|e| PlatformError::Engine(format!("The installer's engine is incomplete: {e}")))?;
    let python = engine.root.join(&entry.python);
    if !python.starts_with(&engine.root) || entry.python.contains("..") {
        return Err(PlatformError::Engine("The installer's engine names a program outside itself.".into()));
    }
    Ok(python)
}

fn engine_command(engine: &EngineInstall, args: &[&str]) -> Result<Command, PlatformError> {
    let mut command = Command::new(engine_python(engine)?);
    command
        .args(["-I", "-m", "sekka_engine"])
        .args(args)
        .current_dir(&engine.root)
        .env_clear()
        .env("PATH", PATH)
        .env("LC_ALL", "C")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1");
    Ok(command)
}

impl Platform for MacPlatform {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError> {
        let archive = self.engine_archive();
        let mut f = fs::File::open(&archive)
            .map_err(|e| PlatformError::Engine(format!("This copy of Sekka Installer has no engine ({e}). Download it again.")))?;
        let mut hasher = Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).map_err(|e| PlatformError::Engine(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let digest = hasher.finish();
        if digest != *expected {
            return Err(PlatformError::Engine(
                "This copy of Sekka Installer carries a different engine than the release needs. Download the latest Sekka Installer.".into(),
            ));
        }
        let root = self.paths.engine(&digest);
        if root.join(".complete").exists() {
            return Ok(EngineInstall { root, digest });
        }
        let parent = root.parent().expect("engine directories have a parent");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| PlatformError::Engine(e.to_string()))?;
        let tmp = parent.join(format!(".unpack-{}", digest.hex()));
        let _ = fs::remove_dir_all(&tmp);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&tmp)
            .map_err(|e| PlatformError::Engine(e.to_string()))?;
        // The archive's digest is the catalog's (and the app bundle's
        // signature covers it); bsdtar refuses absolute and parent paths.
        let out = run(
            "/usr/bin/tar",
            &[
                "-x",
                "-z",
                "--no-same-owner",
                "-f",
                archive.to_str().unwrap_or_default(),
                "-C",
                tmp.to_str().unwrap_or_default(),
            ],
        )
        .map_err(|e| PlatformError::Engine(e.to_string()))?;
        if !out.status.success() {
            let _ = fs::remove_dir_all(&tmp);
            return Err(PlatformError::Engine(format!(
                "The installer's engine couldn't be unpacked: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        fs::write(tmp.join(".complete"), digest.to_string()).map_err(|e| PlatformError::Engine(e.to_string()))?;
        let _ = fs::remove_dir_all(&root);
        fs::rename(&tmp, &root).map_err(|e| PlatformError::Engine(e.to_string()))?;
        Ok(EngineInstall { root, digest })
    }

    fn inspect(&self, engine: &EngineInstall) -> Result<Inspection, PlatformError> {
        let child = engine_command(engine, &["inspect"])?
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlatformError::Engine(e.to_string()))?;
        let mut process = ChildEngine::new(child).map_err(|e| PlatformError::Engine(e.to_string()))?;
        let mut transcript = Transcript::inspect();
        let mut inspection = None;
        let mut failure = None;
        while let Some(line) = process
            .next_line()
            .map_err(|e| PlatformError::Inspection(e.to_string()))?
        {
            let message = transcript.accept(&line).map_err(|e| {
                PlatformError::Inspection(format!("The engine's inspection didn't make sense: {e}"))
            })?;
            match message.message {
                EngineBody::Inspection(i) => inspection = Some(*i),
                EngineBody::Failure { failure: f, .. } => failure = Some(f.message.to_string()),
                _ => {}
            }
            if transcript.outcome().is_some() {
                break;
            }
        }
        let _ = process.wait();
        if let Some(f) = failure {
            return Err(PlatformError::Inspection(f));
        }
        match (transcript.outcome(), inspection) {
            (Some(EngineOutcome::Inspected), Some(i)) => Ok(i),
            _ => Err(PlatformError::Inspection("The engine couldn't inspect this Mac.".into())),
        }
    }

    fn authenticate_owner(&self, user: &str, password: &[u8]) -> Result<(), PlatformError> {
        if !plain_account_name(user) {
            return Err(PlatformError::Authorization("That isn't an account name on this Mac.".into()));
        }
        let admin = run("/usr/sbin/dseditgroup", &["-o", "checkmember", "-m", user, "admin"])
            .map_err(|e| PlatformError::Authorization(e.to_string()))?;
        if !admin.status.success() {
            return Err(PlatformError::Authorization(format!(
                "{user} isn't an administrator of this Mac. Use an administrator account."
            )));
        }
        if !pam_check(user, password) {
            return Err(PlatformError::Authorization(format!(
                "That password isn't right for {user}."
            )));
        }
        // On Apple silicon, only a volume owner can change how the Mac
        // starts up; macOS gives those accounts a secure token.
        let token = run("/usr/sbin/sysadminctl", &["-secureTokenStatus", user])
            .map_err(|e| PlatformError::Authorization(e.to_string()))?;
        let said = String::from_utf8_lossy(&token.stderr).to_string() + &String::from_utf8_lossy(&token.stdout);
        if !said.contains("ENABLED") {
            return Err(PlatformError::Authorization(format!(
                "{user} can't change how this Mac starts up (it isn't one of the Mac's owners). Use the account that set up this Mac, or another owner."
            )));
        }
        Ok(())
    }

    fn user_staging(&self, uid: u32) -> Result<PathBuf, PlatformError> {
        let home = home_of(uid).ok_or_else(|| PlatformError::Other("That account has no home folder.".into()))?;
        Ok(home.join("Library/Application Support/Sekka Installer/Staging"))
    }

    fn start_engine(
        &self,
        engine: &EngineInstall,
        plan: &Path,
        assets: &Path,
        owner: &str,
        password: Secret,
    ) -> Result<Box<dyn EngineProcess>, PlatformError> {
        if !plain_account_name(owner) {
            return Err(PlatformError::Authorization("That isn't an account name on this Mac.".into()));
        }
        // The password goes through a pipe on the engine's file descriptor
        // 3, written before it starts and closed: never an argument, never
        // the environment, never a file.
        let mut fds = [0 as c_int; 2];
        // SAFETY: a fresh pipe.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(PlatformError::Engine(std::io::Error::last_os_error().to_string()));
        }
        let (read_end, write_end) = (fds[0], fds[1]);
        {
            // SAFETY: we own the write end; File closes it when dropped.
            let mut w = unsafe { <fs::File as std::os::fd::FromRawFd>::from_raw_fd(write_end) };
            if password.len() >= 16 * 1024 || w.write_all(&password).is_err() {
                unsafe { libc::close(read_end) };
                return Err(PlatformError::Engine("The password couldn't be handed to the engine.".into()));
            }
        }
        drop(password);
        let mut command = engine_command(
            engine,
            &[
                "execute",
                "--plan",
                plan.to_str().unwrap_or_default(),
                "--assets",
                assets.to_str().unwrap_or_default(),
                "--owner",
                owner,
            ],
        )?;
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(read_end, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn();
        unsafe { libc::close(read_end) };
        let child = child.map_err(|e| PlatformError::Engine(e.to_string()))?;
        Ok(Box::new(ChildEngine::new(child).map_err(|e| PlatformError::Engine(e.to_string()))?))
    }

    fn set_startup_disk(&self, facts: &[Fact], owner: &str, password: &[u8]) -> Result<Vec<Fact>, Failure> {
        let fail = |code: &str, message: String, apple: Option<String>| Failure {
            code: code.parse().unwrap(),
            message: message.chars().take(2000).collect::<String>().parse().unwrap(),
            apple_diagnostic: apple.and_then(|a| a.chars().take(2000).collect::<String>().parse().ok()),
            disk_changed: sekka_mac_contract::DiskChanged::No,
        };
        let volume = facts
            .iter()
            .rev()
            .find(|f| f.name.as_str() == "stub-system-volume")
            .and_then(|f| f.value.as_str().parse::<Uuid>().ok())
            .ok_or_else(|| fail("record", "The installation doesn't record its startup volume.".into(), None))?;
        let info = run("/usr/sbin/diskutil", &["info", "-plist", &volume.to_upper()])
            .map_err(|e| fail("diskutil", e.to_string(), None))?;
        let device = plist_string(&info.stdout, "DeviceIdentifier")
            .filter(|d| d.starts_with("disk") && d.bytes().all(|b| b.is_ascii_alphanumeric()))
            .ok_or_else(|| fail("target-gone", "The new system's startup volume isn't on this Mac any more.".into(), None))?;
        if !plain_account_name(owner) {
            return Err(fail("account", "That isn't an account name on this Mac.".into(), None));
        }
        let mut child = Command::new("/usr/sbin/bless")
            .args(["--setBoot", "--device", &format!("/dev/{device}"), "--user", owner, "--stdinpass"])
            .env_clear()
            .env("PATH", PATH)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| fail("bless", e.to_string(), None))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(password);
        }
        let out = child.wait_with_output().map_err(|e| fail("bless", e.to_string(), None))?;
        if !out.status.success() {
            return Err(fail(
                "bless",
                "macOS didn't set the startup disk. If your password begins or ends with a space, set the startup disk in Recovery instead.".into(),
                Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            ));
        }
        let now = run("/usr/sbin/bless", &["--getBoot"]).map_err(|e| fail("bless", e.to_string(), None))?;
        let selected = String::from_utf8_lossy(&now.stdout).trim().to_string();
        if !selected.starts_with(&format!("/dev/{device}")) {
            return Err(fail(
                "not-selected",
                "macOS reported success, but the startup disk is still another volume.".into(),
                Some(selected),
            ));
        }
        Ok(vec![
            Fact::new("stub-system-volume", volume).expect("uuid"),
            Fact::new("selected-device", device).expect("device"),
        ])
    }
}

/// A string value for `key` at the top of an XML property list, enough for
/// `diskutil info -plist`.
fn plist_string(xml: &[u8], key: &str) -> Option<String> {
    let text = std::str::from_utf8(xml).ok()?;
    let at = text.find(&format!("<key>{key}</key>"))?;
    let rest = &text[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].to_string())
}

fn home_of(uid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0 as c_char; 16 * 1024];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes into our buffers.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buffer.as_mut_ptr(), buffer.len(), &mut result) };
    if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: pw_dir points into `buffer`, NUL-terminated.
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) }.to_str().ok()?.to_owned();
    Some(PathBuf::from(dir))
}

// OpenPAM, as macOS ships it, for the `checkpw` service: password checks
// without the password appearing in any argument list.
#[repr(C)]
struct PamMessage {
    style: c_int,
    msg: *const c_char,
}
#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    retcode: c_int,
}
#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    data: *mut c_void,
}
const PAM_SUCCESS: c_int = 0;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, handle: *mut *mut c_void) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}

extern "C" fn converse(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if count <= 0 || count > 32 || data.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: OpenPAM passes `count` messages and expects `count` responses
    // allocated with malloc, which it frees.
    unsafe {
        let password = &*(data as *const CString);
        let out = libc::calloc(count as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if out.is_null() {
            return PAM_CONV_ERR;
        }
        for i in 0..count as usize {
            let m = &**messages.add(i);
            if m.style == PAM_PROMPT_ECHO_OFF {
                (*out.add(i)).resp = libc::strdup(password.as_ptr());
            }
        }
        *responses = out;
    }
    PAM_SUCCESS
}

fn pam_check(user: &str, password: &[u8]) -> bool {
    let (Ok(user), Ok(service)) = (CString::new(user), CString::new("checkpw")) else {
        return false;
    };
    let Ok(secret) = CString::new(password.to_vec()) else {
        return false;
    };
    let conv = PamConv {
        conv: converse,
        data: &secret as *const CString as *mut c_void,
    };
    let mut handle = std::ptr::null_mut();
    // SAFETY: standard PAM sequence; `conv` and `secret` outlive the handle.
    let ok = unsafe {
        if pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut handle) != PAM_SUCCESS {
            return false;
        }
        let status = pam_authenticate(handle, 0);
        let status = if status == PAM_SUCCESS { pam_acct_mgmt(handle, 0) } else { status };
        pam_end(handle, status);
        status == PAM_SUCCESS
    };
    let mut bytes = secret.into_bytes();
    zeroize::Zeroize::zeroize(&mut bytes);
    ok
}
