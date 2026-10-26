//! The sessions there are to log in to: the desktop entries in the
//! session directories (`wayland-sessions`, `xsessions`) the configuration
//! names. On Sekka those hold one entry for each desktop chosen (Aizome,
//! Tatami, Plasma, GNOME, Xfce, Hyprland…); elsewhere every session NixOS
//! installs, less any hidden.

use std::path::{Path, PathBuf};

/// How a session runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Wayland,
    /// Started under an X server, through the configuration's `x11_prefix`.
    X11,
}

/// One session to log in to.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The entry's file name without `.desktop` (`aizome`, `hyprland-uwsm`):
    /// what's remembered and what `default_session` names.
    pub id: String,
    pub name: String,
    pub comment: String,
    /// What to run, unquoted, its field codes removed.
    pub exec: Vec<String>,
    /// `DesktopNames`, for `XDG_CURRENT_DESKTOP`.
    pub desktop_names: Vec<String>,
    pub kind: Kind,
    pub path: PathBuf,
}

impl Session {
    /// The command greetd runs, X11 sessions under `x11_prefix`.
    pub fn command(&self, x11_prefix: &[String]) -> Vec<String> {
        match self.kind {
            Kind::Wayland => self.exec.clone(),
            Kind::X11 => x11_prefix.iter().cloned().chain(self.exec.iter().cloned()).collect(),
        }
    }

    /// The environment the session starts with: its type and desktop, as a
    /// display manager sets them (pam_systemd reads `XDG_SESSION_TYPE` and
    /// `XDG_SESSION_DESKTOP`).
    pub fn env(&self) -> Vec<String> {
        let mut env = vec![
            format!("XDG_SESSION_TYPE={}", if self.kind == Kind::Wayland { "wayland" } else { "x11" }),
            format!("XDG_SESSION_DESKTOP={}", self.id),
            "XDG_SESSION_CLASS=user".to_owned(),
        ];
        if !self.desktop_names.is_empty() {
            env.push(format!("XDG_CURRENT_DESKTOP={}", self.desktop_names.join(":")));
        }
        env
    }
}

/// Every session in `dirs` (a directory named `xsessions` holds X11 ones),
/// less `hidden` and entries that say not to show or whose `TryExec` isn't
/// there; one per id, the first directory's winning; sorted by name.
/// `x11` false leaves X11 sessions out (no way to start them).
pub fn find(dirs: &[PathBuf], hidden: &[String], x11: bool) -> Vec<Session> {
    let mut out: Vec<Session> = Vec::new();
    for dir in dirs {
        let kind = if dir.file_name().is_some_and(|n| n == "xsessions") { Kind::X11 } else { Kind::Wayland };
        if kind == Kind::X11 && !x11 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
            if hidden.contains(&id) || out.iter().any(|s| s.id == id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if let Some(s) = parse(&text, &id, kind, &path) {
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    out
}

/// A desktop entry, if it's a session to show.
pub fn parse(text: &str, id: &str, kind: Kind, path: &Path) -> Option<Session> {
    let mut in_entry = false;
    let (mut name, mut comment, mut exec, mut try_exec, mut names) = (None, None, None, None, None);
    let mut hidden = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Name" => name = Some(unescape(value)),
            "Comment" => comment = Some(unescape(value)),
            "Exec" => exec = Some(value.to_owned()),
            "TryExec" => try_exec = Some(unescape(value)),
            "DesktopNames" => names = Some(value.to_owned()),
            "Hidden" | "NoDisplay" if value == "true" => hidden = true,
            _ => {}
        }
    }
    if hidden {
        return None;
    }
    if let Some(t) = try_exec
        && !program_exists(&t)
    {
        return None;
    }
    let exec = split_exec(&exec?)?;
    Some(Session {
        id: id.to_owned(),
        name: name.unwrap_or_else(|| id.to_owned()),
        comment: comment.unwrap_or_default(),
        exec,
        desktop_names: names
            .map(|n| n.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default(),
        kind,
        path: path.to_owned(),
    })
}

/// A string value's escapes (`\s`, `\n`, `\t`, `\r`, `\\`).
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Not a string escape: Exec's quoting reads it next.
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `Exec` as arguments: split at spaces, double quotes grouping (inside
/// them `\"`, `` \` ``, `\$` and `\\` escape), field codes (`%f`, `%U`…)
/// dropped and `%%` a percent sign. `None` if it's malformed or empty.
pub fn split_exec(exec: &str) -> Option<Vec<String>> {
    let exec = unescape(exec);
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next() {
                Some(e @ ('"' | '`' | '$' | '\\')) => cur.push(e),
                Some(o) => {
                    cur.push('\\');
                    cur.push(o);
                }
                None => return None,
            },
            ' ' | '\t' if !quoted => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            '%' if !quoted => match chars.next() {
                Some('%') => {
                    cur.push('%');
                    started = true;
                }
                // A field code: nothing for a session.
                Some(_) => {}
                None => return None,
            },
            o => {
                cur.push(o);
                started = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if started {
        args.push(cur);
    }
    (!args.is_empty()).then_some(args)
}

/// Whether `program` is a path to something executable, or found on `$PATH`.
fn program_exists(program: &str) -> bool {
    let executable = |p: &Path| {
        std::fs::metadata(p).is_ok_and(|m| {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        })
    };
    if program.contains('/') {
        return executable(Path::new(program));
    }
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| executable(&d.join(program)))).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_split_as_the_spec_says() {
        let s = |e: &str| split_exec(e).unwrap();
        assert_eq!(s("/run/current-system/sw/bin/aizome-session"), ["/run/current-system/sw/bin/aizome-session"]);
        assert_eq!(
            s("uwsm start -e -D Hyprland hyprland.desktop"),
            ["uwsm", "start", "-e", "-D", "Hyprland", "hyprland.desktop"]
        );
        assert_eq!(s(r#"sh -c "echo \"hi\" \$HOME""#), ["sh", "-c", r#"echo "hi" $HOME"#]);
        assert_eq!(s("startplasma-wayland %U"), ["startplasma-wayland"]);
        assert_eq!(s("a 100%% b"), ["a", "100%", "b"]);
        assert_eq!(s(r#"a "" b"#), ["a", "", "b"]);
        assert_eq!(s("  spaced   out  "), ["spaced", "out"]);
        assert!(split_exec(r#"unterminated "quote"#).is_none());
        assert!(split_exec("   ").is_none());
    }

    #[test]
    fn an_entry_reads_into_a_session() {
        let text = "[Desktop Entry]\nName=Aizome\nComment=Sekka's own desktop\nExec=/run/current-system/sw/bin/aizome-session\nType=Application\nDesktopNames=Aizome\n\n[Desktop Action other]\nName=Not this\n";
        let s = parse(text, "aizome", Kind::Wayland, Path::new("/x/aizome.desktop")).unwrap();
        assert_eq!(s.name, "Aizome");
        assert_eq!(s.comment, "Sekka's own desktop");
        assert_eq!(s.exec, ["/run/current-system/sw/bin/aizome-session"]);
        assert_eq!(s.desktop_names, ["Aizome"]);
        assert_eq!(
            s.env(),
            [
                "XDG_SESSION_TYPE=wayland",
                "XDG_SESSION_DESKTOP=aizome",
                "XDG_SESSION_CLASS=user",
                "XDG_CURRENT_DESKTOP=Aizome"
            ]
        );
        assert_eq!(s.command(&["startx".into()]), ["/run/current-system/sw/bin/aizome-session"]);
        // Hidden, or whose TryExec isn't there: not shown.
        assert!(parse("[Desktop Entry]\nName=A\nExec=a\nHidden=true\n", "a", Kind::Wayland, Path::new("a")).is_none());
        assert!(
            parse("[Desktop Entry]\nName=A\nExec=a\nNoDisplay=true\n", "a", Kind::Wayland, Path::new("a")).is_none()
        );
        assert!(
            parse("[Desktop Entry]\nName=A\nExec=a\nTryExec=/nonexistent/a\n", "a", Kind::Wayland, Path::new("a"))
                .is_none()
        );
        assert!(parse("[Desktop Entry]\nName=No exec\n", "a", Kind::Wayland, Path::new("a")).is_none());
    }

    #[test]
    fn an_x11_session_starts_under_the_prefix() {
        let s = parse(
            "[Desktop Entry]\nName=Xfce Session\nExec=xfce4-session\nDesktopNames=XFCE\n",
            "xfce",
            Kind::X11,
            Path::new("x"),
        )
        .unwrap();
        let prefix = ["startx".to_owned(), "/usr/bin/env".to_owned()];
        assert_eq!(s.command(&prefix), ["startx", "/usr/bin/env", "xfce4-session"]);
        assert!(s.env().contains(&"XDG_SESSION_TYPE=x11".to_owned()));
    }

    #[test]
    fn sessions_are_found_hidden_and_deduplicated() {
        let dir = std::env::temp_dir().join(format!("aizome-greeter-sessions-{}", std::process::id()));
        let (w, x) = (dir.join("wayland-sessions"), dir.join("xsessions"));
        std::fs::create_dir_all(&w).unwrap();
        std::fs::create_dir_all(&x).unwrap();
        let entry = |name: &str, exec: &str| format!("[Desktop Entry]\nName={name}\nExec={exec}\n");
        std::fs::write(w.join("tatami.desktop"), entry("Tatami", "tatami-session")).unwrap();
        std::fs::write(w.join("aizome.desktop"), entry("Aizome", "aizome-session")).unwrap();
        std::fs::write(w.join("hyprland.desktop"), entry("Hyprland", "Hyprland")).unwrap();
        std::fs::write(x.join("xfce.desktop"), entry("Xfce Session", "xfce4-session")).unwrap();
        // The same id under xsessions loses to the Wayland one.
        std::fs::write(x.join("aizome.desktop"), entry("Aizome (X11)", "nope")).unwrap();
        std::fs::write(w.join("README"), "not an entry").unwrap();
        let found = find(&[w.clone(), x.clone()], &["hyprland".into()], true);
        let ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["aizome", "tatami", "xfce"]);
        assert_eq!(found[2].kind, Kind::X11);
        // No way to start X11: its sessions aren't offered.
        let found = find(&[w, x], &[], false);
        assert!(found.iter().all(|s| s.kind == Kind::Wayland));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
