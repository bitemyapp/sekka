//! The implicit baseline keeps the current code's confirmed bugs: exactly
//! the confirmed-bug traces must fail (a baseline result, recorded in
//! SME_RESULTS when set). With `--features fixed` the bugs are off, every
//! trace must pass, and the differential tests compare the rest of this
//! shape with impl-enum.

use impl_implicit as m;
use impl_implicit::BUGS;
use spec::testing::{Target, bug_traces, conformance};
use spec::{install, lock, pointer, polkit, transfer, wrap};

fn expected<T: Target>() -> Vec<&'static str> {
    if BUGS { bug_traces::<T>() } else { Vec::new() }
}

#[test]
fn pointer_conformance() {
    conformance::<pointer::traces::Spec, m::pointer::Pointer>(&expected::<pointer::traces::Spec>());
}
#[test]
fn wrap_conformance() {
    conformance::<wrap::traces::Spec, m::wrap::Wrap>(&expected::<wrap::traces::Spec>());
}
#[test]
fn polkit_conformance() {
    conformance::<polkit::traces::Spec, m::polkit::Polkit>(&expected::<polkit::traces::Spec>());
}
#[test]
fn transfer_conformance() {
    conformance::<transfer::traces::Spec, m::transfer::Transfer>(&expected::<transfer::traces::Spec>());
}
#[test]
fn lock_conformance() {
    conformance::<lock::traces::Spec, m::lock::Lock>(&expected::<lock::traces::Spec>());
}
#[test]
fn install_conformance() {
    conformance::<install::traces::Spec, m::install::Install>(&expected::<install::traces::Spec>());
}

#[cfg(feature = "fixed")]
mod differential {
    use super::m;
    use spec::testing::differential;
    use spec::{install, lock, pointer, polkit, transfer, wrap};

    #[test]
    fn pointer() {
        differential::<pointer::traces::Spec, m::pointer::Pointer, impl_enum::pointer::Pointer>();
    }
    #[test]
    fn wrap() {
        differential::<wrap::traces::Spec, m::wrap::Wrap, impl_enum::wrap::Wrap>();
    }
    #[test]
    fn polkit() {
        differential::<polkit::traces::Spec, m::polkit::Polkit, impl_enum::polkit::Polkit>();
    }
    #[test]
    fn transfer() {
        differential::<transfer::traces::Spec, m::transfer::Transfer, impl_enum::transfer::Transfer>();
    }
    #[test]
    fn lock() {
        differential::<lock::traces::Spec, m::lock::Lock, impl_enum::lock::Lock>();
    }
    #[test]
    fn install() {
        differential::<install::traces::Spec, m::install::Install, impl_enum::install::Install>();
    }
}
