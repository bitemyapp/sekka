// SPDX-License-Identifier: GPL-3.0-or-later
//! Prints what the installer detects on this machine's GPUs and displays, and
//! the configuration lines it would write. Read-only; needs no privileges.
//!
//! ```text
//! cargo build --release --example detect-graphics --target x86_64-unknown-linux-musl
//! ```
use calamares_nixos::graphics::Graphics;

fn main() -> anyhow::Result<()> {
    let found = Graphics::detect();
    found.check()?;
    println!("{}", serde_json::to_string_pretty(&found)?);
    println!("Review page: {}", found.describe(true));
    if let (Some(nvidia), Some(gpu), true) = (&found.nvidia, &found.integrated, found.offload) {
        println!(
            "calamares.nvidia.prime = {{ nvidiaBusId = \"{nvidia}\"; {} = \"{}\"; }};",
            gpu.option(),
            gpu.bus_id()
        );
    }
    Ok(())
}
