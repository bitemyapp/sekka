// SPDX-License-Identifier: GPL-3.0-or-later
//! The target disk and what writes to it: discovery and the disk's proof
//! ([`disk`]), formatting ([`filesystem`]) and encryption ([`encryption`]),
//! and the programs the installer runs ([`Program`]).
//!
//! A program that writes to a disk (wipefs, parted, blockdev, the mkfs
//! family, mkswap, cryptsetup, mount) is named only here, in the modules
//! that take the confirmed disk's proof ([`disk::Confirmed`],
//! [`disk::Device`]): code anywhere else, the rest of the installer
//! included, can't run one on a path it picked. Naming one doesn't compile:
//!
//! ```compile_fail,E0624
//! use calamares_nixos::process::{self, Program};
//! fn erase(path: &str) -> anyhow::Result<()> {
//!     process::output(Program::WIPEFS, &["--all", "--force", path], 60).map(drop)
//! }
//! ```
//!
//! nor does running one by its name, or making a `Program`:
//!
//! ```compile_fail,E0308
//! fn erase(path: &str) -> anyhow::Result<()> {
//!     calamares_nixos::process::output("wipefs", &["--all", "--force", path], 60).map(drop)
//! }
//! ```
//!
//! ```compile_fail,E0423
//! let _ = calamares_nixos::process::Program("wipefs");
//! ```
//!
//! ```
//! use calamares_nixos::process::{self, Program};
//! fn list() -> anyhow::Result<String> {
//!     process::output(Program::LSBLK, &["--json"], 30)
//! }
//! ```
//!
//! `std::process::Command::new`, and libc's ways to start a program, are
//! denied in the crate outside `process` (Clippy's `disallowed_methods`,
//! see `clippy.toml`, also run in the helper's Nix check), so the standard
//! library is no way around it either.

#[path = "disk.rs"]
pub mod disk;
#[path = "encryption.rs"]
pub mod encryption;
#[path = "filesystem.rs"]
pub mod filesystem;

/// A program the installer runs, found on its fixed tool path. Only the
/// installer names its programs: those that change no disk are public, and
/// those that write to one are this module's (see the module's docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program(&'static str);

impl Program {
    pub const BLKID: Self = Self("blkid");
    pub const CURL: Self = Self("curl");
    pub const DMESG: Self = Self("dmesg");
    pub const FINDMNT: Self = Self("findmnt");
    pub const LSBLK: Self = Self("lsblk");
    pub const MODPROBE: Self = Self("modprobe");
    pub const NIX: Self = Self("nix");
    pub const NIX_STORE: Self = Self("nix-store");
    pub const NIXOS_GENERATE_CONFIG: Self = Self("nixos-generate-config");
    /// Installs the bootloader on the mounted target, after the disk's
    /// writes.
    pub const NIXOS_INSTALL: Self = Self("nixos-install");
    pub const SYNC: Self = Self("sync");
    pub const TIMEDATECTL: Self = Self("timedatectl");
    pub const UDEVADM: Self = Self("udevadm");
    pub const UMOUNT: Self = Self("umount");

    // What writes to a disk: only the proof's modules name these.
    const WIPEFS: Self = Self("wipefs");
    const PARTED: Self = Self("parted");
    const BLOCKDEV: Self = Self("blockdev");
    const MKFS_EXT4: Self = Self("mkfs.ext4");
    const MKFS_BTRFS: Self = Self("mkfs.btrfs");
    const MKFS_XFS: Self = Self("mkfs.xfs");
    const MKFS_FAT: Self = Self("mkfs.fat");
    const MKSWAP: Self = Self("mkswap");
    const CRYPTSETUP: Self = Self("cryptsetup");
    const MOUNT: Self = Self("mount");

    /// What the tests in disposable VMs run besides: cryptsetup to reopen
    /// what they wrote, fstrim and loop devices.
    #[cfg(test)]
    pub(crate) const TEST_CRYPTSETUP: Self = Self::CRYPTSETUP;
    #[cfg(test)]
    pub(crate) const TEST_FSTRIM: Self = Self("fstrim");
    #[cfg(test)]
    const LOSETUP: Self = Self("losetup");

    pub fn name(self) -> &'static str {
        self.0
    }
}
