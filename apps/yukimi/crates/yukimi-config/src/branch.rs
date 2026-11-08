// SPDX-License-Identifier: MIT OR Apache-2.0
//! Which branch a flake input follows, and the same address following
//! another: `github:bitemyapp/tatami/stable` following `main` is
//! `github:bitemyapp/tatami/main`. GitHub, GitLab and SourceHut addresses
//! name the branch after the repository (or as `?ref=` when it has a slash in
//! it); Git addresses as `?ref=`. Other kinds of address have no branch to
//! choose.

use crate::flake_url::FlakeUrl;

/// Whether a name is one Yukimi will write as a branch.
pub fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        && !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
}

/// The branch an address follows, or `None` for the repository's default
/// branch (or an address that names none).
pub fn of(url: &str) -> Option<String> {
    FlakeUrl::parse(url)?.reference
}

/// Whether Yukimi can make this address follow another branch.
pub fn changeable(url: &str) -> bool {
    with(url, "main").is_some()
}

/// The address following `branch` instead, pinned to no commit.
pub fn with(url: &str, branch: &str) -> Option<String> {
    if !valid(branch) {
        return None;
    }
    FlakeUrl::parse(url)?.with_branch(branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_of_addresses() {
        assert_eq!(of("github:bitemyapp/tatami/stable").as_deref(), Some("stable"));
        assert_eq!(of("github:bitemyapp/tatami"), None);
        assert_eq!(of("github:o/r?ref=feature/x&dir=sub").as_deref(), Some("feature/x"));
        assert_eq!(of("git+https://example.org/r.git?ref=dev").as_deref(), Some("dev"));
        assert_eq!(of("https://flakehub.com/f/NixOS/nixpkgs/0.1"), None);
    }

    #[test]
    fn following_another_branch() {
        assert_eq!(with("github:bitemyapp/tatami/stable", "main").as_deref(), Some("github:bitemyapp/tatami/main"));
        assert_eq!(with("github:bitemyapp/tatami", "main").as_deref(), Some("github:bitemyapp/tatami/main"));
        assert_eq!(with("gitlab:o/r/v1?dir=sub", "dev").as_deref(), Some("gitlab:o/r/dev?dir=sub"));
        assert_eq!(with("github:o/r/main", "feature/x").as_deref(), Some("github:o/r?ref=feature%2Fx"));
        assert_eq!(
            with("github:o/r/2111c84ac0b4555518b9482ec436f813d26905ef", "main").as_deref(),
            Some("github:o/r/main")
        );
        assert_eq!(
            with("git+https://example.org/r.git?ref=dev&submodules=1", "main").as_deref(),
            Some("git+https://example.org/r.git?ref=main&submodules=1")
        );
        assert_eq!(with("https://flakehub.com/f/NixOS/nixpkgs/0.1", "main"), None);
        assert!(changeable("github:o/r") && !changeable("path:/home/a/x"));
        for bad in ["", "-x", "a..b", "a b", "x.lock", "a//b", "/a", "a/", "$(x)"] {
            assert!(with("github:o/r", bad).is_none(), "{bad}");
        }
    }
}
