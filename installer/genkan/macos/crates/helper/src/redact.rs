// SPDX-License-Identifier: GPL-3.0-or-later
//! What leaves the helper in a support report: no account names, no home
//! folders, nothing that looks like a secret.

/// Replace each of `names` (account names known to this installation) and
/// home-folder paths with placeholders.
pub fn redact(line: &str, names: &[&str]) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("/Users/") {
        out.push_str(&rest[..i]);
        out.push_str("/Users/…");
        let after = &rest[i + "/Users/".len()..];
        let end = after.find(['/', ' ', '"', '\'']).unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    for name in names.iter().filter(|n| n.len() >= 2) {
        out = replace_word(&out, name, "(account)");
    }
    out
}

fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(word) {
        let before = rest[..i].chars().last();
        let after = rest[i + word.len()..].chars().next();
        let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() && c != '_');
        out.push_str(&rest[..i]);
        if boundary(before) && boundary(after) {
            out.push_str(with);
        } else {
            out.push_str(word);
        }
        rest = &rest[i + word.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homes_and_names_are_removed() {
        assert_eq!(
            redact("copied /Users/callen/Library/x for callen (callenx)", &["callen"]),
            "copied /Users/…/Library/x for (account) (callenx)"
        );
    }
}
