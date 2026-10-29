//! Nisshi 日誌 (Logs): the system journal and the log files under
//! /var/log, filtered as you type, with filters suggested by what wrote
//! each message. What only root may read is read by a helper that polkit
//! starts once an administrator's password is given.
//!
//! - [`entry`]: a message, read from `journalctl -o json` or a file's line;
//! - [`query`], [`protocol`]: what the window asks its reader, the
//!   `aizome-nisshi-helper` program, and how it answers;
//! - [`backend`]: the window's side of the reader;
//! - [`search`], [`filter`], [`model`]: what's shown;
//! - [`shape`], [`known`], [`suggest`]: what Nisshi suggests;
//! - [`app`] and `ui`: the window.

pub mod app;
pub mod backend;
pub mod entry;
pub mod filter;
pub mod fixture;
pub mod instance;
pub mod known;
pub mod model;
pub mod protocol;
pub mod query;
pub mod search;
pub mod shape;
pub mod suggest;
pub mod time;
pub mod ui;

pub use app::{APP_ID, Msg, Nisshi};

/// A number with thousands separated: 12,345.
pub fn number(n: usize) -> String {
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

/// "1 message", "12,345 messages".
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", number(n), if n == 1 { one } else { many })
}

/// A size in bytes, in words.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} bytes");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if v < 10.0 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numbers() {
        assert_eq!(super::number(0), "0");
        assert_eq!(super::number(999), "999");
        assert_eq!(super::number(1000), "1,000");
        assert_eq!(super::number(1234567), "1,234,567");
        assert_eq!(super::count(1, "message", "messages"), "1 message");
        assert_eq!(super::bytes(39_722), "40 KB");
        assert_eq!(super::bytes(1_500), "1.5 KB");
    }
}
