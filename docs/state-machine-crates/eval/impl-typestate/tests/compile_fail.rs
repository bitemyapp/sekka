//! The compile-fail probes of this implementation:
//! `compile-fail/cases/typestate/<target>/{reject,runtime,accept}/` (see IMPLEMENTING.md).

#[test]
fn compile_fail() {
    compile_fail::run("typestate");
}
