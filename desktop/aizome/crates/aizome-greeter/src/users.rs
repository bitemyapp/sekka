//! Who can log in: people's accounts from `/etc/passwd`, between
//! `login.defs`' `UID_MIN` and `UID_MAX`, with a login shell, less the
//! configuration's hidden ones. Their pictures are AccountsService's
//! (`/var/lib/AccountsService/icons/NAME`, which Settings sets); nothing
//! of theirs is read from their homes.

use std::path::{Path, PathBuf};

/// Where AccountsService keeps people's pictures.
pub const ICONS: &str = "/var/lib/AccountsService/icons";

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub name: String,
    /// The full name (GECOS' first field), else the user name.
    pub real_name: String,
    pub uid: u32,
    /// Their picture, if there's one.
    pub icon: Option<PathBuf>,
}

impl User {
    /// What stands in for a missing picture: the first letter of the name.
    pub fn initial(&self) -> String {
        self.real_name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default()
    }
}

/// `UID_MIN` and `UID_MAX` from `login.defs` (1000 and 60000 without).
pub fn uid_range(login_defs: &str) -> (u32, u32) {
    let get = |key: &str, default: u32| {
        login_defs
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| {
                let mut it = l.split_whitespace();
                (it.next() == Some(key)).then(|| it.next().and_then(|v| v.parse().ok())).flatten()
            })
            .unwrap_or(default)
    };
    (get("UID_MIN", 1000), get("UID_MAX", 60000))
}

/// The people in `passwd` who can log in, in its order.
pub fn parse(passwd: &str, (min, max): (u32, u32), hidden: &[String], icons: &Path) -> Vec<User> {
    passwd
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() < 7 {
                return None;
            }
            let (name, uid, gecos, shell) = (f[0], f[2].parse::<u32>().ok()?, f[4], f[6]);
            if uid < min || uid > max || name.is_empty() || hidden.iter().any(|h| h == name) {
                return None;
            }
            let shell_name = shell.rsplit('/').next().unwrap_or("");
            if shell.is_empty() || matches!(shell_name, "nologin" | "false") {
                return None;
            }
            let real = gecos.split(',').next().unwrap_or("").trim();
            let icon = icons.join(name);
            Some(User {
                name: name.to_owned(),
                real_name: if real.is_empty() { name.to_owned() } else { real.to_owned() },
                uid,
                icon: icon.is_file().then_some(icon),
            })
        })
        .collect()
}

/// The people on this system who can log in.
pub fn find(hidden: &[String]) -> Vec<User> {
    let defs = std::fs::read_to_string("/etc/login.defs").unwrap_or_default();
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    parse(&passwd, uid_range(&defs), hidden, Path::new(ICONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_with_a_login_shell_in_range() {
        let passwd = "\
root:x:0:0:System administrator:/root:/run/current-system/sw/bin/bash
nobody:x:65534:65534:Unprivileged account:/var/empty:/run/current-system/sw/bin/nologin
greeter:x:999:998::/var/empty:/run/current-system/sw/bin/nologin
callen:x:1000:100:Chris Allen,,,:/home/callen:/run/current-system/sw/bin/bash
bob:x:1001:100::/home/bob:/bin/fish
svc:x:1002:100:A service:/var/lib/svc:/bin/false
guest:x:1003:100:Guest:/home/guest:/bin/sh
huge:x:70000:100:Out of range:/home/huge:/bin/sh
short:x:1004
";
        let icons = std::env::temp_dir().join(format!("aizome-greeter-icons-{}", std::process::id()));
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("callen"), b"png").unwrap();
        let users = parse(passwd, (1000, 60000), &["guest".to_owned()], &icons);
        let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["callen", "bob"]);
        assert_eq!(users[0].real_name, "Chris Allen");
        assert_eq!(users[0].icon.as_deref(), Some(icons.join("callen").as_path()));
        assert_eq!(users[0].initial(), "C");
        assert_eq!(users[1].real_name, "bob");
        assert_eq!(users[1].icon, None);
        assert_eq!(users[1].initial(), "B");
        std::fs::remove_dir_all(&icons).unwrap();
    }

    #[test]
    fn the_uid_range_comes_from_login_defs() {
        assert_eq!(uid_range(""), (1000, 60000));
        assert_eq!(uid_range("# UID_MIN 5\nUID_MIN\t\t500\nUID_MAX 999\n"), (500, 999));
    }
}
