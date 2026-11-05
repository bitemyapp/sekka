//! Specifications of six Sekka state machines, for comparing ways of
//! writing them: the plain-Rust baselines and the state-machine crates.
//!
//! Each target module (`pointer`, `wrap`, `polkit`, `transfer`, `lock`,
//! `install`) defines, with no I/O:
//!
//! - `Config`, `Event` (inputs), `Effect` (outputs the machine asks for:
//!   deliver an event to a node, start a helper, arm a timer, move a file)
//!   and `StateName` (a small `Copy` observation of where it is);
//! - the `Machine` trait every implementation provides;
//! - `SPEC.md` beside it: the state chart, guards, effects and their order,
//!   invariants, the illegal operations the compile-fail suite probes, and
//!   the catalog's confirmed-bug scenarios;
//! - with the `testing` feature: conformance traces and the random
//!   environment the differential property tests drive.
//!
//! Effects go to a [`Sink`], which implementations take generically
//! (`out: &mut impl Sink<Effect>`), so benchmarks can count or digest them
//! instead of growing a `Vec`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

#[cfg(feature = "testing")]
pub mod testing;

use std::hash::{Hash, Hasher};

/// Where a machine's effects go.
pub trait Sink<E> {
    fn push(&mut self, effect: E);
}

impl<E> Sink<E> for Vec<E> {
    #[inline]
    fn push(&mut self, effect: E) {
        Vec::push(self, effect);
    }
}

/// Counts effects and drops them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Count(pub u64);

impl<E> Sink<E> for Count {
    #[inline]
    fn push(&mut self, _: E) {
        self.0 += 1;
    }
}

/// Folds every effect into a 64-bit digest (FxHash-style), so the work of
/// computing an effect can't be optimized away and two runs can be compared
/// without storing their effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct Digest(pub u64);

impl<E: Hash> Sink<E> for Digest {
    #[inline]
    fn push(&mut self, effect: E) {
        let mut h = Fx(self.0);
        effect.hash(&mut h);
        self.0 = h.0.rotate_left(5) ^ 0x9e37_79b9_7f4a_7c15;
    }
}

struct Fx(u64);

impl Hasher for Fx {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.write_u64(u64::from(n));
    }
    #[inline]
    fn write_u16(&mut self, n: u16) {
        self.write_u64(u64::from(n));
    }
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }
    #[inline]
    fn write_i32(&mut self, n: i32) {
        self.write_u64(n as u32 as u64);
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn write_isize(&mut self, n: isize) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// A small deterministic generator (SplitMix64) for workloads, so the
/// spec crate needs no `rand`.
#[derive(Clone, Debug)]
pub struct SplitMix(pub u64);

impl SplitMix {
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (n > 0).
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }
    /// True with probability `per_mille / 1000`.
    #[inline]
    pub fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
}
