#!/usr/bin/env -S rust-script --force
//! Complete ISO installs over previously formatted disks, followed by disk-only boots.
//! Includes all applications, no applications, and Rustup's build-tools dependency.
//!
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! respin-tools = { path = "../rust" }
//! ```
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 1, "Usage: qemu_storage_matrix.rs ISO");
    for (firmware, bus) in [("uefi", "nvme"), ("uefi", "nvme4k"), ("bios", "virtio")] {
        for (filesystem, previous) in [("ext4", "btrfs"), ("btrfs", "xfs"), ("xfs", "ext4")] {
            let applications = match (firmware, bus, filesystem) {
                ("uefi", "nvme", "ext4") => "all",
                ("uefi", "nvme4k", "ext4") => "rustup",
                ("bios", "virtio", "ext4") => "none",
                _ => "firefox",
            };
            respin_tools::dispatch(
                "qemu-test",
                vec![
                    args[0].clone(),
                    "--install".into(),
                    "--firmware".into(),
                    firmware.into(),
                    "--disk-bus".into(),
                    bus.into(),
                    "--filesystem".into(),
                    filesystem.into(),
                    "--previous-filesystem".into(),
                    previous.into(),
                    "--applications".into(),
                    applications.into(),
                ],
            )?;
        }
    }
    respin_tools::dispatch(
        "qemu-test",
        vec![
            args[0].clone(),
            "--install".into(),
            "--disk-bus".into(),
            "virtio".into(),
            "--previous-filesystem".into(),
            "blank".into(),
        ],
    )
}
