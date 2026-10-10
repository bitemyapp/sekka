#!/usr/bin/env -S rust-script --force
//! boot-installed: executable Rust entry point; shared implementation in ../rust.
//!
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! respin-tools = { path = "../rust" }
//! ```
fn main() -> anyhow::Result<()> {
    respin_tools::dispatch("boot-installed", std::env::args().skip(1).collect())
}
