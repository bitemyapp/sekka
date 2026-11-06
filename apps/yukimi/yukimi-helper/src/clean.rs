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
//! only through the [`BootMenu`] that step ends with. The step borrows the
//! process's [`Operation`] until the garbage is collected, so a menu
//! written before a later deletion can't be used after it:
//!
//! ```compile_fail,E0499
//! use yukimi_helper::clean::Generations;
//! use yukimi_helper::process::Operation;
//! # fn main() -> yukimi_helper::process::Result<()> {
//! let mut operation = Operation::begin()?;
//! let menu = Generations::hold(&mut operation)?.update_boot_menu()?;
//! let deleted = Generations::hold(&mut operation)?.delete_older_than(7);
//! menu.collect_garbage()?;
//! # Ok(()) }
//! ```
//!
//! `nix-env` and `nix-store` are named only here and in the change's
//! commit, so they can't run around these steps.
use std::io::Write;
use std::marker::PhantomData;
use std::path::Path;
use std::process::Stdio;

use aizome_typestate::Unsettled;
use yukimi_system::SAVED_CONFIGURATIONS;
use yukimi_system::generations::SYSTEM_PROFILE;

use crate::process::{self, Failure, Operation, Program, Result, Unstoppable};

const NIX_ENV: Program = Program::new("nix-env");
const NIX_STORE: Program = Program::new("nix-store");

/// What a stop waits for, as Yukimi shows it.
const FINISHING: &str = "Deleting the old generations and updating the boot menu";

/// The system profile's generations, held for a clean-up: from now until
/// the boot menu is updated, a stop waits.
#[must_use = "holding the generations does nothing until the boot menu is updated"]
pub struct Generations<'o> {
    step: Unstoppable,
    operation: PhantomData<&'o mut Operation>,
}

impl<'o> Generations<'o> {
    /// Holds them, unless a stop was asked for already.
    pub fn hold(_operation: &'o mut Operation) -> Result<Generations<'o>> {
        Ok(Generations { step: Unstoppable::begin(FINISHING)?, operation: PhantomData })
    }

    /// Deletes the generations older than `days` days (never the current
    /// one), and the configurations kept for them. A deletion that fails
    /// part-way may have deleted some: the boot menu is updated either way,
    /// and from before the deletion starts, an early return or a panic
    /// updates it too.
    pub fn delete_older_than(self, days: u32) -> Deleted<'o> {
        let step = Unsettled::new(self.step, update_menu_dropped as fn(Unstoppable));
        process::stage(1, "Deleting old generations");
        let mut delete = process::command(NIX_ENV);
        delete.args(["-p", SYSTEM_PROFILE, "--delete-generations", &format!("{days}d")]);
        let deleting = process::run(delete, "Deleting old generations");
        yukimi_system::saved::prune(Path::new(SAVED_CONFIGURATIONS), Path::new(SYSTEM_PROFILE));
        process::stage_done(1);
        Deleted { step, deleting, operation: PhantomData }
    }

    /// Deletes the generations numbered `numbers`, and the configurations
    /// kept for them. A deletion that fails part-way may have deleted some:
    /// the boot menu is updated either way.
    pub fn delete(self, numbers: &[u32]) -> Deleted<'o> {
        let step = Unsettled::new(self.step, update_menu_dropped as fn(Unstoppable));
        process::stage(1, &format!("Deleting {}", generation_words(numbers)));
        let mut delete = process::command(NIX_ENV);
        delete.args(["-p", SYSTEM_PROFILE, "--delete-generations"]).args(numbers.iter().map(u32::to_string));
        let deleting = process::run(delete, "Deleting the generation");
        yukimi_system::saved::prune(Path::new(SAVED_CONFIGURATIONS), Path::new(SYSTEM_PROFILE));
        process::stage_done(1);
        Deleted { step, deleting, operation: PhantomData }
    }

    /// Updates the boot menu without deleting anything.
    pub fn update_boot_menu(self) -> Result<BootMenu<'o>> {
        let menu = update_menu();
        drop(self.step);
        match menu {
            Ok(()) => Ok(BootMenu { operation: PhantomData }),
            Err(Failure(message)) => Err(Failure(format!("{message}, so garbage wasn't collected"))),
        }
    }
}

fn update_menu_dropped(step: Unstoppable) {
    let _ = update_menu();
    drop(step);
}

/// Generations were deleted, and the boot menu may still list them.
/// Dropped without [`update_boot_menu`](Self::update_boot_menu), it updates
/// the menu anyway.
#[must_use = "the boot menu lists deleted generations until it is updated"]
pub struct Deleted<'o> {
    step: Unsettled<Unstoppable, fn(Unstoppable)>,
    deleting: Result<()>,
    operation: PhantomData<&'o mut Operation>,
}

impl<'o> Deleted<'o> {
    /// Writes the boot menu for the generations left, which ends the step:
    /// a stop asked for meanwhile applies now.
    pub fn update_boot_menu(self) -> Result<BootMenu<'o>> {
        let menu = update_menu();
        drop(self.step.settle());
        match (self.deleting, menu) {
            (_, Err(Failure(message))) => Err(Failure(format!(
                "{message}. The boot menu may still list deleted generations: don't collect garbage until a system switch has rewritten it"
            ))),
            (Err(failure), Ok(())) => Err(failure),
            (Ok(()), Ok(())) => Ok(BootMenu { operation: PhantomData }),
        }
    }
}

/// The boot menu was written after the last deletion: it lists only the
/// generations that exist. It is spent by collecting garbage, and holds
/// the operation until then.
///
/// Collecting while the menu still lists deleted generations, as Yukimi
/// once did when the collection was stopped or failed, doesn't compile:
///
/// ```compile_fail,E0599
/// use yukimi_helper::clean::Generations;
/// use yukimi_helper::process::Operation;
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let mut operation = Operation::begin()?;
/// let deleted = Generations::hold(&mut operation)?.delete_older_than(7);
/// let freed = deleted.collect_garbage()?;
/// # Ok(()) }
/// ```
///
/// ```no_run
/// use yukimi_helper::clean::Generations;
/// use yukimi_helper::process::Operation;
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let mut operation = Operation::begin()?;
/// let menu = Generations::hold(&mut operation)?.delete_older_than(7).update_boot_menu()?;
/// let freed = menu.collect_garbage()?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
#[must_use = "the menu is what allows collecting garbage"]
pub struct BootMenu<'o> {
    operation: PhantomData<&'o mut Operation>,
}

impl BootMenu<'_> {
    /// Collects garbage. Returns Nix's line saying how much was freed.
    pub fn collect_garbage(self) -> Result<String> {
        process::stage(3, "Collecting garbage");
        let mut gc = process::command(NIX_STORE);
        gc.arg("--gc");
        let out = process::wait(gc.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
        let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
        let mut log = std::io::stderr();
        for line in text.lines() {
            let _ = writeln!(log, "{line}");
        }
        // Nix deletes its reserve (`gc-reserved-space`, 8 MiB in
        // /nix/var/nix/db/reserved) as a collection starts, so it can work
        // on a full disk, and makes it again whenever it next opens the
        // store. Open it now, so that free space measured afterwards is what
        // stays free.
        let mut reopen = process::command(NIX_STORE);
        reopen.args(["--query", "--hash", SYSTEM_PROFILE]);
        let _ = process::wait(reopen.stdout(Stdio::null()).stderr(Stdio::null()));
        if !out.status.success() {
            return Err(Failure("Collecting garbage failed".into()));
        }
        process::stage_done(3);
        Ok(text.lines().rev().find(|l| l.contains("freed")).unwrap_or("").trim().to_owned())
    }
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
    let mut menu = process::prepared(&Path::new(SYSTEM_PROFILE).join("bin/switch-to-configuration"));
    menu.arg("boot");
    process::run(menu, "Updating the boot menu")?;
    process::stage_done(2);
    Ok(())
}
