//! What the benchmarks, the smoke run and the size report share: the
//! registry of implementations, the workloads, and drivers that feed a
//! machine a workload into a [`Digest`] sink.
//!
//! **Registry.** [`impls!`] lists every implementation crate. A crate agent
//! never edits it: each `impl-<crate>` is listed from the start, and a
//! target whose module says `IMPLEMENTED = false` is skipped.

use spec::testing::Target;
use spec::{pointer, wrap};

pub use impl_enum;
pub use impl_implicit;
pub use impl_typestate;

/// A crate slot: the crate when its feature is on, else a placeholder whose
/// targets all say `IMPLEMENTED = false` (so the registry skips them).
macro_rules! slot {
    ($feature:literal, $krate:ident) => {
        #[cfg(feature = $feature)]
        pub use $krate;
        #[cfg(not(feature = $feature))]
        pub mod $krate {
            //! Placeholder: this slot's feature is off.
            macro_rules! placeholder {
                ($t:ident, $Type:ident) => {
                    pub mod $t {
                        pub const IMPLEMENTED: bool = false;
                        pub type $Type = impl_enum::$t::$Type;
                    }
                };
            }
            crate::targets!(placeholder);
        }
    };
}

slot!("statig", impl_statig);
slot!("smlang", impl_smlang);
slot!("sml", impl_sml);
slot!("finny", impl_finny);
slot!("fsmentry", impl_fsmentry);
slot!("statum", impl_statum);
slot!("state-machines", impl_state_machines);
slot!("rust-fsm", impl_rust_fsm);

/// Calls `$callback! { "name" => crate_ident, ... }` with every implementation.
#[macro_export]
macro_rules! impls {
    ($callback:ident) => {
        $callback! {
            "implicit" => impl_implicit,
            "enum" => impl_enum,
            "typestate" => impl_typestate,
            "statig" => impl_statig,
            "smlang" => impl_smlang,
            "sml" => impl_sml,
            "finny" => impl_finny,
            "fsmentry" => impl_fsmentry,
            "statum" => impl_statum,
            "state-machines" => impl_state_machines,
            "rust-fsm" => impl_rust_fsm,
        }
    };
}

/// Calls `$callback!(module, Type)` for each target.
#[macro_export]
macro_rules! targets {
    ($callback:ident) => {
        $callback!(pointer, Pointer);
        $callback!(wrap, Wrap);
        $callback!(polkit, Polkit);
        $callback!(transfer, Transfer);
        $callback!(lock, Lock);
        $callback!(install, Install);
    };
}

/// Whether smoke mode is on (`SME_SMOKE=1`): small workloads, short runs.
pub fn smoke() -> bool {
    std::env::var("SME_SMOKE").is_ok_and(|v| v != "0" && !v.is_empty())
}

/// The pointer stream: 1 kHz motion with presses, drags, edges, captures,
/// leaves and compositor drags.
pub fn pointer_stream() -> Vec<pointer::Event> {
    pointer::workload::stream(0xC0FFEE, if smoke() { 20_000 } else { 1_000_000 })
}

/// Terminal output for a `cols`-column screen: about 10M printed cells
/// (200k in smoke mode).
pub fn wrap_stream(cols: u16) -> (wrap::Config, Vec<wrap::Event>) {
    let rows = if cols >= 200 { 50 } else { 24 };
    let cells = if smoke() { 200_000 } else { 10_000_000 };
    (wrap::Config { cols, rows }, wrap::workload::stream(0x7E57 + u64::from(cols), cols, rows, cells))
}

/// Every conformance trace of `T`, as (config, events), for replay.
pub fn replay<T: Target>() -> Vec<(T::Config, Vec<T::Event>)> {
    T::traces().into_iter().map(|t| (t.config.clone(), t.events().cloned().collect())).collect()
}

/// The number of events in a replay set.
pub fn replay_len<C, E>(set: &[(C, Vec<E>)]) -> u64 {
    set.iter().map(|(_, e)| e.len() as u64).sum()
}

macro_rules! drivers {
    ($t:ident, $Type:ident) => {
        pub mod $t {
            use spec::Digest;

            /// Feeds `events` to a fresh `M`; returns the digest of its effects and final state.
            #[inline(never)]
            pub fn run<M: spec::$t::Machine>(config: spec::$t::Config, events: &[spec::$t::Event]) -> u64 {
                let mut m = M::new(config);
                let mut sink = Digest(0);
                for e in events {
                    m.handle(*e, &mut sink);
                }
                let mut h = std::hash::DefaultHasher::new();
                std::hash::Hash::hash(&m.state(), &mut h);
                sink.0 ^ std::hash::Hasher::finish(&h)
            }

            /// Replays every trace, each on a fresh machine.
            #[inline(never)]
            pub fn replay<M: spec::$t::Machine>(set: &[(spec::$t::Config, Vec<spec::$t::Event>)]) -> u64 {
                set.iter().fold(0u64, |acc, (c, events)| acc.rotate_left(7) ^ run::<M>(*c, events))
            }
        }
    };
}

/// Drivers per target: `drive::pointer::run::<M>(config, &events)`.
pub mod drive {

    targets!(drivers);
}
