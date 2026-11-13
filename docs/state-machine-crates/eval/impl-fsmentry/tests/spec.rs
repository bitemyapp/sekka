//! Conformance and differential tests of `impl-fsmentry` (see IMPLEMENTING.md).

use impl_fsmentry as m;
use spec::testing::{conformance, differential};
use spec::{install, lock, pointer, polkit, transfer, wrap};

/// Trace names this implementation can't pass, each explained in NOTES.md
/// (empty when everything is expressed).
const NOT_EXPRESSED: &[&str] = &[];

#[test]
fn pointer_conformance() {
    conformance::<pointer::traces::Spec, m::pointer::Pointer>(NOT_EXPRESSED);
}
#[test]
fn pointer_differential() {
    differential::<pointer::traces::Spec, m::pointer::Pointer, impl_enum::pointer::Pointer>();
}
#[test]
fn wrap_conformance() {
    conformance::<wrap::traces::Spec, m::wrap::Wrap>(NOT_EXPRESSED);
}
#[test]
fn wrap_differential() {
    differential::<wrap::traces::Spec, m::wrap::Wrap, impl_enum::wrap::Wrap>();
}
#[test]
fn polkit_conformance() {
    conformance::<polkit::traces::Spec, m::polkit::Polkit>(NOT_EXPRESSED);
}
#[test]
fn polkit_differential() {
    differential::<polkit::traces::Spec, m::polkit::Polkit, impl_enum::polkit::Polkit>();
}
#[test]
fn transfer_conformance() {
    conformance::<transfer::traces::Spec, m::transfer::Transfer>(NOT_EXPRESSED);
}
#[test]
fn transfer_differential() {
    differential::<transfer::traces::Spec, m::transfer::Transfer, impl_enum::transfer::Transfer>();
}
#[test]
fn lock_conformance() {
    conformance::<lock::traces::Spec, m::lock::Lock>(NOT_EXPRESSED);
}
#[test]
fn lock_differential() {
    differential::<lock::traces::Spec, m::lock::Lock, impl_enum::lock::Lock>();
}
#[test]
fn install_conformance() {
    conformance::<install::traces::Spec, m::install::Install>(NOT_EXPRESSED);
}
#[test]
fn install_differential() {
    differential::<install::traces::Spec, m::install::Install, impl_enum::install::Install>();
}
