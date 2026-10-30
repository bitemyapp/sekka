//! A message's shape: the message with what changes between its repeats
//! taken out (numbers, addresses, ids, hashes, quoted names), so that
//! "Connection 4123 closed" and "Connection 88 closed" are one kind of
//! message, which can be counted, followed or hidden together.

use std::hash::{Hash, Hasher};

/// Where a shape stops: messages alike this far are alike.
const LONGEST: usize = 160;

/// The message's shape, as text: `#` for each number, id or address, `…`
/// for what was quoted.
pub fn shape(message: &str) -> String {
    let mut out = String::with_capacity(message.len().min(LONGEST) + 8);
    let first = message.lines().next().unwrap_or("");
    let mut chars = first.char_indices().peekable();
    let mut space = false;
    while let Some((at, c)) = chars.next() {
        if out.len() >= LONGEST {
            break;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if c == '"' || c == '\'' || c == '“' || c == '‘' {
            // A quoted name: kept as quotes around an ellipsis, if it closes
            // on this line soon enough.
            let close = match c {
                '“' => '”',
                '‘' => '’',
                q => q,
            };
            let rest = &first[at + c.len_utf8()..];
            if let Some(end) = rest.find(close).filter(|&e| e <= 120 && (c != '\'' || e > 0)) {
                // An apostrophe ("can't") isn't a quote: a quote follows a
                // space or punctuation.
                let opens = first[..at].chars().next_back().is_none_or(|p| !p.is_alphanumeric());
                if opens {
                    out.push(c);
                    out.push('…');
                    out.push(close);
                    let stop = at + c.len_utf8() + end + close.len_utf8();
                    while chars.peek().is_some_and(|(i, _)| *i < stop) {
                        chars.next();
                    }
                    continue;
                }
            }
            out.push(c);
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            // A word: up to the next character that can't be in one.
            let mut end = at + c.len_utf8();
            while let Some(&(i, d)) = chars.peek() {
                if d.is_alphanumeric() || d == '_' {
                    end = i + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            push_word(&mut out, &first[at..end]);
            if out.len() > LONGEST {
                let mut cut = LONGEST;
                while !out.is_char_boundary(cut) {
                    cut -= 1;
                }
                out.truncate(cut);
            }
            continue;
        }
        out.push(c);
    }
    // Runs of placeholders joined by punctuation are one: an address
    // (#.#.#.#, #:#:#), a version, a UUID, a time.
    collapse(&out)
}

/// A word as it stands in a shape.
fn push_word(out: &mut String, word: &str) {
    let digits = word.bytes().filter(u8::is_ascii_digit).count();
    if digits == 0 {
        out.push_str(word);
        return;
    }
    let hex = word.bytes().all(|b| b.is_ascii_hexdigit());
    let prefixed = word.len() > 2 && (word.starts_with("0x") || word.starts_with("0X"));
    // All digits; hexadecimal long enough to be an id or address; a long
    // run of letters and digits (a store path's hash, a token).
    if digits == word.len() || prefixed || (hex && word.len() >= 4) || (word.len() >= 16 && digits >= 3) {
        out.push('#');
        return;
    }
    // A name with a number in it (sda1, wlp14s0, eth0): the numbers go.
    let mut in_digits = false;
    for c in word.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(c);
            in_digits = false;
        }
    }
}

fn collapse(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            // #, then any run of (one joining character, #).
            let mut j = i + 1;
            while j + 1 < b.len() && matches!(b[j], b'.' | b':' | b'-' | b'/' | b',') && b[j + 1] == b'#' {
                j += 2;
            }
            out.push('#');
            i = j;
        } else {
            let c = s[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The shape as a number, for counting and matching without its text.
pub fn fingerprint(message: &str) -> u64 {
    hash(&shape(message))
}

pub fn hash(shape: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    shape.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_changes_goes() {
        assert_eq!(shape("Connection 4123 closed"), "Connection # closed");
        assert_eq!(shape("Connection 88 closed"), shape("Connection 4123 closed"));
        assert_eq!(
            shape("Trying to enable 0x5c33391b8350 but 0x5c33391b8350 is already enabled"),
            "Trying to enable # but # is already enabled"
        );
        assert_eq!(shape("DHCPACK of 192.168.1.23 from 192.168.1.1"), "DHCPACK of # from #");
        assert_eq!(shape("wlp14s0: link becomes ready"), "wlp#s#: link becomes ready");
        assert_eq!(
            shape("Started session-c3.scope 5b2e3c1e-9f1a-4c7e-8d2b-0e1f2a3b4c5d for user"),
            "Started session-c#.scope # for user"
        );
        assert_eq!(shape("Failed to open '/home/alice/x.txt': No such file"), "Failed to open '…': No such file");
        assert_eq!(shape("Can't open display"), "Can't open display");
        assert_eq!(shape("  spaced\t\tout  "), "spaced out");
        assert_eq!(shape("took 12.5ms (p95 31.2ms)"), "took #ms (p# #ms)");
    }

    #[test]
    fn only_the_first_line_and_the_start_count() {
        assert_eq!(shape("Process 12 dumped core.\n\nStack trace of thread 12:\n#0 0x1"), "Process # dumped core.");
        let long = "x".repeat(400);
        assert!(shape(&long).len() <= LONGEST + 4);
        assert_eq!(fingerprint(&format!("{long}a")), fingerprint(&format!("{long}b")));
    }
}
