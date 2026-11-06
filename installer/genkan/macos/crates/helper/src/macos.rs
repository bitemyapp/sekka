// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper on macOS: launchd entry, XPC, PAM, `bless`, and the engine's
//! process. Everything it runs is an absolute path under `/usr` or the
//! helper's own engine directory, with a fixed argument list and a cleared
//! environment.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sekka_mac_contract::{
    APP_IDENTIFIER, Digest, HELPER_SERVICE, Inspection, Timestamp, Uuid,
};
use sekka_mac_journal::InstallLock;
use sekka_mac_trust::TrustConfig;

use crate::engine::{ChildEngine, EngineProcess};
use sekka_mac_engine::PATH;
use crate::{EngineInstall, EngineRun, Helper, PASSWORD_LIMIT, Paths, Platform, PlatformError, Secret};

/// The team that signs release builds, fixed at build time.
const TEAM: Option<&str> = option_env!("SEKKA_TEAM_ID");
#[cfg(feature = "production")]
const _: () = assert!(
    option_env!("SEKKA_TEAM_ID").is_some(),
    "production helpers name the team whose app they accept (SEKKA_TEAM_ID)"
);

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

impl Platform for MacPlatform {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError> {
        let cache = self.paths.root().join("engine");
        sekka_mac_engine::unpack(&self.engine_archive(), &cache, Some(expected))
            .map_err(|e| PlatformError::Engine(e.to_string()))
    }

    fn inspect(&self, engine: &EngineInstall) -> Result<Inspection, PlatformError> {
        sekka_mac_engine::inspect(engine).map_err(|e| PlatformError::Inspection(e.to_string()))
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
        run: EngineRun<'_>,
        installation: Uuid,
        plan: &Path,
        owner: &str,
        password: Secret,
        lock: &InstallLock,
    ) -> Result<Box<dyn EngineProcess>, PlatformError> {
        if !plain_account_name(owner) {
            return Err(PlatformError::Authorization("That isn't an account name on this Mac.".into()));
        }
        // The password goes through a pipe on the engine's file descriptor
        // 3, written before it starts and closed: never an argument, never
        // the environment, never a file. Both ends are close-on-exec; only
        // the engine gets the read end, as its 3.
        let (read_end, mut write_end) = std::io::pipe().map_err(|e| PlatformError::Engine(e.to_string()))?;
        if password.len() > PASSWORD_LIMIT || write_end.write_all(&password).is_err() {
            return Err(PlatformError::Engine("The password couldn't be handed to the engine.".into()));
        }
        drop(write_end);
        drop(password);
        let id = installation.to_string();
        let plan = plan.to_str().unwrap_or_default();
        let inventory = match run {
            EngineRun::StartupDisk { esp_inventory } => esp_inventory.to_string(),
            EngineRun::Execute { .. } => String::new(),
        };
        let args: Vec<&str> = match run {
            EngineRun::Execute { assets } => vec![
                "execute",
                "--plan",
                plan,
                "--assets",
                assets.to_str().unwrap_or_default(),
                "--installation",
                &id,
                "--owner",
                owner,
            ],
            EngineRun::StartupDisk { .. } => vec![
                "startup-disk",
                "--plan",
                plan,
                "--installation",
                &id,
                "--owner",
                owner,
                "--esp-inventory",
                &inventory,
            ],
        };
        let mut command = sekka_mac_engine::command(engine, &args)
        .map_err(|e| PlatformError::Engine(e.to_string()))?;
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        sekka_mac_engine::inherit(&mut command, read_end.as_raw_fd(), lock.as_fd().as_raw_fd());
        let child = command.spawn();
        drop(read_end);
        let child = child.map_err(|e| PlatformError::Engine(e.to_string()))?;
        Ok(Box::new(ChildEngine::new(child).map_err(|e| PlatformError::Engine(e.to_string()))?))
    }
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
