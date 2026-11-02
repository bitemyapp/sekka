//! Worker threads that lay text out in parallel.
//!
//! A display pass that draws many lines nobody has measured before (a new
//! page, a tile scrolled into view) defers laying them out until its end,
//! then lays them all out at once: the calling thread takes jobs alongside
//! the workers, and waits for the ones they took. Shaping is the costly
//! part of drawing new text, and lines are independent, so a page lays out
//! about as many times faster as there are cores.
//!
//! Each worker keeps its own [`Ctx`](super::Ctx), as every thread that lays
//! text out does. Jobs carry only Rust data (text, attributes, options), no
//! Objective-C objects.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::Ctx;
use super::layout::{Job, TextLayout, compute};

/// Jobs of one kind, each laid out with the same function.
struct Batch<J, R> {
    /// A job, until a thread takes it; then the job with its result.
    slots: Vec<Mutex<Slot<J, R>>>,
    /// The next job nobody has taken.
    next: AtomicUsize,
    done: Mutex<usize>,
    all_done: Condvar,
    lay_out: fn(&mut Ctx, &J) -> R,
}

enum Slot<J, R> {
    Waiting(J),
    Taken,
    /// Preserve a worker's panic until every job in the batch has finished.
    Done(J, std::thread::Result<R>),
}

/// A batch a worker can help with, whatever its jobs are.
trait Work: Send + Sync {
    fn work(&self);
}

impl<J: Send, R: Send> Work for Batch<J, R> {
    /// Take jobs until none are left.
    fn work(&self) {
        loop {
            let index = self.next.fetch_add(1, Ordering::Relaxed);
            let Some(slot) = self.slots.get(index) else { return };
            let Slot::Waiting(job) = std::mem::replace(&mut *lock(slot), Slot::Taken) else { continue };
            // A panic here must not leave the caller waiting.
            let result = catch_unwind(AssertUnwindSafe(|| super::with_ctx(|ctx| (self.lay_out)(ctx, &job))));
            if result.is_err() {
                super::discard_ctx();
            }
            *lock(slot) = Slot::Done(job, result);
            let mut done = lock(&self.done);
            *done += 1;
            if *done == self.slots.len() {
                self.all_done.notify_all();
            }
        }
    }
}

fn workers() -> &'static [Sender<Arc<dyn Work>>] {
    static WORKERS: OnceLock<Vec<Sender<Arc<dyn Work>>>> = OnceLock::new();
    WORKERS.get_or_init(|| {
        let count = std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).clamp(1, 7);
        (0..count)
            .filter_map(|i| {
                let (tx, rx) = mpsc::channel::<Arc<dyn Work>>();
                let spawned = std::thread::Builder::new()
                    .name(format!("sidestep-text-{i}"))
                    .spawn(move || rx.iter().for_each(|batch| batch.work()));
                spawned.ok().map(|_| tx)
            })
            .collect()
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lay `jobs` out with `lay_out`, in parallel if there are other cores to
/// help, and hand them back with their results, in order. After all jobs
/// finish, rethrow a failed layout on the caller, just as a synchronous
/// layout does. Never cache empty text as a successful result.
pub fn map<J: Send + 'static, R: Send + 'static>(jobs: Vec<J>, lay_out: fn(&mut Ctx, &J) -> R) -> Vec<(J, R)> {
    let count = jobs.len();
    let batch = Arc::new(Batch {
        slots: jobs.into_iter().map(|job| Mutex::new(Slot::Waiting(job))).collect(),
        next: AtomicUsize::new(0),
        done: Mutex::new(0),
        all_done: Condvar::new(),
        lay_out,
    });
    for worker in workers().iter().take(count.saturating_sub(1)) {
        // A worker that has gone leaves its share to the others.
        let _ = worker.send(batch.clone() as Arc<dyn Work>);
    }
    batch.work();
    let mut done = lock(&batch.done);
    while *done < count {
        done = batch.all_done.wait(done).unwrap_or_else(|e| e.into_inner());
    }
    drop(done);
    batch
        .slots
        .iter()
        .map(|slot| match std::mem::replace(&mut *lock(slot), Slot::Taken) {
            Slot::Done(job, Ok(result)) => (job, result),
            Slot::Done(_, Err(panic)) => resume_unwind(panic),
            _ => unreachable!("all text jobs completed before collecting their results"),
        })
        .collect()
}

/// Lay `jobs` out, in parallel if there are other cores to help, and hand
/// them back with their layouts, in order.
pub fn run(jobs: Vec<Job>) -> Vec<(Job, TextLayout)> {
    map(jobs, |ctx, job| compute(ctx, &job.text, &job.attrs, &job.runs, &job.opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_batch_finishes_then_propagates_the_original_panic() {
        let completed = Arc::new(AtomicUsize::new(0));
        let jobs = (0..32).map(|i| (i, completed.clone())).collect();
        let result = catch_unwind(|| {
            map(jobs, |_, (i, completed)| {
                completed.fetch_add(1, Ordering::SeqCst);
                assert_ne!(*i, 7, "layout fixture failure");
                *i
            })
        });
        let panic = result.expect_err("a layout failure must reach the caller");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("layout fixture failure"));
        assert_eq!(completed.load(Ordering::SeqCst), 32, "no jobs remain when the caller unwinds");
        assert_eq!(
            map(vec![1, 2, 3], |_, n| n * 2),
            vec![(1, 2), (2, 4), (3, 6)],
            "the pool remains usable after failed jobs"
        );
    }
}
