//! Explicit fixtures use real shells in disposable homes. No host dotfiles are
//! read or changed; run the ignored parent with HOTARU_TEST_FISH configured.
use hotaru_io::{Pty, PtyCommand, PtySize};
use std::io::{self, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires Bash, Zsh, Fish and ncurses tic; disposable-home fixture"]
fn actual_shell_startup() {
    let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources")
        .canonicalize()
        .unwrap();
    let root = std::env::temp_dir().join(format!("hotaru-shell-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let terminfo = root.join("terminfo");
    assert!(
        Command::new("tic")
            .args(["-x", "-o"])
            .arg(&terminfo)
            .arg(resources.join("hotaru.terminfo"))
            .status()
            .unwrap()
            .success()
    );
    let fish = std::env::var_os("HOTARU_TEST_FISH")
        .expect("HOTARU_TEST_FISH must name the real Fish binary");
    let shells = [
        ("bash", "/usr/bin/bash".into()),
        ("zsh", "/usr/bin/zsh".into()),
        ("fish", fish),
    ];
    for (name, shell) in shells {
        for mode in [
            "integrated",
            "hostile",
            "array",
            "hook-reset",
            "disabled",
            "explicit",
        ] {
            if mode == "array" && name != "bash" {
                continue;
            }
            if mode == "hook-reset" && name != "zsh" {
                continue;
            }
            let home = root.join(format!("{name}-{mode}"));
            let cwd = home.join("a b%é");
            std::fs::create_dir_all(&cwd).unwrap();
            std::fs::write(home.join(".bashrc"), "printf 'USER_RC\\n'\nPS1='HOTARU> '\nPROMPT_COMMAND='printf \"USER_PROMPT:%s\\n\" \"$?\"'\n").unwrap();
            if mode == "array" {
                std::fs::write(home.join(".bashrc"), "printf 'USER_RC\\n'\nPS1='HOTARU> '\nPROMPT_COMMAND=('printf \"USER_PROMPT:%s\\n\" \"$?\"' 'printf \"USER_SECOND\\n\"')\n").unwrap();
            }
            let initial = if mode == "hostile" {
                home.join(std::ffi::OsString::from_vec(
                    b"hostile-\x07-\x1b-\x9c-\x9b-\xc2\x9c-\xe6\x97\xa5\xe6\x9c\xac".to_vec(),
                ))
            } else {
                home.clone()
            };
            std::fs::create_dir_all(&initial).unwrap();
            let zdot = home.join("zsh-user");
            std::fs::create_dir_all(&zdot).unwrap();
            // Existing ZDOTDIR survives injection. User .zshenv can also change
            // it before Zsh continues its normal .zshrc chain.
            std::fs::write(zdot.join(".zshenv"), "printf 'USER_ENV\\n'\n").unwrap();
            std::fs::write(zdot.join(".zshrc"), "printf 'USER_RC\\n'\nPS1='HOTARU> '\nprecmd() { printf 'USER_PROMPT:%s\\n' \"$?\"; }\n").unwrap();
            if mode == "hook-reset" {
                std::fs::write(zdot.join(".zshrc"), "printf 'USER_RC\\n'\nPS1='HOTARU> '\nuser_prompt() { printf 'USER_PROMPT:%s\\n' \"$?\"; }\nprecmd_functions=(user_prompt)\nsource \"$HOTARU_RESOURCES_DIR/shell-integration/zsh/hotaru.zsh\"\nsource \"$HOTARU_RESOURCES_DIR/shell-integration/zsh/hotaru.zsh\"\n").unwrap();
            }
            let fish_config = home.join("config/fish");
            std::fs::create_dir_all(&fish_config).unwrap();
            std::fs::write(
                fish_config.join("config.fish"),
                "echo USER_RC\nfunction fish_prompt; printf 'HOTARU> '; end\n",
            )
            .unwrap();
            let script = home.join("commands");
            let text = format!(
                "cd '{}'\nfalse\nprintf 'FINAL_TERM:%s:%s:%s\\n' \"$TERM\" \"$TERM_PROGRAM\" \"$TERM_PROGRAM_VERSION\"\nprintf 'HOOK:%s\\n' \"$_hotaru_{name}_loaded\"\nprintf 'PRESERVED:%s:%s\\n' \"$ENV\" \"$BASH_ENV\"\nexit 0\n",
                cwd.display()
            );
            std::fs::write(&script, text).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "shell_child", "--nocapture"])
                .env("HOTARU_TEST_CHILD", mode)
                .env("HOTARU_TEST_COMMANDS", script)
                .env("HOTARU_TEST_INITIAL_CWD", initial)
                .env("SHELL", shell.clone())
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", home.join("config"))
                .env("XDG_DATA_HOME", home.join("data"))
                .env("XDG_CACHE_HOME", home.join("cache"))
                .env("ZDOTDIR", &zdot)
                .env("HOTARU_RESOURCES_DIR", &resources)
                .env("HOTARU_TERMINFO_DIR", &terminfo)
                .env("BASH_ENV", "original BASH_ENV value")
                .env("ENV", "original ENV value")
                .env_remove("BASHOPTS")
                .env_remove("SHELLOPTS")
                .env_remove("PROMPT_COMMAND")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "{name}/{mode}: {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::fs::write(home.join("output"), &output.stdout).unwrap();
            if mode != "explicit" {
                assert_eq!(
                    text.matches("USER_RC\r\n").count(),
                    1,
                    "{name}/{mode}: {text}"
                );
                assert!(
                    text.contains("FINAL_TERM:xterm-hotaru:Hotaru:0.1.0"),
                    "{name}/{mode}: {text}"
                );
                assert!(
                    text.contains("PRESERVED:original ENV value:original BASH_ENV value"),
                    "{name}/{mode} environment changed: {text}"
                );
            }
            if matches!(mode, "integrated" | "hostile" | "array" | "hook-reset") {
                assert!(text.contains("HOOK:1\r\n"), "{name} hook absent: {text}");
                assert!(text.contains("\x1b]7;file://"), "{name}: {text}");
                assert!(text.contains("/a%20b%25%C3%A9\x07"), "{name}: {text}");
                if name != "fish" {
                    assert!(text.contains("USER_PROMPT:1"), "{name} status: {text}");
                }
                if mode == "hostile" {
                    assert!(
                        text.contains("hostile-%07-%1B-%9C-%9B-%C2%9C-%E6%97%A5%E6%9C%AC\x07"),
                        "{name} unsafe path encoding: {text}"
                    );
                }
            } else {
                assert!(
                    !text.contains("HOOK:1\r\n"),
                    "{name}/{mode} hook injected: {text}"
                );
                if name != "fish" || mode == "explicit" {
                    assert!(
                        !text.contains("\x1b]7;file://"),
                        "{name}/{mode} injected: {text}"
                    );
                }
            }
            if name == "zsh" && mode != "explicit" {
                assert_eq!(
                    text.matches("USER_ENV\r\n").count(),
                    1,
                    "{name}/{mode}: {text}"
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "isolated subprocess helper"]
fn shell_child() {
    let Some(mode) = std::env::var_os("HOTARU_TEST_CHILD") else {
        return;
    };
    let mut command = PtyCommand::shell();
    command.cwd = std::env::var_os("HOTARU_TEST_INITIAL_CWD").map(PathBuf::from);
    if mode == "disabled" {
        command.integrate_shell = false;
    } else if mode == "explicit" {
        command = PtyCommand::new(command.program);
        command.args = vec!["-c".into(), "printf EXPLICIT_UNCHANGED".into()];
    }
    let input = std::fs::read(std::env::var_os("HOTARU_TEST_COMMANDS").unwrap()).unwrap();
    let mut pty = Pty::spawn(command, PtySize::default()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    let mut terminal = hotaru_core::Terminal::new(80, 24, 0);
    let mut responses = Vec::new();
    let mut response_at = 0;
    let mut sent = 0;
    let mut buffer = [0; 8192];
    loop {
        assert!(
            Instant::now() < deadline,
            "shell timed out: {}",
            String::from_utf8_lossy(&output)
        );
        match pty.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                output.extend_from_slice(&buffer[..n]);
                terminal.feed(&buffer[..n]);
                responses.extend(terminal.take_responses());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
        if response_at < responses.len() {
            match pty.write(&responses[response_at..]) {
                Ok(n) => response_at += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
        } else if mode != "explicit"
            && output.windows(8).any(|w| w == b"HOTARU> ")
            && sent < input.len()
        {
            match pty.write(&input[sent..]) {
                Ok(n) => sent += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
        }
        if pty.try_wait().unwrap().is_some() {
            // Drain buffered final output before the EIO/EOF read.
            while let Ok(n) = pty.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                output.extend_from_slice(&buffer[..n]);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    std::io::stdout().write_all(&output).unwrap();
}
