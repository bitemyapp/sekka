//! Sizes and counts for people: decimal units, as the Finder and DaisyDisk
//! show them ("12.3 GB"), with digits grouped in counts ("142,301").

/// `0 bytes`, `912 bytes`, `4.1 kB`, `12.3 GB`, `1.25 TB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["kB", "MB", "GB", "TB", "PB", "EB"];
    if bytes < 1000 {
        return if bytes == 1 { "1 byte".into() } else { format!("{bytes} bytes") };
    }
    let mut v = bytes as f64 / 1000.0;
    let mut u = 0;
    while v >= 999.95 && u + 1 < UNITS.len() {
        v /= 1000.0;
        u += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.2} {}", UNITS[u]).replace(".00 ", " ")
    }
}

/// `142,301`.
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `1 file`, `3 files`.
pub fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", count(n), if n == 1 { one } else { many })
}

/// A share as a percentage: `42%`, `0.4%`, `<0.1%`.
pub fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".into();
    }
    let p = part as f64 * 100.0 / whole as f64;
    if p == 0.0 {
        "0%".into()
    } else if p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// A path for people: the home folder as `~`.
pub fn path(p: &std::path::Path) -> String {
    if let Some(home) = crate::home_dir()
        && let Ok(rest) = p.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
    }
    p.display().to_string()
}

/// A path quoted for a POSIX shell.
pub fn shell_quote(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-+,:@%=".contains(&b)) {
        return s.into_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(999), "999 bytes");
        assert_eq!(size(1000), "1 kB");
        assert_eq!(size(4_096), "4.10 kB");
        assert_eq!(size(12_345_678_901), "12.3 GB");
        assert_eq!(size(999_960_000), "1 GB");
        assert_eq!(size(250_000_000_000), "250 GB");
        assert_eq!(count(142_301), "142,301");
        assert_eq!(count(7), "7");
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(shell_quote(std::path::Path::new("/a b/it's")), r"'/a b/it'\''s'");
        assert_eq!(shell_quote(std::path::Path::new("/a/b.txt")), "/a/b.txt");
    }
}
