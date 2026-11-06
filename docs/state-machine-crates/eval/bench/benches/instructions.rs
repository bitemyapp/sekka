//! Instruction counts (valgrind's callgrind, through gungraun) for every
//! implemented machine, on smaller workloads than the criterion benches:
//! 20k pointer events, 200k printed cells at 80 and 200 columns, and the
//! trace replays. Counts are stable on a noisy host.
//!
//! Needs valgrind on PATH and `GUNGRAUN_RUNNER` naming a gungraun-runner
//! 0.20.0 (see IMPLEMENTING.md):
//!
//! ```text
//! PATH=$VALGRIND/bin:$PATH GUNGRAUN_RUNNER=$RUNNER \
//!   cargo bench -p bench --features iai --bench instructions
//! ```
//!
//! Bench ids are the implementations (`enum_` for impl-enum); a target an
//! implementation hasn't done (`IMPLEMENTED = false`) reports 0 work.

use std::hint::black_box;

use bench::impls;
use gungraun::prelude::*;
use spec::{install, lock, pointer, polkit, transfer, wrap};

macro_rules! tables {
    ($($name:literal => $krate:ident),* $(,)?) => {
        macro_rules! table {
            ($t:ident, $Type:ident) => {
                pub mod $t {
                    pub type Set = Vec<(spec::$t::Config, Vec<spec::$t::Event>)>;
                    pub type Run = fn(&[(spec::$t::Config, Vec<spec::$t::Event>)]) -> u64;
                    pub const ALL: &[(&str, bool, Run)] = &[
                        $( ($name, bench::$krate::$t::IMPLEMENTED, bench::drive::$t::replay::<bench::$krate::$t::$Type>) ),*
                    ];
                }
            };
        }
        bench::targets!(table);
    };
}

mod tables {
    use super::*;
    impls!(tables);
}

fn pointer_stream_input(which: usize) -> (usize, tables::pointer::Set) {
    (which, vec![(pointer::Config::default(), pointer::workload::stream(0xC0FFEE, 20_000))])
}
fn wrap_input(which: usize, cols: u16) -> (usize, tables::wrap::Set) {
    let rows = if cols >= 200 { 50 } else { 24 };
    (which, vec![(wrap::Config { cols, rows }, wrap::workload::stream(0x7E57 + u64::from(cols), cols, rows, 200_000))])
}
fn wrap_80_input(which: usize) -> (usize, tables::wrap::Set) {
    wrap_input(which, 80)
}
fn wrap_200_input(which: usize) -> (usize, tables::wrap::Set) {
    wrap_input(which, 200)
}

macro_rules! replay_input {
    ($f:ident, $t:ident) => {
        fn $f(which: usize) -> (usize, tables::$t::Set) {
            (which, bench::replay::<$t::traces::Spec>())
        }
    };
}
replay_input!(replay_pointer_input, pointer);
replay_input!(replay_wrap_input, wrap);
replay_input!(replay_polkit_input, polkit);
replay_input!(replay_transfer_input, transfer);
replay_input!(replay_lock_input, lock);
replay_input!(replay_install_input, install);

// The functions below are written out (generated once by a shell loop):
// `#[library_benchmark]` isn't hygienic inside `macro_rules!`.

#[library_benchmark]
#[bench::implicit(args = (0), setup = pointer_stream_input)]
#[bench::enum_(args = (1), setup = pointer_stream_input)]
#[bench::typestate(args = (2), setup = pointer_stream_input)]
#[bench::statig(args = (3), setup = pointer_stream_input)]
#[bench::smlang(args = (4), setup = pointer_stream_input)]
#[bench::sml(args = (5), setup = pointer_stream_input)]
#[bench::finny(args = (6), setup = pointer_stream_input)]
#[bench::fsmentry(args = (7), setup = pointer_stream_input)]
#[bench::statum(args = (8), setup = pointer_stream_input)]
#[bench::state_machines(args = (9), setup = pointer_stream_input)]
#[bench::rust_fsm(args = (10), setup = pointer_stream_input)]
fn pointer_stream(input: (usize, tables::pointer::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::pointer::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = wrap_80_input)]
#[bench::enum_(args = (1), setup = wrap_80_input)]
#[bench::typestate(args = (2), setup = wrap_80_input)]
#[bench::statig(args = (3), setup = wrap_80_input)]
#[bench::smlang(args = (4), setup = wrap_80_input)]
#[bench::sml(args = (5), setup = wrap_80_input)]
#[bench::finny(args = (6), setup = wrap_80_input)]
#[bench::fsmentry(args = (7), setup = wrap_80_input)]
#[bench::statum(args = (8), setup = wrap_80_input)]
#[bench::state_machines(args = (9), setup = wrap_80_input)]
#[bench::rust_fsm(args = (10), setup = wrap_80_input)]
fn wrap_80(input: (usize, tables::wrap::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::wrap::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = wrap_200_input)]
#[bench::enum_(args = (1), setup = wrap_200_input)]
#[bench::typestate(args = (2), setup = wrap_200_input)]
#[bench::statig(args = (3), setup = wrap_200_input)]
#[bench::smlang(args = (4), setup = wrap_200_input)]
#[bench::sml(args = (5), setup = wrap_200_input)]
#[bench::finny(args = (6), setup = wrap_200_input)]
#[bench::fsmentry(args = (7), setup = wrap_200_input)]
#[bench::statum(args = (8), setup = wrap_200_input)]
#[bench::state_machines(args = (9), setup = wrap_200_input)]
#[bench::rust_fsm(args = (10), setup = wrap_200_input)]
fn wrap_200(input: (usize, tables::wrap::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::wrap::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_pointer_input)]
#[bench::enum_(args = (1), setup = replay_pointer_input)]
#[bench::typestate(args = (2), setup = replay_pointer_input)]
#[bench::statig(args = (3), setup = replay_pointer_input)]
#[bench::smlang(args = (4), setup = replay_pointer_input)]
#[bench::sml(args = (5), setup = replay_pointer_input)]
#[bench::finny(args = (6), setup = replay_pointer_input)]
#[bench::fsmentry(args = (7), setup = replay_pointer_input)]
#[bench::statum(args = (8), setup = replay_pointer_input)]
#[bench::state_machines(args = (9), setup = replay_pointer_input)]
#[bench::rust_fsm(args = (10), setup = replay_pointer_input)]
fn replay_pointer(input: (usize, tables::pointer::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::pointer::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_wrap_input)]
#[bench::enum_(args = (1), setup = replay_wrap_input)]
#[bench::typestate(args = (2), setup = replay_wrap_input)]
#[bench::statig(args = (3), setup = replay_wrap_input)]
#[bench::smlang(args = (4), setup = replay_wrap_input)]
#[bench::sml(args = (5), setup = replay_wrap_input)]
#[bench::finny(args = (6), setup = replay_wrap_input)]
#[bench::fsmentry(args = (7), setup = replay_wrap_input)]
#[bench::statum(args = (8), setup = replay_wrap_input)]
#[bench::state_machines(args = (9), setup = replay_wrap_input)]
#[bench::rust_fsm(args = (10), setup = replay_wrap_input)]
fn replay_wrap(input: (usize, tables::wrap::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::wrap::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_polkit_input)]
#[bench::enum_(args = (1), setup = replay_polkit_input)]
#[bench::typestate(args = (2), setup = replay_polkit_input)]
#[bench::statig(args = (3), setup = replay_polkit_input)]
#[bench::smlang(args = (4), setup = replay_polkit_input)]
#[bench::sml(args = (5), setup = replay_polkit_input)]
#[bench::finny(args = (6), setup = replay_polkit_input)]
#[bench::fsmentry(args = (7), setup = replay_polkit_input)]
#[bench::statum(args = (8), setup = replay_polkit_input)]
#[bench::state_machines(args = (9), setup = replay_polkit_input)]
#[bench::rust_fsm(args = (10), setup = replay_polkit_input)]
fn replay_polkit(input: (usize, tables::polkit::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::polkit::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_transfer_input)]
#[bench::enum_(args = (1), setup = replay_transfer_input)]
#[bench::typestate(args = (2), setup = replay_transfer_input)]
#[bench::statig(args = (3), setup = replay_transfer_input)]
#[bench::smlang(args = (4), setup = replay_transfer_input)]
#[bench::sml(args = (5), setup = replay_transfer_input)]
#[bench::finny(args = (6), setup = replay_transfer_input)]
#[bench::fsmentry(args = (7), setup = replay_transfer_input)]
#[bench::statum(args = (8), setup = replay_transfer_input)]
#[bench::state_machines(args = (9), setup = replay_transfer_input)]
#[bench::rust_fsm(args = (10), setup = replay_transfer_input)]
fn replay_transfer(input: (usize, tables::transfer::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::transfer::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_lock_input)]
#[bench::enum_(args = (1), setup = replay_lock_input)]
#[bench::typestate(args = (2), setup = replay_lock_input)]
#[bench::statig(args = (3), setup = replay_lock_input)]
#[bench::smlang(args = (4), setup = replay_lock_input)]
#[bench::sml(args = (5), setup = replay_lock_input)]
#[bench::finny(args = (6), setup = replay_lock_input)]
#[bench::fsmentry(args = (7), setup = replay_lock_input)]
#[bench::statum(args = (8), setup = replay_lock_input)]
#[bench::state_machines(args = (9), setup = replay_lock_input)]
#[bench::rust_fsm(args = (10), setup = replay_lock_input)]
fn replay_lock(input: (usize, tables::lock::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::lock::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

#[library_benchmark]
#[bench::implicit(args = (0), setup = replay_install_input)]
#[bench::enum_(args = (1), setup = replay_install_input)]
#[bench::typestate(args = (2), setup = replay_install_input)]
#[bench::statig(args = (3), setup = replay_install_input)]
#[bench::smlang(args = (4), setup = replay_install_input)]
#[bench::sml(args = (5), setup = replay_install_input)]
#[bench::finny(args = (6), setup = replay_install_input)]
#[bench::fsmentry(args = (7), setup = replay_install_input)]
#[bench::statum(args = (8), setup = replay_install_input)]
#[bench::state_machines(args = (9), setup = replay_install_input)]
#[bench::rust_fsm(args = (10), setup = replay_install_input)]
fn replay_install(input: (usize, tables::install::Set)) -> u64 {
    let (which, set) = input;
    let (_, implemented, run) = tables::install::ALL[which];
    if !implemented {
        return 0;
    }
    black_box(run(black_box(&set)))
}

library_benchmark_group!(
    name = machines,
    benchmarks = [
        pointer_stream,
        wrap_80,
        wrap_200,
        replay_pointer,
        replay_wrap,
        replay_polkit,
        replay_transfer,
        replay_lock,
        replay_install
    ]
);

main!(library_benchmark_groups = machines);
