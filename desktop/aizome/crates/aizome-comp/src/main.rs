//! Aizome's compositor. The core (`aizome-comp-core`, a Rust dylib) owns
//! the session: the Wayland display, every client, outputs, input and IPC.
//! It loads the logic (`aizome-comp-logic`) at run time and swaps it for a
//! new build on `aizomectl reload` without a client noticing
//! (docs/reload.md).

fn main() -> std::process::ExitCode {
    aizome_comp_core::run()
}
