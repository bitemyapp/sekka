#!/usr/bin/env -S rust-script --force
//! prepare-builder: executable Rust entry point; shared implementation in ../rust.
//!
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! respin-tools = { path = "../rust" }
//! ```
fn main() -> anyhow::Result<()> {
    respin_tools::dispatch("prepare-builder", std::env::args().skip(1).collect())
}
