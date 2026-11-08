// SPDX-License-Identifier: MIT OR Apache-2.0
//! The computer's people: the accounts someone logs in to, as
//! `/etc/passwd` lists them. A choice such as who logs in by themselves
//! (`people` in [`Choice`](crate::setup::Choice)) may name any of them,
//! including people added after the system was built.

use std::path::Path;

/// Where the accounts are.
pub const PASSWD: &str = "/etc/passwd";

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

/// Whether `name` is one of the computer's people now.
pub fn is_person(name: &str) -> bool {
    read().iter().any(|p| p.name == name)
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
}
