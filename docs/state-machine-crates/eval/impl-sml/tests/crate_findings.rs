//! Behaviour of stateforward-sml 2.0.0 found while writing the targets
//! (see NOTES.md, "Crate findings"). Each test asserts what the release
//! does; a comment says what its documentation says it should do.

use std::cell::RefCell;

#[derive(Default)]
struct Log(RefCell<Vec<&'static str>>);

impl Log {
    fn push(&self, s: &'static str) -> Result<(), ()> {
        self.0.borrow_mut().push(s);
        Ok(())
    }
    fn take(&self) -> Vec<&'static str> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

mod self_transition {
    use super::Log;

    sml::sml! {
        Kid {
            *A + Next = B,
             A + on_entry<_> / enter_a,
             B + on_exit<_> / exit_b,
        }

        Parent {
            *Idle + Go = state<Kid>,
             state<Kid> + Again = state<Kid>,
             state<Kid> + on_entry<_> / enter_kid,
             state<Kid> + on_exit<_> / exit_kid,
        }
    }

    impl ParentStateMachineContext for &Log {
        fn enter_a(&mut self) -> Result<(), ()> {
            self.push("enter A")
        }
        fn exit_b(&mut self) -> Result<(), ()> {
            self.push("exit B")
        }
        fn enter_kid(&mut self) -> Result<(), ()> {
            self.push("enter Kid")
        }
        fn exit_kid(&mut self) -> Result<(), ()> {
            self.push("exit Kid")
        }
    }

    /// docs/dsl.md: "An explicit target equal to the source is an external
    /// self-transition, so exit and entry callbacks do run", and "Leaving and
    /// re-entering the child resets it to its initial state". For a
    /// composite state the parent's hooks run but the child is neither left
    /// nor re-entered: it keeps `B`, and neither `exit B` nor `enter A` runs.
    #[test]
    fn a_composite_self_transition_does_not_leave_or_reenter_the_child() {
        let log = Log::default();
        let mut sm = ParentStateMachine::new(&log);
        sm.process_event(ParentEvents::Go).unwrap();
        sm.process_event(ParentEvents::Next).unwrap();
        log.take();
        sm.process_event(ParentEvents::Again).unwrap();
        assert_eq!(log.take(), ["exit Kid", "enter Kid"]); // expected: exit B, exit Kid, enter Kid, enter A
        assert!(sm.is(&ParentKidStates::B)); // expected: A
    }
}

/// The same with a second child table, which takes the tree generator
/// (`generate_multi_code`): the same result.
mod self_transition_in_a_tree {
    use super::Log;

    sml::sml! {
        Kid {
            *A + Next = B,
             A + on_entry<_> / enter_a,
             B + on_exit<_> / exit_b,
        }

        Other {
            *C + Next = X,
        }

        Parent {
            *Idle + Go = state<Kid>,
             Idle + Elsewhere = state<Other>,
             state<Kid> + Again = state<Kid>,
             state<Kid> + on_entry<_> / enter_kid,
             state<Kid> + on_exit<_> / exit_kid,
        }
    }

    impl ParentStateMachineContext for &Log {
        fn enter_a(&mut self) -> Result<(), ()> {
            self.push("enter A")
        }
        fn exit_b(&mut self) -> Result<(), ()> {
            self.push("exit B")
        }
        fn enter_kid(&mut self) -> Result<(), ()> {
            self.push("enter Kid")
        }
        fn exit_kid(&mut self) -> Result<(), ()> {
            self.push("exit Kid")
        }
    }

    #[test]
    fn a_composite_self_transition_in_a_tree_does_not_reenter_the_child_either() {
        let log = Log::default();
        let mut sm = ParentStateMachine::new(&log);
        assert!(sm.process_event(ParentEvents::Elsewhere).is_ok());
        let mut sm = ParentStateMachine::new(&log);
        sm.process_event(ParentEvents::Go).unwrap();
        sm.process_event(ParentEvents::Next).unwrap();
        log.take();
        sm.process_event(ParentEvents::Again).unwrap();
        assert_eq!(log.take(), ["exit Kid", "enter Kid"]); // expected: exit B, exit Kid, enter Kid, enter A
        assert!(sm.is(&ParentKidStates::B)); // expected: A
    }
}

/// Not only self-transitions: any composite-to-composite edge skips the
/// child's exit, reset and entry (`composite_lifecycle` and the inline
/// completion gates in `composite_codegen.rs` only fire across a
/// composite/non-composite boundary).
mod composite_to_composite {
    use super::Log;

    sml::sml! {
        Ka {
            *A1 + Next = A2,
             A2 + on_exit<_> / exit_a2,
        }

        Kb {
            *B1 + Next = B2,
             B1 + on_entry<_> / enter_b1,
        }

        Swapper {
            *Idle + Go = state<Ka>,
             state<Ka> + Swap = state<Kb>,
             state<Kb> + Swap = state<Ka>,
             state<Ka> + on_exit<_> / exit_ka,
             state<Kb> + on_entry<_> / enter_kb,
        }
    }

    impl SwapperStateMachineContext for &Log {
        fn exit_a2(&mut self) -> Result<(), ()> {
            self.push("exit A2")
        }
        fn enter_b1(&mut self) -> Result<(), ()> {
            self.push("enter B1")
        }
        fn exit_ka(&mut self) -> Result<(), ()> {
            self.push("exit Ka")
        }
        fn enter_kb(&mut self) -> Result<(), ()> {
            self.push("enter Kb")
        }
    }

    #[test]
    fn a_composite_to_composite_edge_skips_both_children() {
        let log = Log::default();
        let mut sm = SwapperStateMachine::new(&log);
        sm.process_event(SwapperEvents::Go).unwrap();
        sm.process_event(SwapperEvents::Next).unwrap();
        log.take();
        sm.process_event(SwapperEvents::Swap).unwrap();
        assert_eq!(log.take(), ["exit Ka", "enter Kb"]); // expected: exit A2, exit Ka, enter Kb, enter B1
        sm.process_event(SwapperEvents::Swap).unwrap();
        assert!(sm.is(&SwapperStates::Ka));
        assert!(sm.is(&SwapperKaStates::A2)); // expected: A1 (no `(H)`, so reset)
    }
}

/// A flat machine dispatches a processed event before the current event's
/// anonymous rows run; sml.cpp would handle `P` in `C`.
mod flat_process {
    use super::Log;

    sml::sml! {
        Flat {
            *A + E / process(FlatEvents::P) = B,
             B / to_c = C,
             B + P / p_in_b,
             C + P / p_in_c,
        }
    }

    impl FlatStateMachineContext for &Log {
        fn to_c(&mut self) -> Result<(), ()> {
            self.push("B->C")
        }
        fn p_in_b(&mut self) -> Result<(), ()> {
            self.push("P in B")
        }
        fn p_in_c(&mut self) -> Result<(), ()> {
            self.push("P in C")
        }
    }

    #[test]
    fn a_processed_event_runs_before_the_anonymous_rows() {
        let log = Log::default();
        let mut sm = FlatStateMachine::new(&log);
        sm.process_event(FlatEvents::E).unwrap();
        assert_eq!(log.take(), ["P in B", "B->C"]); // expected: B->C, P in C
        assert!(sm.is(&FlatStates::C));
    }
}

mod process_order {
    use super::Log;

    // Without a row reaching `X` this table doesn't compile: the
    // orthogonal generator names `RegionsStates::X` ("no variant ... named
    // `X`"), so `Quit` is here only to make it build.
    sml::sml! {
        Regions {
            *A + Quit = X,
             A + E / process(RegionsEvents::P),
             A + F / process(RegionsEvents::Nobody),
            *B + E / saw_e,
             B + F / saw_f,
             B + P / saw_p,
             B + Nobody [never] = B,
        }
    }

    impl RegionsStateMachineContext for &Log {
        fn saw_e(&mut self) -> Result<(), ()> {
            self.push("E")
        }
        fn saw_f(&mut self) -> Result<(), ()> {
            self.push("F")
        }
        fn saw_p(&mut self) -> Result<(), ()> {
            self.push("P")
        }
        fn never(&self) -> Result<bool, ()> {
            Ok(false)
        }
    }

    /// sml.cpp queues `process(...)` events until the current event has run
    /// to completion. A synchronous sml.rs machine dispatches it at once,
    /// inside the step: the second region sees `P` before the `E` that
    /// caused it.
    #[test]
    fn a_processed_event_overtakes_the_current_one_in_later_regions() {
        let log = Log::default();
        let mut sm = RegionsStateMachine::new(&log);
        sm.process_event(RegionsEvents::E).unwrap();
        assert_eq!(log.take(), ["P", "E"]);
    }

    /// The nested dispatch's error is returned with `?`: when nothing takes
    /// the processed event, the current event fails and the regions after
    /// the one that processed it never see it.
    #[test]
    fn an_unhandled_processed_event_aborts_the_current_step() {
        let log = Log::default();
        let mut sm = RegionsStateMachine::new(&log);
        assert!(sm.process_event(RegionsEvents::F).is_err());
        assert_eq!(log.take(), Vec::<&str>::new()); // expected: ["F"]
    }
}
