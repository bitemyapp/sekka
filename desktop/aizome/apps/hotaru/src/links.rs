//! Gesture identity and validation for links in an immutable displayed frame.
use crate::terminal_view::CellPoint;
use hotaru_core::Snapshot;
use hotaru_core::hyperlink::LinkIdentity;
use std::sync::Arc;

#[derive(Clone)]
enum Origin {
    Explicit(LinkIdentity),
    Automatic { request: Arc<crate::automatic_links::Request>, start: CellPoint, end: CellPoint },
}
#[derive(Clone)]
pub struct Link {
    pub snapshot: Arc<Snapshot>,
    pub cell: CellPoint,
    pub target: Arc<str>,
    origin: Origin,
}
impl Link {
    pub fn at(snapshot: &Arc<Snapshot>, cell: CellPoint) -> Option<Self> {
        let link = snapshot.lines.get(cell.row)?.hyperlink(cell.col)?;
        let target = valid_uri(&link.uri)?;
        Some(Self {
            snapshot: snapshot.clone(),
            cell,
            origin: Origin::Explicit(link.id.clone()),
            target: Arc::from(target),
        })
    }
    pub fn automatic(request: Arc<crate::automatic_links::Request>, matched: hotaru_link_regex::Match) -> Option<Self> {
        if !matched.contains(request.cell)
            || request
                .snapshot
                .lines
                .get(usize::try_from(request.cell.row).ok()?)?
                .hyperlink(usize::from(request.cell.col))
                .is_some()
        {
            return None;
        }
        let target = valid_target(matched.text().as_bytes())?;
        let start =
            CellPoint { row: usize::try_from(matched.start().row).ok()?, col: usize::from(matched.start().col) };
        let end = CellPoint { row: usize::try_from(matched.end().row).ok()?, col: usize::from(matched.end().col) };
        Some(Self {
            snapshot: request.snapshot.clone(),
            cell: CellPoint { row: usize::try_from(request.cell.row).ok()?, col: usize::from(request.cell.col) },
            target: Arc::from(target),
            origin: Origin::Automatic { request, start, end },
        })
    }
    pub fn request(&self) -> Option<&Arc<crate::automatic_links::Request>> {
        match &self.origin {
            Origin::Automatic { request, .. } => Some(request),
            Origin::Explicit(_) => None,
        }
    }
    pub fn is_automatic(&self) -> bool {
        self.request().is_some()
    }
    pub fn explicit_matches(&self, link: &hotaru_core::hyperlink::Hyperlink) -> bool {
        matches!(&self.origin,Origin::Explicit(identity) if *identity==link.id && self.target.as_bytes()==link.uri.as_ref())
    }
    pub fn automatic_span(&self, row: usize, cols: usize) -> Option<std::ops::Range<usize>> {
        let Origin::Automatic { start, end, .. } = &self.origin else {
            return None;
        };
        if row < start.row || row > end.row {
            return None;
        }
        let from = if row == start.row { start.col } else { 0 };
        let to = if row == end.row { end.col + 1 } else { cols };
        Some(from.min(cols)..to.min(cols))
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.snapshot, &other.snapshot)
            && self.cell == other.cell
            && self.target == other.target
            && match (&self.origin, &other.origin) {
                (Origin::Explicit(a), Origin::Explicit(b)) => a == b,
                (Origin::Automatic { request: a, .. }, Origin::Automatic { request: b, .. }) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
    pub fn matches_snapshot(&self, snapshot: &Arc<Snapshot>) -> bool {
        Arc::ptr_eq(&self.snapshot, snapshot)
            && match &self.origin {
                Origin::Explicit(_) => snapshot
                    .lines
                    .get(self.cell.row)
                    .and_then(|row| row.hyperlink(self.cell.col))
                    .is_some_and(|link| self.explicit_matches(link)),
                Origin::Automatic { request, .. } => Arc::ptr_eq(&request.snapshot, snapshot),
            }
    }
}
/// A literal desktop-handler argument. Never treated as shell input.
pub fn valid_target(bytes: &[u8]) -> Option<&str> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 || bytes[0] == b'-' {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// OSC 8 targets are URIs, not arbitrary command arguments or local relative paths.
/// Preserve valid Unicode and percent encoding verbatim for the desktop handler.
pub fn valid_uri(bytes: &[u8]) -> Option<&str> {
    let text = valid_target(bytes)?;
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
    fn explicit_link_presence_suppresses_automatic_fallback_even_when_uri_is_invalid() {
        let mut terminal = hotaru_core::Terminal::new(40, 2, 0);
        terminal.feed(b"\x1b]8;;invalid\x1b\\https://example.test\x1b]8;;\x1b\\");
        let snapshot = Arc::new(terminal.snapshot());
        let cell = hotaru_link_regex::CellPoint { row: 0, col: 2 };
        let mapped = crate::automatic_links::mapping::extract(
            &snapshot,
            cell,
            &std::sync::atomic::AtomicBool::new(false),
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .unwrap();
        let found = hotaru_link_regex::Matcher::new()
            .unwrap()
            .find_at(
                &mapped.source,
                cell,
                &hotaru_link_regex::Limits::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
        assert!(Link::at(&snapshot, CellPoint { row: 0, col: 2 }).is_none());
        let request = Arc::new(crate::automatic_links::Request { pane: 1, snapshot, cell, retained_bytes: 1024 });
        assert!(Link::automatic(request, found).is_none());
    }
    #[test]
    fn changed_frames_movement_or_cell_cancel_the_owned_gesture() {
        let mut term = hotaru_core::Terminal::new(12, 2, 0);
        term.feed("\x1b]8;id=one;https://example.test\x1b\\界AB\x1b]8;;\x1b\\".as_bytes());
        let first = Arc::new(term.snapshot());
        let head = Link::at(&first, CellPoint { row: 0, col: 0 }).unwrap();
        let tail = Link::at(&first, CellPoint { row: 0, col: 1 }).unwrap();
        assert!(head.explicit_matches(first.lines[0].hyperlink(1).unwrap()));
        assert_eq!(head.target, tail.target);
        let press = || Press { link: tail.clone(), position: (15., 10.), cancelled: false };
        assert!(press().release(Some(&tail), (15., 10.), true).is_some());
        assert!(press().release(None, (15., 10.), true).is_none());
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
