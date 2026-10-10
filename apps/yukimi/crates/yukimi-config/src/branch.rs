// SPDX-License-Identifier: MIT OR Apache-2.0
//! Which branch a flake input follows, and the same address following
//! another: `github:bitemyapp/tatami/stable` following `main` is
//! `github:bitemyapp/tatami/main`. GitHub, GitLab and SourceHut addresses
//! name the branch after the repository (or as `?ref=` when it has a slash in
//! it); Git addresses as `?ref=`. Other kinds of address have no branch to
//! choose.

const FORGES: [&str; 3] = ["github:", "gitlab:", "sourcehut:"];

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

/// The address and its query, as `(key, value)` pairs.
fn split(url: &str) -> (&str, Vec<(String, String)>) {
    let (base, query) = url.split_once('?').map_or((url, ""), |(b, q)| (b, q));
    let params = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').map_or((p.to_owned(), String::new()), |(k, v)| (k.to_owned(), v.to_owned())))
        .collect();
    (base, params)
}

fn join(base: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return base.to_owned();
    }
    let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{base}?{}", query.join("&"))
}

/// The branch an address follows, or `None` for the repository's default
/// branch (or an address that names none).
pub fn of(url: &str) -> Option<String> {
    let (base, params) = split(url);
    if let Some((_, branch)) = params.iter().find(|(k, _)| k == "ref") {
        return Some(branch.clone());
    }
    let forge = FORGES.iter().find(|f| base.starts_with(**f))?;
    let mut parts = base[forge.len()..].splitn(3, '/');
    let (_, _, branch) = (parts.next()?, parts.next()?, parts.next()?);
    Some(branch.to_owned())
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
    let (base, mut params) = split(url);
    params.retain(|(k, _)| k != "ref" && k != "rev");
    if let Some(forge) = FORGES.iter().find(|f| base.starts_with(**f)) {
        let mut parts = base[forge.len()..].split('/');
        let (owner, repo) = (parts.next().filter(|o| !o.is_empty())?, parts.next().filter(|r| !r.is_empty())?);
        if branch.contains('/') {
            params.insert(0, ("ref".to_owned(), branch.to_owned()));
            return Some(join(&format!("{forge}{owner}/{repo}"), &params));
        }
        return Some(join(&format!("{forge}{owner}/{repo}/{branch}"), &params));
    }
    if base.starts_with("git+") {
        params.insert(0, ("ref".to_owned(), branch.to_owned()));
        return Some(join(base, &params));
    }
    None
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
        assert_eq!(with("github:o/r/main", "feature/x").as_deref(), Some("github:o/r?ref=feature/x"));
        assert_eq!(with("github:o/r/abc?rev=0123", "main").as_deref(), Some("github:o/r/main"));
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
