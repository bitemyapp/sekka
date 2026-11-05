//! The drives the window lists, the one chosen among them, and the proof
//! that a write erases the drive the person confirmed.
//!
//! The window lists the drives every [`REFRESH`], also while the "Erase …?"
//! dialog is open. A drive's name (`sdb`, `disk4`, `PhysicalDrive2`) passes
//! to the next drive plugged in, so the choice is kept as the drive itself
//! and found again by [`Drive::same`] at each listing. A write starts only
//! from the drive the dialog described, a [`ReviewedDrive`], found again in
//! the list as it is when the person agrees, a [`ConfirmedDrive`], which is
//! the only way to make a [`Plan`](crate::write::Plan). The helper checks
//! the plan's drive once more against the drive as it is when it opens it
//! ([`still_there`](crate::write::still_there)).
//!
//! ```
//! use kakehashi_core::Drive;
//! use kakehashi_core::choice::{DriveList, Unconfirmed};
//! use std::time::Instant;
//!
//! let stick = |vendor: &str, serial: &str| Drive {
//!     name: "sdb".into(),
//!     bytes: 32_000_000_000,
//!     vendor: vendor.into(),
//!     serial: serial.into(),
//!     usb_or_removable: true,
//!     ..Drive::default()
//! };
//! let mut list = DriveList::new();
//! let listing = list.request(Instant::now()).unwrap();
//! list.listed(listing, Ok(vec![stick("SanDisk", "4C53")]));
//! list.choose(&stick("SanDisk", "4C53"));
//!
//! // The dialog opens: it describes this drive.
//! let reviewed = list.review(None).unwrap();
//! assert_eq!(reviewed.drive().vendor, "SanDisk");
//!
//! // Under the dialog, the stick is swapped for another the system names sdb too.
//! let listing = list.request(Instant::now() + DriveList::REFRESH).unwrap();
//! list.listed(listing, Ok(vec![stick("Kingston", "E0D5")]));
//! assert!(list.chosen(None).is_none(), "the choice was the SanDisk, not the name");
//!
//! // OK: the drive the dialog named is not there any more, so there is no write.
//! assert!(matches!(list.confirm(reviewed, None), Err(Unconfirmed::Changed(_))));
//! ```

use crate::drive::{Drive, Excluded};
use aizome_typestate::{Changed, Confirmed, Current, Generation, Reviewed, Ticket};
use std::fmt;
use std::time::{Duration, Instant};

/// How often the drives are listed, so a stick plugged in appears.
pub const REFRESH: Duration = Duration::from_secs(2);

/// One listing of the drives: a listing thread takes its [`Ticket`] and
/// sends it back with the result.
pub enum Listing {}

/// The ticket of one listing.
pub type ListingTicket = Ticket<Listing>;

/// The drives as last listed.
#[derive(Debug)]
pub enum Listed {
    /// Not listed yet.
    Pending,
    Ready(Vec<Drive>),
    Failed(String),
}

/// The drive list, refreshed while no write runs, and the drive chosen in
/// it.
#[derive(Debug)]
pub struct DriveList {
    listed: Listed,
    listings: Generation<Listing>,
    /// The listing asked for and not answered yet.
    asked: Option<Ticket<Listing>>,
    /// When the last listing was asked for; `None` lists at the next chance.
    last: Option<Instant>,
    show_all: bool,
    /// The drive chosen, as last listed.
    chosen: Option<Drive>,
}

impl Default for DriveList {
    fn default() -> Self {
        Self::new()
    }
}

impl DriveList {
    pub const REFRESH: Duration = REFRESH;

    /// Nothing listed yet; the first [`request`](Self::request) is due at once.
    pub fn new() -> Self {
        DriveList {
            listed: Listed::Pending,
            listings: Generation::new(),
            asked: None,
            last: None,
            show_all: false,
            chosen: None,
        }
    }

    /// A listing to start now, if one is due: the last one was asked for at
    /// least [`REFRESH`] ago and has been answered. List the drives off the
    /// window's thread and hand the ticket back to [`listed`](Self::listed)
    /// with the result.
    pub fn request(&mut self, now: Instant) -> Option<Ticket<Listing>> {
        if self.asked.is_some() || self.last.is_some_and(|at| now.saturating_duration_since(at) < REFRESH) {
            return None;
        }
        let ticket = self.listings.issue();
        self.asked = Some(ticket);
        self.last = Some(now);
        Some(ticket)
    }

    /// Lists again at the next chance (a write ended).
    pub fn refresh_soon(&mut self) {
        self.last = None;
    }

    /// A listing's result. One for a listing that is no longer the latest
    /// is dropped.
    pub fn listed(&mut self, ticket: Ticket<Listing>, result: Result<Vec<Drive>, String>) {
        if let Some(current) = self.listings.check(ticket) {
            self.apply(current, result);
        }
    }

    fn apply(&mut self, _current: Current<Listing>, result: Result<Vec<Drive>, String>) {
        self.asked = None;
        self.listed = match result {
            Ok(drives) => Listed::Ready(drives),
            Err(e) => Listed::Failed(e),
        };
        // The choice follows the drive, not its name: a drive replaced by
        // another under the same name is no longer chosen.
        if let Listed::Ready(drives) = &self.listed {
            self.chosen = self.chosen.take().and_then(|chosen| drives.iter().find(|d| chosen.same(d)).cloned());
        }
    }

    /// The drives as last listed.
    pub fn drives(&self) -> &Listed {
        &self.listed
    }

    /// Whether external drives that are not USB or removable, or are
    /// large, are offered too.
    pub fn show_all(&self) -> bool {
        self.show_all
    }

    pub fn set_show_all(&mut self, all: bool) {
        self.show_all = all;
    }

    /// The drives offered, and those that are not with why. `holds` names
    /// the drive the image is on.
    pub fn sorted(&self, holds: Option<&str>) -> (Vec<&Drive>, Vec<(&Drive, Excluded)>) {
        let Listed::Ready(drives) = &self.listed else { return (Vec::new(), Vec::new()) };
        let mut offered = Vec::new();
        let mut not = Vec::new();
        for drive in drives {
            match drive.excluded(holds, self.show_all) {
                None => offered.push(drive),
                Some(why) => not.push((drive, why)),
            }
        }
        (offered, not)
    }

    /// Chooses a drive the window showed, if the list still has it. It
    /// stays chosen while each listing finds the same drive.
    pub fn choose(&mut self, drive: &Drive) {
        if let Listed::Ready(drives) = &self.listed
            && let Some(listed) = drives.iter().find(|d| d.same(drive))
        {
            self.chosen = Some(listed.clone());
        }
    }

    pub fn clear_choice(&mut self) {
        self.chosen = None;
    }

    /// The chosen drive, if the latest listing has it and it is offered.
    pub fn chosen(&self, holds: Option<&str>) -> Option<&Drive> {
        if !matches!(self.listed, Listed::Ready(_)) {
            return None;
        }
        self.chosen.as_ref().filter(|d| d.excluded(holds, self.show_all).is_none())
    }

    /// Whether `drive`, a row of the list, is the chosen one.
    pub fn is_chosen(&self, drive: &Drive) -> bool {
        self.chosen.as_ref().is_some_and(|chosen| chosen.same(drive))
    }

    /// The chosen drive, as the confirmation dialog describes it. Make the
    /// dialog's text from [`ReviewedDrive::drive`], and hand this to
    /// [`confirm`](Self::confirm) if the person agrees.
    pub fn review(&self, holds: Option<&str>) -> Option<ReviewedDrive> {
        self.chosen(holds).map(|drive| ReviewedDrive(Reviewed::new(drive.clone())))
    }

    /// The drive the person reviewed, found again in the list as it is now:
    /// the same drive ([`Drive::same`]: its serial, size, vendor, model, bus
    /// and name), and still one that may be written.
    pub fn confirm(&self, reviewed: ReviewedDrive, holds: Option<&str>) -> Result<ConfirmedDrive, Unconfirmed> {
        let seen = reviewed.0.target();
        let Listed::Ready(drives) = &self.listed else { return Err(Unconfirmed::Gone(Box::new(seen.clone()))) };
        let fresh = drives.iter().find(|d| seen.same(d)).or_else(|| drives.iter().find(|d| d.name == seen.name));
        let Some(fresh) = fresh else { return Err(Unconfirmed::Gone(Box::new(seen.clone()))) };
        let confirmed = reviewed.0.confirm(fresh.clone()).map_err(|c| Unconfirmed::Changed(Box::new(c)))?;
        if let Some(why) = confirmed.target().excluded(holds, self.show_all) {
            return Err(Unconfirmed::Excluded(Box::new(confirmed.target().clone()), why));
        }
        Ok(ConfirmedDrive(confirmed))
    }
}

/// The chosen drive as the confirmation dialog describes it, taken when it
/// opens. Only [`DriveList::review`] makes one.
#[derive(Debug)]
pub struct ReviewedDrive(Reviewed<Drive>);

impl ReviewedDrive {
    /// The drive as the person saw it.
    pub fn drive(&self) -> &Drive {
        self.0.target()
    }
}

/// Proof that a drive is the one the person confirmed, found again when
/// they agreed. Only [`DriveList::confirm`] makes one, and
/// [`Plan::new`](crate::write::Plan::new) takes it, so a plan can't name a
/// drive picked any other way:
///
/// ```compile_fail,E0423
/// use aizome_typestate::Reviewed;
/// use kakehashi_core::{Drive, choice::ConfirmedDrive};
/// let chosen_now = Drive { name: "sdb".into(), ..Drive::default() };
/// let forged = Reviewed::new(chosen_now.clone()).confirm(chosen_now).unwrap();
/// let _ = ConfirmedDrive(forged);
/// ```
#[derive(Debug)]
#[must_use = "a confirmation is spent by the plan it allows"]
pub struct ConfirmedDrive(Confirmed<Drive>);

impl ConfirmedDrive {
    /// The drive as it was when the person agreed.
    pub fn drive(&self) -> &Drive {
        self.0.target()
    }
}

/// Why the drive the person reviewed is not written.
#[derive(Debug)]
pub enum Unconfirmed {
    /// No drive like it is listed now.
    Gone(Box<Drive>),
    /// Another drive has its name now.
    Changed(Box<Changed<Drive>>),
    /// It may not be written now.
    Excluded(Box<Drive>, Excluded),
}

impl fmt::Display for Unconfirmed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unconfirmed::Gone(drive) => write!(f, "{} is no longer plugged in; nothing was written.", drive.label()),
            Unconfirmed::Changed(changed) => write!(
                f,
                "{} ({}) was replaced by {} ({}); nothing was written. Choose the drive again.",
                changed.reviewed.label(),
                changed.reviewed.serial_hint(),
                changed.found.label(),
                changed.found.serial_hint()
            ),
            Unconfirmed::Excluded(drive, why) => {
                write!(f, "{} may not be written now: it is {}; nothing was written.", drive.label(), why.reason())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stick(name: &str, serial: &str) -> Drive {
        Drive {
            name: name.into(),
            bytes: 32_000_000_000,
            vendor: "SanDisk".into(),
            model: "Ultra".into(),
            serial: serial.into(),
            bus: "USB".into(),
            usb_or_removable: true,
            ..Drive::default()
        }
    }

    fn list(list: &mut DriveList, at: Instant, drives: Vec<Drive>) {
        let ticket = list.request(at).expect("a listing is due");
        list.listed(ticket, Ok(drives));
    }

    #[test]
    fn lists_one_at_a_time_and_every_refresh() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        let first = l.request(t0).unwrap();
        assert!(l.request(t0 + REFRESH * 3).is_none(), "one listing at a time");
        l.listed(first, Ok(vec![stick("sdb", "A")]));
        assert!(l.request(t0 + REFRESH / 2).is_none(), "not before the refresh");
        assert!(l.request(t0 + REFRESH).is_some());
    }

    #[test]
    fn refresh_soon_lists_at_the_next_chance() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        list(&mut l, t0, vec![]);
        l.refresh_soon();
        assert!(l.request(t0).is_some());
    }

    #[test]
    fn a_superseded_listing_is_dropped() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        let old = l.request(t0).unwrap();
        // A second listing is only issued once the first is answered; a
        // ticket kept from an earlier listing is stale.
        l.listed(old, Ok(vec![stick("sdb", "A")]));
        let _new = l.request(t0 + REFRESH).unwrap();
        l.listed(old, Ok(vec![]));
        assert!(matches!(l.drives(), Listed::Ready(d) if d.len() == 1));
    }

    #[test]
    fn the_choice_follows_the_drive_not_its_name() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        list(&mut l, t0, vec![stick("sdb", "A")]);
        l.choose(&stick("sdb", "A"));
        assert_eq!(l.chosen(None).map(|d| d.serial.as_str()), Some("A"));
        list(&mut l, t0 + REFRESH, vec![stick("sdb", "B")]);
        assert!(l.chosen(None).is_none());
        assert!(!l.is_chosen(&stick("sdb", "B")));
    }

    #[test]
    fn a_failed_listing_offers_nothing_but_keeps_the_choice() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        list(&mut l, t0, vec![stick("sdb", "A")]);
        l.choose(&stick("sdb", "A"));
        let ticket = l.request(t0 + REFRESH).unwrap();
        l.listed(ticket, Err("lsblk failed".into()));
        assert!(l.chosen(None).is_none());
        list(&mut l, t0 + REFRESH * 2, vec![stick("sdb", "A")]);
        assert!(l.chosen(None).is_some());
    }

    #[test]
    fn choosing_a_drive_no_longer_listed_does_nothing() {
        let mut l = DriveList::new();
        list(&mut l, Instant::now(), vec![stick("sdb", "B")]);
        l.choose(&stick("sdb", "A"));
        assert!(l.chosen(None).is_none());
    }

    #[test]
    fn confirm_finds_the_reviewed_drive_again() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        list(&mut l, t0, vec![stick("sdb", "A")]);
        l.choose(&stick("sdb", "A"));
        let reviewed = l.review(None).unwrap();
        let mut mounted = stick("sdb", "A");
        mounted.mounts = vec!["/run/media/stick".into()];
        list(&mut l, t0 + REFRESH, vec![mounted]);
        let confirmed = l.confirm(reviewed, None).unwrap();
        // The proof carries the drive as it is now.
        assert_eq!(confirmed.drive().mounts, ["/run/media/stick"]);
    }

    #[test]
    fn confirm_refuses_a_swapped_unplugged_or_excluded_drive() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        list(&mut l, t0, vec![stick("sdb", "A")]);
        l.choose(&stick("sdb", "A"));
        let swapped = l.review(None).unwrap();
        let unplugged = l.review(None).unwrap();
        let holds = l.review(None).unwrap();
        let in_list = l.review(None).unwrap();

        assert!(matches!(l.confirm(holds, Some("sdb")), Err(Unconfirmed::Excluded(_, Excluded::HoldsImage))));
        list(&mut l, t0 + REFRESH, vec![stick("sdb", "B")]);
        let err = l.confirm(swapped, None).unwrap_err();
        assert!(matches!(&err, Unconfirmed::Changed(c) if c.found.serial == "B"), "{err:?}");
        assert!(err.to_string().contains("nothing was written"), "{err}");
        list(&mut l, t0 + REFRESH * 2, vec![]);
        assert!(matches!(l.confirm(unplugged, None), Err(Unconfirmed::Gone(_))));
        let ticket = l.request(t0 + REFRESH * 3).unwrap();
        l.listed(ticket, Err("lsblk failed".into()));
        assert!(matches!(l.confirm(in_list, None), Err(Unconfirmed::Gone(_))));
    }
}
