//! The six targets with stateforward-sml 2.0.0 (renamed `sml` here: Boost.SML DSL with composites, regions, history, defer).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

/// A one-event defer queue. A target defers only the event that enters a
/// state (`/ defer = state<…>`), and the generated code re-dispatches it
/// before the step ends; the default queue holds 16 events in every machine.
#[derive(Default)]
pub struct DeferOne;

impl sml::QueuePolicy for DeferOne {
    type Queue<E> = sml::utility::EventQueue<E, 1>;

    fn new_queue<E>(&self) -> Self::Queue<E> {
        sml::utility::EventQueue::new()
    }
}

sml::sml_policies!(OneDeferred { defer_queue: DeferOne });
