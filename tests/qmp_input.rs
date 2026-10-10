#!/usr/bin/env -S rust-script --force
//! QMP controls for a disposable graphical installation test only.
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! respin-tools = { path = "../rust" }
//! ```
fn main() -> anyhow::Result<()> {
    respin_tools::dispatch("qmp-input", std::env::args().skip(1).collect())
}
