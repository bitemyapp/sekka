// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cleaning up: deleting old system generations, or the ones the person
//! chose, then collecting garbage.
//!
//! The boot menu lists the system's generations. Once some are deleted,
//! nothing but the menu's entries leads to their systems until
//! `switch-to-configuration boot` writes the menu again, and collecting
//! garbage before that removes what those entries start. So deleting
//! generations and updating the boot menu make one step that a stop
//! doesn't split ([`Generations`], [`Deleted`]), and garbage is collected
//! only with the [`BootMenu`] that step ends with.
use std::path::Path;
use std::process::Stdio;

use yukimi_system::SAVED_CONFIGURATIONS;
use yukimi_system::generations::SYSTEM_PROFILE;

use crate::process::{self, Failure, Result, Unstoppable};
use crate::snapshots;

/// The system profile's generations, held for a clean-up: from now until
/// the boot menu is updated, a stop waits.
#[must_use = "holding the generations does nothing until the boot menu is updated"]
pub struct Generations {
    step: Unstoppable,
}

impl Generations {
    /// Holds them, unless a stop was asked for already.
    pub fn hold() -> Result<Generations> {
        Ok(Generations { step: Unstoppable::begin()? })
    }

    /// Deletes the generations older than `days` days (never the current
    /// one), and the configurations kept for them. A deletion that fails
    /// part-way may have deleted some: the boot menu is updated either way.
    pub fn delete_older_than(self, days: u32) -> Deleted {
        process::stage(1, "Deleting old generations");
        let mut delete = process::command("nix-env");
        delete.args(["-p", SYSTEM_PROFILE, "--delete-generations", &format!("{days}d")]);
        let deleting = process::run(delete, "Deleting old generations");
        snapshots::prune(Path::new(SAVED_CONFIGURATIONS), Path::new(SYSTEM_PROFILE));
        process::stage_done(1);
        Deleted { step: Some(self.step), deleting }
    }

    /// Deletes the generations numbered `numbers`, and the configurations
    /// kept for them. A deletion that fails part-way may have deleted some:
    /// the boot menu is updated either way.
    pub fn delete(self, numbers: &[u32]) -> Deleted {
        process::stage(1, &format!("Deleting {}", generation_words(numbers)));
        let mut delete = process::command("nix-env");
        delete.args(["-p", SYSTEM_PROFILE, "--delete-generations"]).args(numbers.iter().map(u32::to_string));
        let deleting = process::run(delete, "Deleting the generation");
        snapshots::prune(Path::new(SAVED_CONFIGURATIONS), Path::new(SYSTEM_PROFILE));
        process::stage_done(1);
        Deleted { step: Some(self.step), deleting }
    }

    /// Updates the boot menu without deleting anything.
    pub fn update_boot_menu(self) -> Result<BootMenu> {
        let menu = update_menu();
        drop(self.step);
        menu.map(|()| BootMenu(()))
    }
}

/// Generations were deleted, and the boot menu may still list them.
#[must_use = "the boot menu lists deleted generations until it is updated"]
pub struct Deleted {
    /// Taken by `update_boot_menu`; until then, dropping this updates the
    /// menu.
    step: Option<Unstoppable>,
    deleting: Result<()>,
}

impl Deleted {
    /// Writes the boot menu for the generations left, which ends the step:
    /// a stop asked for meanwhile applies now.
    pub fn update_boot_menu(mut self) -> Result<BootMenu> {
        let menu = update_menu();
        let deleting = std::mem::replace(&mut self.deleting, Ok(()));
        drop(self.step.take());
        match (deleting, menu) {
            (_, Err(Failure(message))) => Err(Failure(format!(
                "{message}. The boot menu may still list deleted generations: don't collect garbage until a system switch has rewritten it"
            ))),
            (Err(failure), Ok(())) => Err(failure),
            (Ok(()), Ok(())) => Ok(BootMenu(())),
        }
    }
}

/// A clean-up that ends before updating the menu (an early return, a
/// panic) still updates it.
impl Drop for Deleted {
    fn drop(&mut self) {
        if self.step.is_some() {
            let _ = update_menu();
        }
    }
}

/// The boot menu was written after the last deletion: it lists only the
/// generations that exist.
#[derive(Debug)]
pub struct BootMenu(());

/// Collects garbage. Only with a [`BootMenu`]: collecting while the menu
/// still lists deleted generations, as Yukimi once did when the collection
/// was stopped or failed, doesn't compile:
///
/// ```compile_fail,E0308
/// use yukimi_helper::clean::{Generations, collect_garbage};
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let deleted = Generations::hold()?.delete_older_than(7);
/// let freed = collect_garbage(&deleted)?;
/// deleted.update_boot_menu()?;
/// # Ok(()) }
/// ```
///
/// ```no_run
/// use yukimi_helper::clean::{Generations, collect_garbage};
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let menu = Generations::hold()?.delete_older_than(7).update_boot_menu()?;
/// let freed = collect_garbage(&menu)?;
/// # Ok(()) }
/// ```
///
/// Returns Nix's line saying how much was freed.
pub fn collect_garbage(_menu: &BootMenu) -> Result<String> {
    process::stage(3, "Collecting garbage");
    let mut gc = process::command("nix-store");
    gc.arg("--gc");
    let out = process::wait(gc.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        eprintln!("{line}");
    }
    // Nix deletes its reserve (`gc-reserved-space`, 8 MiB in
    // /nix/var/nix/db/reserved) as a collection starts, so it can work on a
    // full disk, and makes it again whenever it next opens the store. Open
    // it now, so that free space measured afterwards is what stays free.
    let mut reopen = process::command("nix-store");
    reopen.args(["--query", "--hash", SYSTEM_PROFILE]);
    let _ = process::wait(reopen.stdout(Stdio::null()).stderr(Stdio::null()));
    if !out.status.success() {
        return Err(Failure("Collecting garbage failed".into()));
    }
    process::stage_done(3);
    Ok(text.lines().rev().find(|l| l.contains("freed")).unwrap_or("").trim().to_owned())
}

/// "generation 3", "generations 3 and 4", "generations 2, 3 and 4".
pub fn generation_words(numbers: &[u32]) -> String {
    let n: Vec<String> = numbers.iter().map(u32::to_string).collect();
    match n.as_slice() {
        [one] => format!("generation {one}"),
        [rest @ .., last] => format!("generations {} and {last}", rest.join(", ")),
        [] => "no generations".into(),
    }
}

fn update_menu() -> Result<()> {
    process::stage(2, "Updating the boot menu");
    let mut menu = process::command(&format!("{SYSTEM_PROFILE}/bin/switch-to-configuration"));
    menu.arg("boot");
    process::run(menu, "Updating the boot menu")?;
    process::stage_done(2);
    Ok(())
}
