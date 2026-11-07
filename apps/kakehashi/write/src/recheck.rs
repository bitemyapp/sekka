//! The helper's re-check of the requested drive, in a module of its own so
//! that the rest of the crate, the platform code included, can't make its
//! proof.

use crate::Requested;
use aizome_typestate::{Confirmed, Reviewed};
use anyhow::{Context, Result, bail};
use kakehashi_core::Drive;

/// The brand of the helper's re-check. Only this module makes one, in
/// [`still_there`], so the platform code can't unmount or open a drive
/// without the check.
#[derive(Debug)]
pub struct Recheck(());

/// The requested drive, found again as it is now.
pub type StillThere = Confirmed<Drive, Recheck>;

/// The drive named in the request, as it is now, if it is still the same
/// one ([`Drive::same`]) and still one that may be written. Unmounting and
/// opening a drive take the proof.
pub fn still_there(plan: &Requested) -> Result<StillThere> {
    // The request carries the drive as the person confirmed it in the window.
    let reviewed = Reviewed::new(plan.drive.clone(), Recheck(()));
    let now = kakehashi_core::platform::drives()?
        .into_iter()
        .find(|d| d.name == plan.drive.name)
        .with_context(|| format!("{} is gone; was it unplugged?", plan.drive.label()))?;
    let Ok(now) = reviewed.confirm(now) else {
        bail!("{} is no longer the drive you picked; nothing was written", plan.drive.name);
    };
    if let Some(why) = now.target().excluded(None, true) {
        bail!("{} may not be written: it is {}", now.target().label(), why.reason());
    }
    Ok(now)
}
