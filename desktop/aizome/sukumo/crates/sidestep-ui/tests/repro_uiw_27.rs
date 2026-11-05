//! UIW-27 (regression): a `SessionLockFinished` for a lock the program has already
//! withdrawn (unlocked) must not end the lock it asked for next.
//!
//! Through the null render thread: the first lock is confirmed; at once
//! the program unlocks and locks again (with a new lock surface), while
//! the compositor's `finished` for the first lock is already on its way
//! to the main thread (injected, as `testing` does, after what the render
//! thread sent). The render thread then confirms the second lock.

#[cfg(not(target_vendor = "apple"))]
mod linux {
    use std::time::{Duration, Instant};

    use sidestep_ui::testing::{self, Seen};
    use sidestep_ui::*;

    const DEADLINE: Duration = Duration::from_secs(20);

    #[derive(Default)]
    pub struct Lock {
        started: Option<Instant>,
        relocked: Option<Instant>,
        lock2: Option<WindowId>,
        locked: usize,
        finished: usize,
        /// Whether the program believes the session is locked, whether the
        /// second lock's surface is open, and what the render thread saw.
        pub result: Option<(bool, bool, Vec<Seen>)>,
        pub counts: (usize, usize),
    }

    impl Handler for Lock {
        fn launched(&mut self, cx: &mut Cx) {
            self.started = Some(Instant::now());
            cx.lock_session();
            cx.open_window(WindowOptions::lock(61));
            cx.set_repeating_timer(Duration::from_millis(2));
        }

        fn draw(&mut self, _cx: &mut Cx, _window: WindowId, _canvas: &mut Canvas) {}

        fn session_locked(&mut self, cx: &mut Cx) {
            self.locked += 1;
            if self.locked == 1 {
                let first = testing::session_lock_request(cx).expect("the first lock held");
                // Unlocked, and locked again at once (a lock screen that
                // unlocks and the idle timer locks again) …
                cx.unlock_session();
                cx.lock_session();
                self.lock2 = Some(cx.open_window(WindowOptions::lock(61)));
                self.relocked = Some(Instant::now());
                // … while the compositor's `finished` for the first lock
                // is already in flight.
                testing::inject_session_lock_finished_for(cx, first);
            }
        }

        fn session_lock_finished(&mut self, _cx: &mut Cx) {
            self.finished += 1;
        }

        fn timer(&mut self, cx: &mut Cx, _timer: TimerId) {
            let started = self.started.expect("launched");
            assert!(started.elapsed() < DEADLINE, "didn't settle in {DEADLINE:?}");
            let Some(at) = self.relocked else { return };
            if at.elapsed() < Duration::from_millis(200) || !testing::render_idle() || self.result.is_some() {
                return;
            }
            let open = self.lock2.is_some_and(|w| cx.window(w).is_some());
            self.result = Some((cx.is_session_locked(), open, testing::take_render_log()));
            self.counts = (self.locked, self.finished);
            cx.quit();
        }
    }

    pub fn run() -> Lock {
        // SAFETY: the test's only thread that reads the environment yet.
        unsafe {
            std::env::set_var("SIDESTEP_APPEARANCE", "light");
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        testing::use_null_backend();
        App::new().quit_when_last_window_closes(false).run(Lock::default()).expect("the null render thread runs")
    }
}

#[cfg(not(target_vendor = "apple"))]
#[test]
fn repro_uiw_27_stale_lock_finished_does_not_end_the_next_lock() {
    let lock = linux::run();
    let (locked, open, log) = lock.result.expect("a result");
    let locks: Vec<_> = log.iter().filter(|s| matches!(s, sidestep_ui::testing::Seen::SessionLock { .. })).collect();
    println!(
        "program believes locked: {locked}; second lock surface open: {open}; session_locked/finished heard: {:?}; render thread saw: {locks:?}",
        lock.counts
    );
    assert!(open, "the second lock's surface must stay open (the stale finished closed it)");
    assert!(locked, "the compositor confirmed the second lock: the program must know it's locked");
}
