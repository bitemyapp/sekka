use hotaru_core::{
    Terminal,
    search::{self, Options},
};
use std::sync::atomic::AtomicBool;
#[test]
fn leading_blank_prune() {
    for count in 2..8 {
        for mut mask in 0..3usize.pow(count) {
            let mut initial = Vec::new();
            for _ in 0..count {
                initial.extend_from_slice(match mask % 3 {
                    0 => b"\r\n",
                    1 => b" \r\n",
                    _ => b"a\r\n",
                });
                mask /= 3;
            }
            initial.extend_from_slice(b"X");
            let mut t = Terminal::new(4, 1, count as usize);
            t.feed(&initial);
            let old = search::find(
                t.content_snapshot(),
                b"\n\n",
                Options::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
            t.feed(b"b\r\n");
            let now = search::find(
                t.content_snapshot(),
                b"\n\n",
                Options::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
            for i in 0..old.matches.len() {
                if let Some(m) = old.resolve_match(i, &t.content_position(0), t.generation()) {
                    assert!(
                        now.matches.contains(&m),
                        "initial={:?} old={:?} stale={m:?} fresh={:?}",
                        String::from_utf8_lossy(&initial),
                        old.matches,
                        now.matches
                    );
                }
            }
        }
    }
}
