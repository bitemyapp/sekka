//! Gesture identity and validation for links in an immutable displayed frame.
use crate::terminal_view::CellPoint;
use hotaru_core::Snapshot;
use hotaru_core::hyperlink::LinkIdentity;
use std::sync::Arc;

#[derive(Clone)]
pub struct Link {
    pub snapshot: Arc<Snapshot>,
    pub cell: CellPoint,
    pub identity: LinkIdentity,
    pub target: Arc<str>,
}

impl Link {
    pub fn at(snapshot: &Arc<Snapshot>, cell: CellPoint) -> Option<Self> {
        let link = snapshot.lines.get(cell.row)?.hyperlink(cell.col)?;
        let target = valid_uri(&link.uri)?;
        Some(Self { snapshot: snapshot.clone(), cell, identity: link.id.clone(), target: Arc::from(target) })
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.snapshot, &other.snapshot)
            && self.cell == other.cell
            && self.identity == other.identity
            && self.target == other.target
    }
    pub fn matches_snapshot(&self, snapshot: &Arc<Snapshot>) -> bool {
        Arc::ptr_eq(&self.snapshot, snapshot)
            && snapshot
                .lines
                .get(self.cell.row)
                .and_then(|row| row.hyperlink(self.cell.col))
                .is_some_and(|link| link.id == self.identity && link.uri.as_ref() == self.target.as_bytes())
    }
}

/// OSC 8 targets are URIs, not arbitrary command arguments or local relative paths.
/// Preserve valid Unicode and percent encoding verbatim for the desktop handler.
pub fn valid_uri(bytes: &[u8]) -> Option<&str> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = text.split_once(':')?;
    if rest.is_empty()
        || !scheme.as_bytes().first()?.is_ascii_alphabetic()
        || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(text)
}

pub struct Press {
    pub link: Link,
    pub position: (f64, f64),
    pub cancelled: bool,
}
impl Press {
    pub fn moved(&mut self, position: (f64, f64)) {
        self.cancelled |= self.position != position;
    }
    pub fn release(self, hit: Option<&Link>, position: (f64, f64), modifier: bool) -> Option<Link> {
        (!self.cancelled && modifier && self.position == position && hit.is_some_and(|hit| self.link.same(hit)))
            .then_some(self.link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_are_validated_without_rewriting_or_shell_interpretation() {
        for invalid in [
            &b"https://x/\xff"[..],
            b"https://x/\0",
            b"https://x/\x1b",
            "https://x/\u{009c}".as_bytes(),
            b"-help",
            b"relative/path",
            b"1bad:target",
            b"http:",
        ] {
            assert!(valid_uri(invalid).is_none());
        }
        let target = "https://example.test/日本?x=$(false)&y='quotes'";
        assert_eq!(valid_uri(target.as_bytes()), Some(target));
    }
    #[test]
    fn changed_frames_movement_or_cell_cancel_the_owned_gesture() {
        let mut term = hotaru_core::Terminal::new(12, 2, 0);
        term.feed("\x1b]8;id=one;https://example.test\x1b\\界AB\x1b]8;;\x1b\\".as_bytes());
        let first = Arc::new(term.snapshot());
        let head = Link::at(&first, CellPoint { row: 0, col: 0 }).unwrap();
        let tail = Link::at(&first, CellPoint { row: 0, col: 1 }).unwrap();
        assert_eq!(head.identity, tail.identity);
        assert_eq!(head.target, tail.target);
        let press = || Press { link: tail.clone(), position: (15., 10.), cancelled: false };
        assert!(press().release(Some(&tail), (15., 10.), true).is_some());
        assert!(press().release(Some(&head), (15., 10.), true).is_none());
        assert!(press().release(Some(&tail), (15., 10.), false).is_none());
        let mut moved = press();
        moved.moved((15.1, 10.));
        moved.moved((15., 10.));
        assert!(moved.release(Some(&tail), (15., 10.), true).is_none());
        let newer = Arc::new(term.snapshot());
        let newer = Link::at(&newer, tail.cell).unwrap();
        assert!(press().release(Some(&newer), (15., 10.), true).is_none());
    }
}
