// SPDX-License-Identifier: MIT OR Apache-2.0
//! The forge and Git flake URLs Yukimi edits. RFC 3986 syntax and encoding
//! belong to fluent-uri; Nix's ref/rev and query semantics belong here.
//! Malformed URIs and unsupported schemes are never rewritten.
use std::collections::BTreeMap;

use fluent_uri::{Uri, build::Builder, component::Scheme, pct_enc::EStr};

#[derive(Debug, PartialEq, Eq)]
enum Source {
    Forge { owner: String, repo: String },
    Git,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefLocation {
    Path,
    Query,
}

#[derive(Debug)]
pub(crate) struct FlakeUrl {
    uri: Uri<String>,
    source: Source,
    pub reference: Option<String>,
    pub revision: Option<String>,
    pub ref_location: RefLocation,
    pub options: BTreeMap<String, String>,
}

fn revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

use crate::url::{github_repository, path, query};

impl FlakeUrl {
    /// A displayable source address from the lock's typed metadata. Commit
    /// pins remain separate metadata; callers must not parse this label back
    /// to discover the branch or repository.
    pub fn source(locked: &crate::lock::Locked) -> Option<Self> {
        let uri = match locked.kind.as_str() {
            "github" | "gitlab" | "sourcehut" => Uri::builder()
                .scheme(Scheme::new(&locked.kind)?)
                .path(&path(&[locked.owner.as_deref()?, locked.repo.as_deref()?]))
                .build()
                .ok()?,
            "git" => {
                let transport = Uri::parse(locked.url.as_deref()?).ok()?;
                if transport.fragment().is_some() {
                    return None;
                }
                let scheme = match transport.scheme().as_str() {
                    "git" => "git".to_owned(),
                    "http" | "https" | "ssh" | "file" => format!("git+{}", transport.scheme()),
                    _ => return None,
                };
                Uri::builder()
                    .scheme(Scheme::new(&scheme)?)
                    .optional(Builder::authority, transport.authority())
                    .path(transport.path())
                    .optional(Builder::query, transport.query())
                    .build()
                    .ok()?
            }
            _ => return None,
        };
        let mut parsed = Self::parse(uri.as_str())?;
        parsed.reference = locked.reference.clone();
        parsed.revision = None;
        if matches!(parsed.source, Source::Forge { .. }) {
            parsed.ref_location = if locked.reference.as_deref().is_some_and(|r| r.contains('/') || revision(r)) {
                RefLocation::Query
            } else {
                RefLocation::Path
            };
            if let Some(host) = &locked.host {
                parsed.options.insert("host".into(), host.clone());
            }
        }
        Some(parsed)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let uri = Uri::parse(value).ok()?.to_owned();
        // Fragments select a flake output, not an input source.
        if uri.fragment().is_some() {
            return None;
        }
        let mut options = BTreeMap::new();
        if let Some(query) = uri.query() {
            for param in query.split('&') {
                // Nix ignores bare keys and keeps the first duplicate. This
                // is URI encoding, not application/x-www-form-urlencoded:
                // decoding a literal '+' must not turn it into a space.
                if let Some((key, value)) = param.split_once('=') {
                    options
                        .entry(key.decode().to_string().ok()?.into_owned())
                        .or_insert(value.decode().to_string().ok()?.into_owned());
                }
            }
        }
        let mut reference = options.remove("ref");
        let mut rev = options.remove("rev");
        let mut ref_location = RefLocation::Query;
        let source = match uri.scheme().as_str() {
            "github" | "gitlab" | "sourcehut" => {
                if uri.authority().is_some() {
                    return None;
                }
                // Split BEFORE decoding: an escaped slash is part of a
                // component, not a separator between owner and repository.
                let parts = uri
                    .path()
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.decode().to_string().ok().map(|s| s.into_owned()))
                    .collect::<Option<Vec<_>>>()?;
                let [owner, repo, tail @ ..] = parts.as_slice() else { return None };
                if !tail.is_empty() {
                    if reference.is_some() || rev.is_some() {
                        return None;
                    }
                    let selector = tail.join("/");
                    if tail.len() == 1 && revision(&selector) {
                        rev = Some(selector);
                    } else {
                        reference = Some(selector);
                        ref_location = RefLocation::Path;
                    }
                }
                if reference.is_some() && rev.is_some() {
                    return None;
                }
                Source::Forge { owner: owner.clone(), repo: repo.clone() }
            }
            "git" | "git+http" | "git+https" | "git+ssh" | "git+file" => Source::Git,
            _ => return None,
        };
        if rev.as_deref().is_some_and(|r| !revision(r)) || reference.as_deref() == Some("") {
            return None;
        }
        Some(Self { uri, source, reference, revision: rev, ref_location, options })
    }

    pub fn is_github(&self) -> bool {
        self.uri.scheme().as_str() == "github"
    }

    pub fn is_official(&self, owner: &str, repo: &str) -> bool {
        match &self.source {
            Source::Forge { owner: actual_owner, repo: actual_repo } => {
                self.is_github()
                    && actual_owner == owner
                    && actual_repo == repo
                    && self.options.get("host").is_none_or(|host| host == "github.com")
            }
            // The same repository rule as lock metadata's (`Locked::github_repo`).
            Source::Git => {
                self.uri.scheme().as_str() == "git+https"
                    && self
                        .uri
                        .as_str()
                        .strip_prefix("git+")
                        .and_then(github_repository)
                        .is_some_and(|(actual_owner, actual_repo)| actual_owner == owner && actual_repo == repo)
            }
        }
    }

    /// Change a branch explicitly, removing any commit pin. Keep all other
    /// fetcher/transport options; serialize their decoded values exactly once.
    pub fn with_branch(mut self, branch: &str) -> Option<String> {
        self.reference = Some(branch.to_owned());
        self.revision = None;
        self.ref_location = if matches!(self.source, Source::Forge { .. }) && !branch.contains('/') && !revision(branch)
        {
            RefLocation::Path
        } else {
            RefLocation::Query
        };
        self.render()
    }

    pub fn render(&self) -> Option<String> {
        let mut options = self.options.clone();
        let mut segments = Vec::new();
        if let Source::Forge { owner, repo } = &self.source {
            segments.extend([owner.as_str(), repo.as_str()]);
        }
        if let Some(reference) = &self.reference {
            if matches!(self.source, Source::Forge { .. }) && self.ref_location == RefLocation::Path {
                segments.extend(reference.split('/'));
            } else {
                options.insert("ref".into(), reference.clone());
            }
        }
        if let Some(rev) = &self.revision {
            options.insert("rev".into(), rev.clone());
        }
        let forge_path = path(&segments);
        let query = query(&options);
        Uri::builder()
            .scheme(self.uri.scheme())
            .optional(Builder::authority, self.uri.authority())
            .path(if matches!(self.source, Source::Forge { .. }) { &forge_path } else { self.uri.path() })
            .optional(Builder::query, (!query.is_empty()).then_some(query.as_ref()))
            .build()
            .ok()
            .map(|uri| uri.into_string())
    }

    /// Construct the Git transport for a known branch of a GitHub repository.
    pub fn github_git(&self, branch: &str) -> Option<String> {
        let Source::Forge { owner, repo } = &self.source else { return None };
        if !self.is_github() || !self.options.is_empty() || self.revision.is_some() {
            return None;
        }
        let path = path(&["", owner, repo]);
        let query = query(&BTreeMap::from([("ref".into(), branch.into()), ("shallow".into(), "1".into())]));
        Uri::builder()
            .scheme(Scheme::new_or_panic("git+https"))
            .authority_with(|b| b.host(EStr::new_or_panic("github.com")))
            .path(&path)
            .query(&query)
            .build()
            .ok()
            .map(|uri| uri.into_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "2111c84ac0b4555518b9482ec436f813d26905ef";
    const URLS: &[&str] = &[
        "github:bitemyapp/sekka",
        "github:bit%65myapp/sekka/%6dain",
        "github:bitemyapp/sekka?%72ef=main",
        "github:o/r?ref=feature%2Fx&dir=sub%20dir",
        "github:o/r/topic+name?dir=a%2Bb%26c%3Dd",
        "github:o/r?ref=first&ref=last&bare&dir=x",
        "github:o/r/2111c84ac0b4555518b9482ec436f813d26905ef",
        "github:o/r?rev=2111c84ac0b4555518b9482ec436f813d26905ef",
        "github:o/r/main?host=github.example.org",
        "gitlab:o/r?ref=foo%2Fbar&dir=a%2Fb",
        "sourcehut:~owner/r?ref=main",
        "git+https://example.org/r.git?ref=refs%2Ftags%2Fv1.0.0&shallow=0&submodules=1",
        "git+ssh://git@example.org:2222/r.git?ref=main&rev=2111c84ac0b4555518b9482ec436f813d26905ef&allRefs=1",
        "git+file:///tmp/flake?ref=main",
        "git://example.org/r?ref=main",
        "git+https://example.org/r.git?ref=topic%2Bname&token=a%2Bb%26c%3Dd",
    ];

    #[test]
    fn decodes_components_using_nix_query_rules() {
        let url =
            FlakeUrl::parse("github:bit%65myapp/sekka?%72ef=feature%2Fx+y&ref=ignored&bare&dir=a%26b%3Dc").unwrap();
        assert!(url.is_official("bitemyapp", "sekka"));
        assert_eq!(url.reference.as_deref(), Some("feature/x+y"));
        assert_eq!(url.options, BTreeMap::from([("dir".into(), "a&b=c".into())]));
        // An escaped separator cannot smuggle another owner/repository in.
        assert!(!FlakeUrl::parse("github:bitemyapp%2Fsekka/other").unwrap().is_official("bitemyapp", "sekka"));
        assert!(
            !FlakeUrl::parse("github:bitemyapp/sekka?host=elsewhere.example")
                .unwrap()
                .is_official("bitemyapp", "sekka")
        );
    }

    #[test]
    fn distinguishes_revisions_from_refs() {
        let url = FlakeUrl::parse(&format!("github:o/r/{SHA}")).unwrap();
        assert_eq!(url.revision.as_deref(), Some(SHA));
        assert_eq!(url.reference, None);
        let url = FlakeUrl::parse(&format!("github:o/r?ref={SHA}")).unwrap();
        assert_eq!(url.reference.as_deref(), Some(SHA));
        assert_eq!(url.revision, None);
        let changed = url.with_branch(SHA).unwrap();
        assert_eq!(FlakeUrl::parse(&changed).unwrap().reference.as_deref(), Some(SHA));
    }

    #[test]
    fn rejects_unsupported_or_ambiguous_inputs() {
        for url in [
            "path:/tmp/flake",
            "https://example.org/flake.tar.gz",
            "github:o",
            "github:o/r?ref=",
            "github:o/r/main?ref=nightly",
            "github:o/r?rev=0123",
            "github:o/r?ref=main&rev=2111c84ac0b4555518b9482ec436f813d26905ef",
            "github:o/r?ref=%FF",
            "github:o/r?ref=bad%2",
            "github:o/r#output",
        ] {
            assert!(FlakeUrl::parse(url).is_none(), "{url}");
        }
    }

    #[test]
    fn reconstructed_urls_preserve_components() {
        for url in URLS {
            let parsed = FlakeUrl::parse(url).unwrap();
            let rendered = parsed.render().unwrap();
            let roundtrip = FlakeUrl::parse(&rendered).unwrap();
            assert_eq!(roundtrip.source, parsed.source, "{url} -> {rendered}");
            assert_eq!(roundtrip.reference, parsed.reference, "{url} -> {rendered}");
            assert_eq!(roundtrip.revision, parsed.revision, "{url} -> {rendered}");
            assert_eq!(roundtrip.options, parsed.options, "{url} -> {rendered}");
            assert_eq!(roundtrip.render().as_deref(), Some(rendered.as_str()));
        }
    }

    #[test]
    #[ignore = "requires Nix with flakes enabled; entirely offline"]
    fn reconstruction_agrees_with_nix() {
        // The independent oracle is Nix's fetcher parser, without fetching
        // anything. Include branch rewrites to check pin removal and encoding.
        let mut pairs = Vec::new();
        for url in URLS {
            let parsed = FlakeUrl::parse(url).unwrap();
            pairs.push((url.to_string(), parsed.render().unwrap()));
            let changed = parsed.with_branch("feature/new").unwrap();
            pairs.push((changed.clone(), FlakeUrl::parse(&changed).unwrap().render().unwrap()));
        }
        // Lock metadata reaches the same builder without reconstructing a
        // URL by concatenating its transport, query and reference strings.
        for (before, source) in [
            (
                "git+https://example.org/r?token=a%2Bb%26c&ref=feature%2Fx%2By",
                crate::lock::Locked {
                    kind: "git".into(),
                    url: Some("https://example.org/r?token=a%2Bb%26c".into()),
                    reference: Some("feature/x+y".into()),
                    ..Default::default()
                },
            ),
            (
                "git://example.org/r?ref=main",
                crate::lock::Locked {
                    kind: "git".into(),
                    url: Some("git://example.org/r".into()),
                    reference: Some("main".into()),
                    ..Default::default()
                },
            ),
            (
                "github:o/r?ref=2111c84ac0b4555518b9482ec436f813d26905ef&host=git.example.org",
                crate::lock::Locked {
                    kind: "github".into(),
                    owner: Some("o".into()),
                    repo: Some("r".into()),
                    reference: Some(SHA.into()),
                    host: Some("git.example.org".into()),
                    ..Default::default()
                },
            ),
        ] {
            pairs.push((before.into(), source.describe()));
        }
        let strings = pairs
            .iter()
            .flat_map(|(before, after)| [before, after])
            .map(|s| crate::edit::nix_string(s))
            .collect::<Vec<_>>()
            .join(" ");
        let output = std::process::Command::new("nix")
            .args([
                "eval",
                "--json",
                "--extra-experimental-features",
                "nix-command flakes",
                "--expr",
                &format!("map builtins.parseFlakeRef [ {strings} ]"),
            ])
            .output()
            .expect("run Nix");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(values.len(), pairs.len() * 2);
        for ((before, after), values) in pairs.iter().zip(values.as_chunks::<2>().0) {
            assert_eq!(values[0], values[1], "{before} -> {after}");
            let parsed = FlakeUrl::parse(before).unwrap();
            assert_eq!(values[0]["ref"].as_str(), parsed.reference.as_deref(), "{before}");
            assert_eq!(values[0]["rev"].as_str(), parsed.revision.as_deref(), "{before}");
            if let Source::Forge { owner, repo } = parsed.source {
                assert_eq!(values[0]["owner"], owner, "{before}");
                assert_eq!(values[0]["repo"], repo, "{before}");
            }
        }
    }
}
