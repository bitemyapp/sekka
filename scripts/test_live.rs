#!/usr/bin/env -S rust-script --force
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! respin-tools = { path = "../rust" }
//! ```
fn main() -> anyhow::Result<()> {
    respin_tools::dispatch("live-test", std::env::args().skip(1).collect())
}
