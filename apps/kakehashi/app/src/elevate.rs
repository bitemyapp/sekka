//! Starting the helper with the system's own administrator prompt: pkexec
//! on Linux, the administrator prompt on macOS, UAC on Windows. The window
//! itself never runs with those rights.

use std::path::{Path, PathBuf};

/// How a run of the helper ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ran {
    /// The helper ran and exited with this code; its progress file says how
    /// the write went.
    Exited(i32),
    /// The person dismissed the prompt.
    Declined,
    /// The helper could not be started.
    Failed(String),
}

/// The helper beside this program: in the same directory, or in the same
/// `Contents/MacOS` of the app bundle.
pub fn helper_path() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let name = if cfg!(windows) { "kakehashi-helper.exe" } else { "kakehashi-helper" };
    Ok(exe.with_file_name(name))
}

#[cfg(target_os = "linux")]
pub fn run(helper: &Path, plan: &Path, progress: &Path) -> Ran {
    // On NixOS only the setuid wrapper works; polkit's own pkexec, which can
    // come first in the search path, refuses to run. Since NixOS 26.11 the
    // wrapper is there only when something asks for it.
    let wrapper = Path::new("/run/wrappers/bin/pkexec");
    let pkexec = if wrapper.exists() {
        wrapper
    } else if Path::new("/run/wrappers/bin").is_dir() {
        return Ran::Failed(
            "this NixOS system has no pkexec wrapper; set security.polkit.enablePkexecWrapper = true".into(),
        );
    } else {
        Path::new("pkexec")
    };
    let status =
        std::process::Command::new(pkexec).arg(helper).arg("--plan").arg(plan).arg("--progress").arg(progress).status();
    match status {
        // pkexec: 126, the prompt was dismissed; 127, not authorized.
        Ok(s) => match s.code() {
            Some(126) => Ran::Declined,
            Some(127) => Ran::Failed("not authorized, or no polkit agent is running to ask".into()),
            Some(code) => Ran::Exited(code),
            None => Ran::Failed("the helper was killed".into()),
        },
        Err(e) => Ran::Failed(format!("could not start pkexec: {e}")),
    }
}

/// A single-quoted shell word.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `do shell script` running `command` as an administrator.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript(command: &str) -> String {
    let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{escaped}\" with administrator privileges")
}

#[cfg(target_os = "macos")]
pub fn run(helper: &Path, plan: &Path, progress: &Path) -> Ran {
    let command = [
        shell_quote(&helper.to_string_lossy()),
        "--plan".into(),
        shell_quote(&plan.to_string_lossy()),
        "--progress".into(),
        shell_quote(&progress.to_string_lossy()),
    ]
    .join(" ");
    match std::process::Command::new("/usr/bin/osascript").arg("-e").arg(applescript(&command)).output() {
        Ok(out) if out.status.success() => Ran::Exited(0),
        // -128 is AppleScript's "User canceled".
        Ok(out) if String::from_utf8_lossy(&out.stderr).contains("-128") => Ran::Declined,
        Ok(_) => Ran::Exited(1),
        Err(e) => Ran::Failed(format!("could not start osascript: {e}")),
    }
}

#[cfg(windows)]
pub fn run(helper: &Path, plan: &Path, progress: &Path) -> Ran {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{PCWSTR, w};

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let file = wide(helper.as_os_str());
    // The paths are in the temporary directory and hold no quotes.
    let params = format!("--plan \"{}\" --progress \"{}\"", plan.display(), progress.display());
    let params = wide(std::ffi::OsStr::new(&params));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: every pointer in `info` outlives the call.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return if e.code() == ERROR_CANCELLED.to_hresult() {
            Ran::Declined
        } else {
            Ran::Failed(format!("could not start the helper: {e}"))
        };
    }
    let mut code = 0u32;
    // SAFETY: SEE_MASK_NOCLOSEPROCESS returned a process handle we own.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let got = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if got.is_err() {
            return Ran::Failed("could not read the helper's exit code".into());
        }
    }
    Ran::Exited(code as i32)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn run(_helper: &Path, _plan: &Path, _progress: &Path) -> Ran {
    Ran::Failed("Kakehashi writes drives on Linux, macOS and Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_the_shell_and_applescript() {
        assert_eq!(shell_quote("/tmp/it's here"), r"'/tmp/it'\''s here'");
        assert_eq!(
            applescript(r#"'/a "b"' --plan 'c\d'"#),
            r#"do shell script "'/a \"b\"' --plan 'c\\d'" with administrator privileges"#
        );
    }
}
