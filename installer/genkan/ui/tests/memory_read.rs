// SPDX-License-Identifier: GPL-3.0-or-later
//! GEN-28: when reading /proc/meminfo fails, the failure is kept as such.
//! It used to leave memory unknown, the same value as "still reading", so
//! turning swap back on showed "Reading installed memory…" on the Disk page
//! and Next answered "Still reading installed memory; try again in a
//! moment." for ever: nothing reads it again.
//!
//! Headless (`aizome_kit::testing::Harness`) on the preview's sample data,
//! so no disk, network or helper is touched. The `memfail` preview answers
//! the window's memory read with a failure, through the real handler.
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use genkan::Step;
use genkan::app::{Genkan, Msg};
use genkan::ram::SwapSize;
use std::time::Duration;

fn says(h: &Harness<Genkan>, w: aizome_kit::WindowKey, what: &str) -> bool {
    h.texts(w).iter().any(|t| t.contains(what))
}

#[test]
fn a_failed_memory_read_is_not_reported_as_still_reading() {
    let appearance = Appearance {
        theme: ThemeChoice::Night,
        reduced_motion: true,
        ..Appearance::default()
    };
    let mut h = Harness::with(Genkan::new(Some("memfail".into())), appearance, false);
    let w = h.window();
    assert_eq!(h.program().step, Step::Disk);
    assert!(matches!(
        h.program().memory().swap_size(true),
        SwapSize::Unreadable(_)
    ));
    assert!(!h.program().swap, "the failure turns swap off");
    assert!(says(&h, w, "Could not read installed memory"));
    // The person turns swap back on and waits; nothing can size it.
    h.send(Msg::Swap(true));
    h.advance(Duration::from_secs(60));
    h.send(Msg::Dismissed);
    let page_says_reading = says(&h, w, "Reading installed memory");
    let swap = h.program().swap;
    let memory = h.program().memory().swap_size(true);
    h.send(Msg::Next);
    let step_after_next = h.program().step;
    let next_says_still_reading = says(&h, w, "Still reading installed memory");
    println!(
        "memory={memory:?} swap={swap} page_says_reading={page_says_reading} \
         step_after_next={step_after_next:?} next_says_still_reading={next_says_still_reading}"
    );
    assert!(
        !page_says_reading && !next_says_still_reading,
        "after the read failed, the Disk page still says it is reading installed memory \
         (page: {page_says_reading}, Next: {next_says_still_reading}, step {step_after_next:?}, memory {memory:?})"
    );
    // Swap stays off, and the installation goes on without it.
    assert!(!swap, "swap can't be turned on without installed memory");
    assert_eq!(step_after_next, Step::Account);
}

#[test]
fn a_read_memory_sizes_swap() {
    let mut h = Harness::with(
        Genkan::new(Some("disk".into())),
        Appearance::default(),
        false,
    );
    assert_eq!(
        h.program().memory().swap_size(true),
        SwapSize::Known(32 * 1024u64.pow(3))
    );
    assert_eq!(h.program().swap_bytes(), Some(32 * 1024u64.pow(3)));
    assert!(h.program().layout().is_ok_and(|l| l.swap.is_some()));
    h.send(Msg::Swap(false));
    assert_eq!(h.program().swap_bytes(), None);
    h.send(Msg::Swap(true));
    assert!(h.program().swap);
}
