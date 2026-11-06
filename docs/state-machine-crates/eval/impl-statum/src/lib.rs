//! The six targets with statum 0.9.0 (typestate attributes with typed rehydration).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

/// The payloads of states that carry no data. statum implements a public
/// `CanTransitionTo<S>` for every declared edge into a unit state `S`, which
/// any crate can take without the transition's body; giving `S` a token only
/// its module can make (`S(STok)`) turns that into `CanTransitionWith<STok>`,
/// which outside code can't call without a token. (It can still move one out
/// of a machine already in `S`: see NOTES.md §4.)
macro_rules! tokens {
    ($($t:ident),* $(,)?) => {$(
        #[derive(Debug)]
        pub struct $t(());
    )*};
}

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

/// statum's `#[machine]` generates a public builder for every state
/// (`Machine::<AnyState>::builder()`), so any crate could build any state
/// with any data. Every machine here has a `seal: Seal` field: only this
/// crate can make a `Seal`, so outside it no builder can finish.
#[derive(Debug)]
pub struct Seal(());

pub(crate) const SEAL: Seal = Seal(());
