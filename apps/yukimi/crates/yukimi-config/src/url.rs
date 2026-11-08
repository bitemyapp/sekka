// SPDX-License-Identifier: MIT OR Apache-2.0
//! URL components shared by flake references, lock metadata and API requests.
use std::collections::BTreeMap;

use fluent_uri::{
    Uri,
    build::Builder,
    component::Scheme,
    pct_enc::{
        EStr, EString,
        encoder::{Data, Path, Query},
    },
};

pub(crate) fn path(segments: &[&str]) -> EString<Path> {
    let mut out = EString::new();
    for (i, segment) in segments.iter().enumerate() {
        if i != 0 {
            out.push('/');
        }
        out.encode_str::<Data>(segment);
    }
    out
}

pub(crate) fn query(params: &BTreeMap<String, String>) -> EString<Query> {
    let mut out = EString::new();
    for (i, (key, value)) in params.iter().enumerate() {
        if i != 0 {
            out.push('&');
        }
        out.encode_str::<Data>(key);
        out.push('=');
        out.encode_str::<Data>(value);
    }
    out
}

pub(crate) fn segments(uri: &Uri<&str>) -> Option<Vec<String>> {
    uri.path().split('/').map(|p| p.decode().to_string().ok().map(|s| s.into_owned())).collect()
}

/// Decoded path components of an ordinary HTTPS origin, excluding the
/// leading slash. User info, nonstandard ports and fragments aren't origins
/// Yukimi's repository and release APIs accept.
pub fn https_parts(value: &str) -> Option<(String, Vec<String>)> {
    let uri = Uri::parse(value).ok()?;
    let authority = uri.authority()?;
    if uri.scheme().as_str() != "https"
        || uri.fragment().is_some()
        || authority.userinfo().is_some()
        || !matches!(authority.port_to_u16(), Ok(None | Some(443)))
    {
        return None;
    }
    let mut parts = segments(&uri)?;
    if parts.first()?.is_empty() {
        parts.remove(0);
    } else {
        return None;
    }
    Some((authority.host().to_ascii_lowercase(), parts))
}

/// The `(owner, repo)` of a repository on github.com, from the HTTPS address
/// Git clones it from: `https://github.com/owner/repo`, with or without
/// `.git` or a final slash.
pub(crate) fn github_repository(value: &str) -> Option<(String, String)> {
    let (host, mut parts) = https_parts(value)?;
    if parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    let [owner, repo] = parts.as_slice() else { return None };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let name = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    (host == "github.com" && name(owner) && name(repo)).then(|| (owner.clone(), repo.to_owned()))
}

/// Build an HTTPS URL from decoded components. Separators inside a component
/// stay data (including slashes inside branch names), and '+' stays literal.
pub fn https(host: &str, segments: &[&str], params: &[(&str, &str)]) -> Option<String> {
    if host.is_empty() {
        return None;
    }
    let host = EStr::new(host)?;
    let mut parts = vec![""];
    parts.extend_from_slice(segments);
    let path = path(&parts);
    let query = query(&params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
    Uri::builder()
        .scheme(Scheme::new_or_panic("https"))
        .authority_with(|b| b.host(host))
        .path(&path)
        .optional(Builder::query, (!query.is_empty()).then_some(query.as_ref()))
        .build()
        .ok()
        .map(|u| u.into_string())
}

/// Host for diagnostics, parsed without treating URL paths as authorities.
pub fn host(value: &str) -> Option<String> {
    Some(Uri::parse(value).ok()?.authority()?.host().to_owned())
}
