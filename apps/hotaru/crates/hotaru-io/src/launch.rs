//! Child-only terminal identity and optional default-shell startup integration.
//!
//! Never mutates the process environment or a user's startup files. Preparation
//! runs on the PTY owner, before fork; explicit commands are not rewritten.

use crate::PtyCommand;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

pub(crate) fn prepare(command: &mut PtyCommand) -> Vec<(OsString, OsString)> {
    let mut env: HashMap<_, _> = std::env::vars_os().collect();
    env.extend(command.env.iter().cloned());
    prepare_with(command, &env)
}

fn prepare_with(
    command: &mut PtyCommand,
    env: &HashMap<OsString, OsString>,
) -> Vec<(OsString, OsString)> {
    let mut defaults = vec![
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("TERM_PROGRAM".into(), "Hotaru".into()),
        (
            "TERM_PROGRAM_VERSION".into(),
            env!("CARGO_PKG_VERSION").into(),
        ),
    ];
    if let Some(directory) = env.get(OsStr::new("HOTARU_TERMINFO_DIR")) {
        let directory = Path::new(directory);
        if directory.is_absolute() && terminfo_available(directory) {
            defaults[0].1 = "xterm-hotaru".into();
            defaults.push(("TERMINFO".into(), directory.as_os_str().into()));
            let mut search = directory.as_os_str().to_os_string();
            search.push(":");
            if let Some(original) = env.get(OsStr::new("TERMINFO_DIRS")) {
                search.push(original);
                search.push(":");
            }
            // An empty component asks ncurses to retain its compiled defaults.
            defaults.push(("TERMINFO_DIRS".into(), search));
        }
    }
    if !command.integrate_shell || !command.args.is_empty() {
        return defaults;
    }
    let Some(resources) = env.get(OsStr::new("HOTARU_RESOURCES_DIR")) else {
        return defaults;
    };
    let resources = Path::new(resources);
    if !resources.is_absolute() {
        return defaults;
    }
    let shell = Path::new(&command.program)
        .file_name()
        .and_then(OsStr::to_str);
    let script = match shell {
        Some("bash") => "shell-integration/bash/rc.bash",
        Some("zsh") => "shell-integration/zsh/.zshenv",
        Some("fish") => "shell-integration/fish/hotaru.fish",
        _ => return defaults,
    };
    if std::fs::File::open(resources.join(script)).is_err() {
        return defaults;
    }
    match shell {
        Some("bash") => {
            // Bash still loads its compiled system bashrc. Only the user rc is
            // substituted; our bootstrap sources it once, then adds the hook.
            command
                .args
                .extend(["--rcfile".into(), resources.join(script).into_os_string()]);
        }
        Some("zsh") => {
            if let Some(original) = env.get(OsStr::new("ZDOTDIR")) {
                defaults.push(("HOTARU_ZSH_ZDOTDIR".into(), original.clone()));
                defaults.push(("HOTARU_ZSH_ZDOTDIR_SET".into(), "1".into()));
            } else {
                defaults.push(("HOTARU_ZSH_ZDOTDIR_SET".into(), "0".into()));
            }
            defaults.push((
                "ZDOTDIR".into(),
                resources.join("shell-integration/zsh").into_os_string(),
            ));
        }
        Some("fish") => command.args.extend([
            "--init-command".into(),
            "source \"$HOTARU_RESOURCES_DIR/shell-integration/fish/hotaru.fish\"".into(),
        ]),
        _ => unreachable!(),
    }
    defaults
}

fn terminfo_available(directory: &Path) -> bool {
    ["x/xterm-hotaru", "78/xterm-hotaru"]
        .into_iter()
        .any(|name| {
            let Ok(mut file) = std::fs::File::open(directory.join(name)) else {
                return false;
            };
            let mut header = [0; 12];
            if file.read_exact(&mut header).is_err() {
                return false;
            }
            let numbers = match &header[..2] {
                [0x1a, 0x01] => 2,
                [0x1e, 0x02] => 4,
                _ => return false,
            };
            let count =
                |offset| usize::from(u16::from_le_bytes([header[offset], header[offset + 1]]));
            let names = count(2);
            if !(1..=1024).contains(&names) {
                return false;
            }
            let booleans_end = 12 + names + count(4);
            let minimum =
                booleans_end + (booleans_end % 2) + numbers * count(6) + 2 * count(8) + count(10);
            if !file
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() >= minimum as u64)
            {
                return false;
            }
            let mut aliases = vec![0; names];
            file.read_exact(&mut aliases).is_ok()
                && aliases.last() == Some(&0)
                && aliases[..names - 1]
                    .split(|byte| *byte == b'|')
                    .any(|name| name == b"xterm-hotaru")
        })
}

/// Accept a bounded local OSC 7 directory without interpreting shell text.
/// `file:` URI paths are percent-decoded as bytes, preserving non-UTF-8 Linux
/// names. Kitty's cwd URI uses its raw path form instead. Foreign hosts, URI
/// credentials/ports, relative paths, C0/C1 controls and NUL are rejected.
pub fn directory_from_osc7(uri: &str, hostname: &str) -> Option<PathBuf> {
    if uri.len() > 16 * 1024 {
        return None;
    }
    let (body, encoded) = if let Some(body) = uri.strip_prefix("file://") {
        (body, true)
    } else {
        (uri.strip_prefix("kitty-shell-cwd://")?, false)
    };
    let (host, path) = body.split_once('/')?;
    // Match the pinned Linux reference: require an explicit localhost or the
    // exact kernel hostname. No DNS lookups or mutable HOSTNAME environment.
    if host.is_empty() || (host != "localhost" && host != hostname) {
        return None;
    }
    if encoded && path.contains(['?', '#']) {
        return None;
    }
    let mut bytes = Vec::with_capacity(path.len().min(4095) + 1);
    bytes.push(b'/');
    let mut source = path.as_bytes().iter().copied();
    while let Some(byte) = source.next() {
        let byte = if encoded && byte == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|v| v as u8);
            hex(source.next()?)? * 16 + hex(source.next()?)?
        } else {
            byte
        };
        if byte < 0x20 || byte == 0x7f || bytes.len() == 4096 {
            return None;
        }
        bytes.push(byte);
    }
    if contains_controls(&bytes) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

fn contains_controls(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => return text.chars().any(char::is_control),
            Err(error) => {
                if std::str::from_utf8(&bytes[..error.valid_up_to()])
                    .unwrap()
                    .chars()
                    .any(char::is_control)
                {
                    return true;
                }
                bytes = &bytes[error.valid_up_to()..];
                let count = error.error_len().unwrap_or(bytes.len());
                if bytes[..count]
                    .iter()
                    .any(|byte| matches!(byte, 0x80..=0x9f))
                {
                    return true;
                }
                bytes = &bytes[count..];
            }
        }
    }
    false
}

/// The kernel hostname, captured on the PTY worker rather than from mutable
/// shell environment variables. Linux bounds this to 64 bytes.
pub fn local_hostname() -> Option<String> {
    let mut bytes = [0u8; 256];
    // SAFETY: the writable buffer and its exact capacity are valid for this
    // call; no pointers escape and a terminator is checked before conversion.
    if unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return None;
    }
    let end = bytes.iter().position(|&byte| byte == 0)?;
    std::str::from_utf8(&bytes[..end])
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn local_directory_decoding_is_bounded_and_never_shell_evaluation() {
        assert_eq!(
            directory_from_osc7("file://host/tmp/a%20b%25c", "host"),
            Some("/tmp/a b%c".into())
        );
        assert_eq!(
            directory_from_osc7("file://localhost/tmp/%FF", "host")
                .unwrap()
                .as_os_str()
                .as_bytes(),
            b"/tmp/\xff"
        );
        assert_eq!(
            directory_from_osc7("kitty-shell-cwd://host/tmp/%20", "host"),
            Some("/tmp/%20".into())
        );
        assert_eq!(
            directory_from_osc7("file://host/tmp/%E6%97%A5%E6%9C%AC", "host"),
            Some("/tmp/日本".into())
        );
        for uri in [
            "file:///tmp",
            "file://remote/tmp",
            "file://host:22/tmp",
            "file://u@host/tmp",
            "file://host",
            "file://host/tmp/%",
            "file://host/tmp/%0a",
            "file://host/tmp/%1B",
            "file://host/tmp/%00",
            "file://host/tmp/%C2%9C",
            "file://host/tmp/%FF%C2%9C",
            "file://host/tmp/%9C",
            "file://host/tmp/?query",
            "file://host/tmp/#fragment",
            "https://host/tmp",
        ] {
            assert!(directory_from_osc7(uri, "host").is_none(), "{uri}");
        }
        assert!(
            directory_from_osc7(&format!("file://host/{}", "x".repeat(4096)), "host").is_none()
        );
    }

    #[test]
    fn missing_resources_and_explicit_commands_keep_arguments() {
        let mut command = PtyCommand::new("bash");
        command.args = vec!["-c".into(), "printf unchanged".into()];
        command.integrate_shell = true;
        let original = command.args.clone();
        let defaults = prepare_with(&mut command, &HashMap::new());
        assert_eq!(command.args, original);
        assert_eq!(defaults[0].1, "xterm-256color");
        assert!(!PtyCommand::new("bash").integrate_shell);
        assert!(PtyCommand::shell().integrate_shell);
    }

    #[test]
    fn private_database_and_shell_environment_preserve_caller_values() {
        let root = std::env::temp_dir().join(format!("hotaru-launch-{}", std::process::id()));
        for directory in [
            "x",
            "shell-integration/bash",
            "shell-integration/zsh",
            "shell-integration/fish",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        for file in [
            "shell-integration/bash/rc.bash",
            "shell-integration/zsh/.zshenv",
            "shell-integration/fish/hotaru.fish",
        ] {
            std::fs::write(root.join(file), "# fixture\n").unwrap();
        }
        let names = b"xterm-hotaru\0";
        let mut entry = vec![0x1e, 0x02, names.len() as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        entry.extend_from_slice(names);
        if entry.len() % 2 != 0 {
            entry.push(0);
        }
        std::fs::write(root.join("x/xterm-hotaru"), entry).unwrap();
        let env = HashMap::from([
            ("HOTARU_RESOURCES_DIR".into(), root.as_os_str().into()),
            ("HOTARU_TERMINFO_DIR".into(), root.as_os_str().into()),
            ("TERMINFO_DIRS".into(), "/custom:/second".into()),
            ("ZDOTDIR".into(), "/user/zsh with spaces".into()),
        ]);
        for shell in ["bash", "zsh", "fish"] {
            let mut command = PtyCommand::new(shell);
            command.integrate_shell = true;
            let defaults: HashMap<_, _> = prepare_with(&mut command, &env).into_iter().collect();
            assert_eq!(defaults[OsStr::new("TERM")], "xterm-hotaru");
            assert_eq!(
                defaults[OsStr::new("TERMINFO_DIRS")],
                OsString::from(format!("{}:/custom:/second:", root.display()))
            );
            match shell {
                "bash" => assert_eq!(command.args[0], "--rcfile"),
                "zsh" => {
                    assert!(command.args.is_empty());
                    assert_eq!(
                        defaults[OsStr::new("HOTARU_ZSH_ZDOTDIR")],
                        "/user/zsh with spaces"
                    );
                    assert_eq!(defaults[OsStr::new("HOTARU_ZSH_ZDOTDIR_SET")], "1");
                }
                "fish" => assert_eq!(command.args[0], "--init-command"),
                _ => unreachable!(),
            }
            let mut disabled = PtyCommand::new(shell);
            let defaults: HashMap<_, _> = prepare_with(&mut disabled, &env).into_iter().collect();
            assert!(disabled.args.is_empty());
            assert!(!defaults.contains_key(OsStr::new("ZDOTDIR")));
            assert!(!defaults.contains_key(OsStr::new("HOTARU_ZSH_ZDOTDIR_SET")));
        }
        std::fs::write(root.join("x/xterm-hotaru"), "not a compiled entry").unwrap();
        let mut command = PtyCommand::new("bash");
        assert_eq!(prepare_with(&mut command, &env)[0].1, "xterm-256color");
        std::fs::remove_dir_all(root).unwrap();
    }
}
