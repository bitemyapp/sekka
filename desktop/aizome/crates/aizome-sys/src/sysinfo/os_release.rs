//! The operating system's identity, from `/etc/os-release` (or
//! `/usr/lib/os-release`), per os-release(5).
//!
//! The file is a list of `KEY=value` shell assignments: values may be bare
//! or in double or single quotes; in double quotes a backslash escapes
//! `"`, `\`, `` ` `` and `$`; outside quotes a backslash escapes any
//! character; `#` starts a comment line. The same format serves
//! `/etc/machine-info`, so the parser is shared ([`parse_os_release`] keeps
//! every field in [`OsRelease::fields`]).

use super::read_text;

const OS_RELEASE: &str = "/etc/os-release";
const OS_RELEASE_FALLBACK: &str = "/usr/lib/os-release";

/// The operating system, as `/etc/os-release` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsRelease {
    /// `NAME`, such as "NixOS"; "Linux" if unset.
    pub name: String,
    /// `ID`, such as `nixos`; `linux` if unset.
    pub id: String,
    /// `ID_LIKE`: the IDs of related systems, closest first.
    pub id_like: Vec<String>,
    /// `PRETTY_NAME`, such as "NixOS 26.11 (Zokor)"; "Linux" if unset.
    pub pretty_name: String,
    /// `VERSION`, such as "26.11 (Zokor)".
    pub version: Option<String>,
    /// `VERSION_ID`, such as "26.11".
    pub version_id: Option<String>,
    /// `VERSION_CODENAME`, such as "zokor".
    pub version_codename: Option<String>,
    /// `BUILD_ID`: on NixOS, the release and nixpkgs revision, such as
    /// "26.11.20261011.7fd4004".
    pub build_id: Option<String>,
    /// `VARIANT`, such as "Workstation Edition".
    pub variant: Option<String>,
    /// `VARIANT_ID`, such as `workstation`.
    pub variant_id: Option<String>,
    /// `HOME_URL`.
    pub home_url: Option<String>,
    /// `LOGO`: an icon name, such as `nix-snowflake`.
    pub logo: Option<String>,
    /// Every assignment in the file, in order, unquoted.
    pub fields: Vec<(String, String)>,
}

impl Default for OsRelease {
    fn default() -> OsRelease {
        parse_os_release("")
    }
}

impl OsRelease {
    /// The value of any key, such as `ANSI_COLOR` or `SUPPORT_URL`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Read `/etc/os-release`, else `/usr/lib/os-release`; if neither can be
/// read, the defaults ("Linux").
pub fn os_release() -> OsRelease {
    match read_text(OS_RELEASE).or_else(|_| read_text(OS_RELEASE_FALLBACK)) {
        Ok(text) => parse_os_release(&text),
        Err(error) => {
            log::debug!("os-release: {error}");
            OsRelease::default()
        }
    }
}

/// Parse os-release text. Malformed lines (no `=`, a bad key, an
/// unterminated quote) are skipped; a key given twice keeps its last value.
pub fn parse_os_release(text: &str) -> OsRelease {
    let fields = parse_assignments(text);
    let get = |key: &str| fields.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    OsRelease {
        name: get("NAME").unwrap_or_else(|| "Linux".into()),
        id: get("ID").unwrap_or_else(|| "linux".into()),
        id_like: get("ID_LIKE").map(|v| v.split_ascii_whitespace().map(str::to_owned).collect()).unwrap_or_default(),
        pretty_name: get("PRETTY_NAME").unwrap_or_else(|| "Linux".into()),
        version: get("VERSION"),
        version_id: get("VERSION_ID"),
        version_codename: get("VERSION_CODENAME"),
        build_id: get("BUILD_ID"),
        variant: get("VARIANT"),
        variant_id: get("VARIANT_ID"),
        home_url: get("HOME_URL"),
        logo: get("LOGO"),
        fields,
    }
}

/// The `KEY=value` assignments of an os-release-style file, unquoted.
pub(crate) fn parse_assignments(text: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        if !is_key(key) {
            continue;
        }
        if let Some(value) = unquote(raw) {
            fields.push((key.to_owned(), value));
        }
    }
    fields
}

/// A shell variable name: a letter or `_`, then letters, digits or `_`.
fn is_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Undo shell quoting of one value. Unquoted whitespace ends it (anything
/// after is ignored); an unterminated quote makes it malformed (`None`).
fn unquote(raw: &str) -> Option<String> {
    let mut value = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => match chars.next()? {
                        escaped @ ('"' | '\\' | '`' | '$') => value.push(escaped),
                        other => {
                            value.push('\\');
                            value.push(other);
                        }
                    },
                    other => value.push(other),
                }
            },
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    other => value.push(other),
                }
            },
            '\\' => {
                if let Some(escaped) = chars.next() {
                    value.push(escaped);
                }
            }
            c if c.is_whitespace() => break,
            c => value.push(c),
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // This machine's /etc/os-release.
    const NIXOS: &str = r#"ANSI_COLOR="0;38;2;126;186;228"
BUG_REPORT_URL="https://github.com/NixOS/nixpkgs/issues"
BUILD_ID="26.11.20261011.7fd4004"
CPE_NAME="cpe:/o:nixos:nixos:26.11"
DEFAULT_HOSTNAME=nixos
DOCUMENTATION_URL="https://nixos.org/learn.html"
HOME_URL="https://nixos.org/"
ID=nixos
LOGO="nix-snowflake"
NAME=NixOS
PRETTY_NAME="NixOS 26.11 (Zokor)"
SUPPORT_URL="https://nixos.org/community.html"
VENDOR_NAME=NixOS
VENDOR_URL="https://nixos.org/"
VERSION="26.11 (Zokor)"
VERSION_CODENAME=zokor
VERSION_ID="26.11"
"#;

    #[test]
    fn nixos_os_release_parses() {
        let os = parse_os_release(NIXOS);
        assert_eq!(os.name, "NixOS");
        assert_eq!(os.id, "nixos");
        assert_eq!(os.pretty_name, "NixOS 26.11 (Zokor)");
        assert_eq!(os.version.as_deref(), Some("26.11 (Zokor)"));
        assert_eq!(os.version_id.as_deref(), Some("26.11"));
        assert_eq!(os.version_codename.as_deref(), Some("zokor"));
        assert_eq!(os.build_id.as_deref(), Some("26.11.20261011.7fd4004"));
        assert_eq!(os.logo.as_deref(), Some("nix-snowflake"));
        assert_eq!(os.variant, None);
        assert_eq!(os.get("ANSI_COLOR"), Some("0;38;2;126;186;228"));
        assert_eq!(os.get("DEFAULT_HOSTNAME"), Some("nixos"));
        assert_eq!(os.fields.len(), 17);
    }

    #[test]
    fn quotes_and_escapes_follow_the_shell() {
        let os = parse_os_release(
            r#"
# A comment
NAME='Fedora Linux'
VARIANT="Workstation \"Edition\" \$HOME \\ \n"
VARIANT_ID=work\ station
ID_LIKE="rhel centos  fedora"
PRETTY_NAME="Fedora"' Linux 41'
BUILD_ID=abc # trailing comment
"#,
        );
        assert_eq!(os.name, "Fedora Linux");
        assert_eq!(os.variant.as_deref(), Some(r#"Workstation "Edition" $HOME \ \n"#));
        assert_eq!(os.variant_id.as_deref(), Some("work station"));
        assert_eq!(os.id_like, ["rhel", "centos", "fedora"]);
        assert_eq!(os.pretty_name, "Fedora Linux 41");
        assert_eq!(os.build_id.as_deref(), Some("abc"));
    }

    #[test]
    fn malformed_lines_are_skipped_and_defaults_apply() {
        let os = parse_os_release(
            "NAME=\"unterminated\nnot an assignment\n1BAD=x\nBAD KEY=x\n=x\nID=\nVERSION=1\nVERSION=2\n",
        );
        assert_eq!(os.name, "Linux");
        assert_eq!(os.id, "linux");
        assert_eq!(os.pretty_name, "Linux");
        assert_eq!(os.version.as_deref(), Some("2"));
        assert_eq!(OsRelease::default().name, "Linux");
        assert!(parse_assignments("").is_empty());
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_os_release_is_read() {
        let os = os_release();
        println!("{os:#?}");
        assert_ne!(os.pretty_name, "");
    }
}
