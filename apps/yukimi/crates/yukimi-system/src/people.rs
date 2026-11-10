// SPDX-License-Identifier: MIT OR Apache-2.0
//! The computer's people: the accounts someone logs in to, as
//! `/etc/passwd` lists them. A choice such as who logs in by themselves
//! (`people` in [`Choice`](crate::setup::Choice)) may name any of them,
//! including people added after the system was built.
//!
//! Logging in by itself asks nothing, so it can't take an account that
//! has something to ask or can't be logged in to at all, and it isn't for
//! one that was locked: root, which reads `/etc/shadow`, checks that when
//! it writes the choice ([`takes`]).

use std::path::Path;

/// Where the accounts are.
pub const PASSWD: &str = "/etc/passwd";
/// Where each account's password, its age and the account's expiry are,
/// for root.
pub const SHADOW: &str = "/etc/shadow";

/// A person's account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    /// The name they log in with.
    pub name: String,
    /// Their full name, when the account gives one.
    pub full_name: Option<String>,
}

/// The computer's people, in the order `/etc/passwd` has them.
pub fn read() -> Vec<Person> {
    read_from(Path::new(PASSWD))
}

/// The people of the `passwd` file at `path`, such as another system's.
pub fn read_from(path: &Path) -> Vec<Person> {
    std::fs::read_to_string(path).map(|text| parse(&text)).unwrap_or_default()
}

/// Whether `name` can log in by themselves now: one of the computer's
/// people, whose account can open a session without anyone being asked
/// anything. Otherwise, why not, in words for whoever chose them.
pub fn takes(name: &str) -> Result<(), String> {
    let today = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| (since.as_secs() / 86_400) as i64);
    takes_from(Path::new(PASSWD), Path::new(SHADOW), today, name)
}

/// As [`takes`], with the `passwd` and `shadow` files to read and the day
/// (days since 1970).
pub fn takes_from(passwd: &Path, shadow: &Path, today: i64, name: &str) -> Result<(), String> {
    if !read_from(passwd).iter().any(|p| p.name == name) {
        return Err(format!("{name:?} isn't one of the people on this computer"));
    }
    // Without a shadow file, or an entry in it, nothing ages or expires
    // the account.
    let shadow = std::fs::read_to_string(shadow).unwrap_or_default();
    match blocked(&shadow, name, today) {
        Some(Blocked::Expired) => {
            Err(format!("{name}'s account has expired, so the computer can't start straight into it"))
        }
        Some(Blocked::MustSetPassword) => Err(format!(
            "{name} has to set a password at their next login, so the computer can't start straight into their \
             account. Set their password first"
        )),
        Some(Blocked::Locked) => Err(format!(
            "{name}'s account is locked, so the computer won't be set to start straight into it. Unlock it first"
        )),
        None => Ok(()),
    }
}

/// Why an account isn't taken to log in by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// The account has expired: PAM's account check fails, and no session
    /// opens.
    Expired,
    /// A password has to be set at the next login: a new account made
    /// without one (`chage -d 0`, as Settei leaves it), or a password older
    /// than the account allows. PAM's account check asks for it, which an
    /// automatic login can't answer, and no session opens.
    MustSetPassword,
    /// Its password was locked (`passwd -l`, `usermod -L`, and so
    /// AccountsService and GNOME's settings): someone meant this account
    /// not to be logged in to. PAM's account check passes all the same, so
    /// an automatic login would open its session; the choice isn't made
    /// for it.
    Locked,
}

/// Why the account `name` of a `shadow` file isn't taken to log in by
/// itself on day `today`. First by the rules of PAM's account check
/// (pam_unix, which every login screen's automatic login still runs): an
/// expired account, or a password to set or change. Then a locked
/// password: `!` before a hash. A `!` alone isn't one: NixOS writes that
/// for an account declared without a password, which is one way to make an
/// account only an automatic login reaches.
pub fn blocked(shadow: &str, name: &str, today: i64) -> Option<Blocked> {
    let line = shadow.lines().find(|line| line.split(':').next() == Some(name))?;
    let fields: Vec<&str> = line.split(':').collect();
    // An empty field is unset, which shadow(5) gives as -1.
    let days = |i: usize| fields.get(i).and_then(|f| f.parse::<i64>().ok()).unwrap_or(-1);
    let (changed, most, expires) = (days(2), days(4), days(7));
    if expires != -1 && today >= expires {
        return Some(Blocked::Expired);
    }
    if changed == 0 {
        return Some(Blocked::MustSetPassword);
    }
    // Changed in the future (a clock set back since): as it is.
    if today >= changed && most != -1 && today - changed > most {
        return Some(Blocked::MustSetPassword);
    }
    if fields.get(1).is_some_and(|password| password.len() > 1 && password.starts_with('!')) {
        return Some(Blocked::Locked);
    }
    None
}

/// The people of a `passwd` file: accounts from uid 1000 to 59999 (people,
/// not the system's own users, Nix's builders or nobody) that have a shell
/// to log in with (none given is `/bin/sh`), and a name a person's account
/// can have.
pub fn parse(passwd: &str) -> Vec<Person> {
    passwd
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let [name, _, uid, _, gecos, _, shell] = fields[..] else { return None };
            let uid: u32 = uid.parse().ok()?;
            let shell = if shell.is_empty() { "sh" } else { shell.rsplit('/').next().unwrap_or("") };
            let logs_in = !shell.is_empty() && !matches!(shell, "nologin" | "false");
            ((1000..60000).contains(&uid) && logs_in && valid_name(name)).then(|| Person {
                name: name.to_owned(),
                full_name: gecos.split(',').next().filter(|n| !n.trim().is_empty()).map(|n| n.trim().to_owned()),
            })
        })
        .collect()
}

/// A name an account can have: a lowercase letter or `_`, then lowercase
/// letters, digits, `_` and `-`, at most 32 in all (as `useradd` takes).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_are_accounts_someone_logs_in_to() {
        let passwd = "\
root:x:0:0:System administrator:/root:/run/current-system/sw/bin/bash
greeter:x:999:999::/var/empty:/run/current-system/sw/bin/bash
adama:x:1000:100:William Adama,,,:/home/adama:/run/current-system/sw/bin/fish
kara:x:1001:100::/home/kara:/run/current-system/sw/bin/bash
nixbld1:x:30001:30000:Nix build user 1:/var/empty:/run/current-system/sw/bin/nologin
locked:x:1002:100::/home/locked:/run/current-system/sw/bin/nologin
nope:x:1003:100::/home/nope:/bin/false
Odd.Name:x:1004:100::/home/odd:/bin/sh
sam:x:1005:100::/home/sam:
nobody:x:65534:65534:Unprivileged account:/var/empty:/run/current-system/sw/bin/nologin
broken line
";
        let people = parse(passwd);
        let names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
        // sam has no shell given, which is /bin/sh.
        assert_eq!(names, ["adama", "kara", "sam"]);
        assert_eq!(people[0].full_name.as_deref(), Some("William Adama"));
        assert_eq!(people[1].full_name, None);
    }

    #[test]
    fn names_are_what_useradd_takes() {
        for good in ["adama", "_svc", "kara-thrace", "a1", "x_y"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "Adama", "1st", "-x", "a.b", "a b", "a\"b", &"a".repeat(33)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    /// Day 20767 is 2026-11-10.
    const TODAY: i64 = 20767;

    #[test]
    fn an_account_that_is_not_to_log_in_by_itself_says_why() {
        // As NixOS, useradd, AccountsService and chage leave them.
        let shadow = "\
root:!:1::::::
adama:$y$j9T$salt$hash:1::::::
kara:$y$j9T$salt$hash:20767:0:99999:7:::
new::0:0:99999:7:::
expired-and-locked:!$y$j9T$salt$hash:20767:0:99999:7::1:
leaves-tomorrow:$y$j9T$salt$hash:20767:0:99999:7::20768:
left-today:$y$j9T$salt$hash:20767:0:99999:7::20767:
no-password:!:1::::::
locked:!$y$j9T$salt$hash:20767:0:99999:7:::
aged:$y$j9T$salt$hash:20000:0:90:7:::
fresh:$y$j9T$salt$hash:20700:0:90:7:::
from-the-future:$y$j9T$salt$hash:20900:0:90:7:::
";
        let why = |name: &str| blocked(shadow, name, TODAY);
        assert_eq!(why("adama"), None);
        assert_eq!(why("kara"), None);
        // Made without a password (AccountsService's SetPasswordMode(1)).
        assert_eq!(why("new"), Some(Blocked::MustSetPassword));
        // An expiry date, come or to come.
        assert_eq!(why("expired-and-locked"), Some(Blocked::Expired));
        assert_eq!(why("leaves-tomorrow"), None);
        assert_eq!(why("left-today"), Some(Blocked::Expired));
        // A password that was locked (AccountsService's SetLocked, passwd
        // -l). Declared without one, as NixOS writes it, is no lock.
        assert_eq!(why("locked"), Some(Blocked::Locked));
        assert_eq!(why("no-password"), None);
        // A password older than the account allows has to be changed.
        assert_eq!(why("aged"), Some(Blocked::MustSetPassword));
        assert_eq!(why("fresh"), None);
        assert_eq!(why("from-the-future"), None);
        // No entry: nothing to stop it.
        assert_eq!(why("nobody-here"), None);
    }

    #[test]
    fn only_people_who_can_log_in_by_themselves_are_taken() {
        let dir = std::env::temp_dir().join(format!("yukimi-people-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (passwd, shadow) = (dir.join("passwd"), dir.join("shadow"));
        let accounts =
            ["kara", "new", "gaius", "leoben"].map(|name| format!("{name}:x:1001:100::/home/{name}:/bin/sh\n"));
        std::fs::write(&passwd, accounts.concat()).unwrap();
        std::fs::write(
            &shadow,
            "kara:$y$hash:20767::::::\nnew::0:0:99999:7:::\ngaius:!$y$hash:20767::::::\nleoben:$y$hash:20767:::::1:\n",
        )
        .unwrap();
        let takes = |name: &str| takes_from(&passwd, &shadow, TODAY, name);
        assert_eq!(takes("kara"), Ok(()));
        assert!(takes("new").unwrap_err().contains("has to set a password"));
        assert!(takes("gaius").unwrap_err().contains("account is locked"));
        assert!(takes("leoben").unwrap_err().contains("account has expired"));
        assert!(takes("ghost").unwrap_err().contains("isn't one of the people"));
        // No shadow file: the accounts are as passwd has them.
        assert_eq!(takes_from(&passwd, &dir.join("absent"), TODAY, "new"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
