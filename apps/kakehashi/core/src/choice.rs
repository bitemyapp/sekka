//! The drives the window lists, the one chosen among them, and the proof
//! that a write erases the drive the person confirmed, with the image they
//! confirmed.
//!
//! The window lists the drives every [`REFRESH`], also while the "Erase …?"
//! dialog is open. A drive's name (`sdb`, `disk4`, `PhysicalDrive2`) passes
//! to the next drive plugged in, so the choice is kept as the drive itself
//! and found again by [`Drive::same`] at each listing. Opening the dialog
//! takes a review of the chosen drive and the image ([`DriveList::open_review`]),
//! named by a [`ReviewTicket`] that the dialog's OK hands back. OK finds both
//! again as they are then ([`DriveList::confirm`]), and only that
//! [`ConfirmedWrite`] makes a [`Plan`](crate::write::Plan). The helper
//! checks the plan's drive once more against the drive as it is when it
//! opens it ([`still_there`](crate::write::still_there)).
//!
//! ```
//! use kakehashi_core::Drive;
//! use kakehashi_core::choice::{DriveList, DriveListing, ImageId, Unconfirmed};
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
//! let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
//! let mut list = DriveList::new();
//! let listing = list.request(Instant::now()).unwrap();
//! list.listed(DriveListing::new(listing, Ok(vec![stick("SanDisk", "4C53")])));
//! list.choose(&stick("SanDisk", "4C53"));
//!
//! // The dialog opens: it describes this drive.
//! let (review, seen) = list.open_review(None, image.clone()).unwrap();
//! assert_eq!(seen.drive().vendor, "SanDisk");
//!
//! // Under the dialog, the stick is swapped for another the system names sdb too.
//! let listing = list.request(Instant::now() + DriveList::REFRESH).unwrap();
//! list.listed(DriveListing::new(listing, Ok(vec![stick("Kingston", "E0D5")])));
//! assert!(list.chosen(None).is_none(), "the choice was the SanDisk, not the name");
//!
//! // OK: the drive the dialog named is not there any more, so there is no write.
//! assert!(matches!(list.confirm(review, None, Some(image)), Err(Unconfirmed::Changed(_))));
//! ```

use crate::drive::{Drive, Excluded};
use aizome_typestate::{Answer, Changed, Confirmed, Fresh, Generation, Reviewed, SameTarget, Ticket};
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often the drives are listed, so a stick plugged in appears.
pub const REFRESH: Duration = Duration::from_secs(2);

/// One listing of the drives.
pub enum Listing {}

/// The ticket of one listing: a listing thread takes it and sends it back
/// in its [`DriveListing`].
pub type ListingTicket = Ticket<Listing>;

/// A listing's answer: the drives, or why they couldn't be listed.
#[derive(Debug)]
pub struct DriveListing {
    ticket: ListingTicket,
    result: Result<Vec<Drive>, String>,
}

impl DriveListing {
    pub fn new(ticket: ListingTicket, result: Result<Vec<Drive>, String>) -> Self {
        DriveListing { ticket, result }
    }
}

impl Answer<Listing> for DriveListing {
    fn ticket(&self) -> ListingTicket {
        self.ticket
    }
}

/// The drives as last listed.
#[derive(Debug)]
pub enum Listed {
    /// Not listed yet.
    Pending,
    Ready(Vec<Drive>),
    Failed(String),
}

/// The image a write would use, as the window has it: its file and the
/// checksum the window checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageId {
    pub path: PathBuf,
    pub sha256: String,
}

/// What the confirmation dialog describes: a drive, and the image to write
/// onto it.
#[derive(Debug)]
pub struct Target {
    drive: Drive,
    image: ImageId,
}

impl Target {
    pub fn drive(&self) -> &Drive {
        &self.drive
    }

    pub fn image(&self) -> &ImageId {
        &self.image
    }

    pub(crate) fn into_parts(self) -> (Drive, ImageId) {
        (self.drive, self.image)
    }
}

/// The same drive ([`Drive::same`]) and the same image file and checksum.
impl SameTarget for Target {
    fn same_target(&self, fresh: &Target) -> bool {
        self.drive.same(&fresh.drive) && self.image == fresh.image
    }
}

/// The brand of the confirmation dialog's reviews. Only this module makes
/// one, so only [`DriveList::open_review`] reviews a write.
#[derive(Debug)]
pub struct Sheet(());

/// One opening of the confirmation dialog.
pub enum Review {}

/// Names the dialog that is open; its OK hands it back to
/// [`DriveList::confirm`].
pub type ReviewTicket = Ticket<Review>;

/// Proof that a drive and an image are the ones the open dialog described,
/// found again when its OK was pressed: the drive by identity in the list
/// the dialog was opened from, the image by file and checksum. Only
/// [`DriveList::confirm`] makes one, and [`Plan::new`](crate::write::Plan::new)
/// takes it. It doesn't prove that a person saw the dialog: code can run a
/// list of its own through the same steps. Nothing else makes a review:
///
/// ```compile_fail,E0423
/// use aizome_typestate::Reviewed;
/// use kakehashi_core::choice::{DriveList, ReviewTicket, Sheet, Unconfirmed};
/// fn ok_pressed(list: &mut DriveList, review: ReviewTicket) {
///     if let Err(Unconfirmed::Changed(changed)) = list.confirm(review, None, None) {
///         // Another stick took the name: write that one instead.
///         let _ = Reviewed::new(changed.found, Sheet(()));
///     }
/// }
/// ```
///
/// The open dialog's review, confirmed:
///
/// ```
/// use kakehashi_core::choice::{ConfirmedWrite, DriveList, ImageId, ReviewTicket};
/// fn ok_pressed(list: &mut DriveList, review: ReviewTicket, image: ImageId) -> Option<ConfirmedWrite> {
///     list.confirm(review, None, Some(image)).ok()
/// }
/// ```
pub type ConfirmedWrite = Confirmed<Target, Sheet>;

/// The drive list, refreshed while no write runs, the drive chosen in it,
/// and the review of the dialog that is open.
#[derive(Debug)]
pub struct DriveList {
    listed: Listed,
    listings: Generation<Listing>,
    /// The listing asked for and not answered yet.
    asked: Option<ListingTicket>,
    /// When the last listing was asked for; `None` lists at the next chance.
    last: Option<Instant>,
    show_all: bool,
    /// The drive chosen, as last listed.
    chosen: Option<Drive>,
    reviews: Generation<Review>,
    /// The open dialog's review.
    review: Option<(ReviewTicket, Reviewed<Target, Sheet>)>,
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
            reviews: Generation::new(),
            review: None,
        }
    }

    /// A listing to start now, if one is due: the last one was asked for at
    /// least [`REFRESH`] ago and has been answered. List the drives off the
    /// window's thread and hand the ticket back to [`listed`](Self::listed)
    /// with the result.
    pub fn request(&mut self, now: Instant) -> Option<ListingTicket> {
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

    /// A listing's answer. One for a listing that is no longer the latest
    /// is dropped.
    pub fn listed(&mut self, answer: DriveListing) {
        if let Ok(fresh) = self.listings.check_answer(answer) {
            self.apply(fresh);
        }
    }

    fn apply(&mut self, fresh: Fresh<Listing, DriveListing>) {
        self.asked = None;
        self.listed = match fresh.into_answer().result {
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
    /// stays chosen while each listing finds the same drive. A review in
    /// progress is dropped.
    pub fn choose(&mut self, drive: &Drive) {
        if let Listed::Ready(drives) = &self.listed
            && let Some(listed) = drives.iter().find(|d| d.same(drive))
        {
            self.chosen = Some(listed.clone());
            self.drop_review();
        }
    }

    pub fn clear_choice(&mut self) {
        self.chosen = None;
        self.drop_review();
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

    /// The confirmation dialog opens for the chosen drive and `image`: what
    /// it should describe, and the ticket its OK hands to
    /// [`confirm`](Self::confirm). It replaces the review of any earlier
    /// dialog, whose OK then confirms nothing.
    pub fn open_review(&mut self, holds: Option<&str>, image: ImageId) -> Option<(ReviewTicket, &Target)> {
        let drive = self.chosen(holds)?.clone();
        let ticket = self.reviews.issue();
        let (_, review) = self.review.insert((ticket, Reviewed::new(Target { drive, image }, Sheet(()))));
        Some((ticket, review.target()))
    }

    /// The dialog was cancelled.
    pub fn cancel_review(&mut self, ticket: ReviewTicket) {
        if self.reviews.is_current(ticket) {
            self.drop_review();
        }
    }

    fn drop_review(&mut self) {
        self.review = None;
        self.reviews.retire();
    }

    /// OK in the dialog `ticket` names: the drive and image it described,
    /// found again as they are now. The drive must be the same drive
    /// ([`Drive::same`]: its serial, size, vendor, model, bus and name) in
    /// the latest listing and still one that may be written; `image` is the
    /// window's image now, which must be the one described.
    pub fn confirm(
        &mut self,
        ticket: ReviewTicket,
        holds: Option<&str>,
        image: Option<ImageId>,
    ) -> Result<ConfirmedWrite, Unconfirmed> {
        let Some((_, review)) = self.review.take_if(|(open, _)| *open == ticket && self.reviews.is_current(*open))
        else {
            return Err(Unconfirmed::NoReview);
        };
        self.reviews.retire();
        let seen = review.target();
        let Some(image) = image else { return Err(Unconfirmed::NoImage(Box::new(seen.image.clone()))) };
        let drives = match &self.listed {
            Listed::Ready(drives) => drives,
            Listed::Pending | Listed::Failed(_) => return Err(Unconfirmed::Unlisted(Box::new(seen.drive.clone()))),
        };
        let fresh =
            drives.iter().find(|d| seen.drive.same(d)).or_else(|| drives.iter().find(|d| d.name == seen.drive.name));
        let Some(fresh) = fresh else { return Err(Unconfirmed::Gone(Box::new(seen.drive.clone()))) };
        let confirmed =
            review.confirm(Target { drive: fresh.clone(), image }).map_err(|c| Unconfirmed::Changed(Box::new(c)))?;
        if let Some(why) = confirmed.target().drive.excluded(holds, self.show_all) {
            return Err(Unconfirmed::Excluded(Box::new(confirmed.target().drive.clone()), why));
        }
        Ok(confirmed)
    }
}

/// Why OK in the dialog writes nothing.
#[derive(Debug)]
pub enum Unconfirmed {
    /// The dialog's review is gone: the dialog was cancelled, another one
    /// was opened, or the choice changed.
    NoReview,
    /// The window has no checked image now.
    NoImage(Box<ImageId>),
    /// The drives couldn't be listed since.
    Unlisted(Box<Drive>),
    /// No drive like it is listed now.
    Gone(Box<Drive>),
    /// Another drive has its name now, or the image changed.
    Changed(Box<Changed<Target>>),
    /// It may not be written now.
    Excluded(Box<Drive>, Excluded),
}

impl fmt::Display for Unconfirmed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = |image: &ImageId| {
            image.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "the image".into())
        };
        match self {
            Unconfirmed::NoReview => write!(f, "That confirmation is no longer open; nothing was written."),
            Unconfirmed::NoImage(image) => {
                write!(f, "{} is no longer ready to write; nothing was written.", file(image))
            }
            Unconfirmed::Unlisted(drive) => {
                write!(
                    f,
                    "The drive list couldn't be read, so {} wasn't found again; nothing was written.",
                    drive.label()
                )
            }
            Unconfirmed::Gone(drive) => write!(f, "{} is no longer plugged in; nothing was written.", drive.label()),
            Unconfirmed::Changed(changed) if !changed.reviewed.drive.same(&changed.found.drive) => write!(
                f,
                "{} ({}) was replaced by {} ({}); nothing was written. Choose the drive again.",
                changed.reviewed.drive.label(),
                changed.reviewed.drive.identity_hint(),
                changed.found.drive.label(),
                changed.found.drive.identity_hint()
            ),
            Unconfirmed::Changed(changed) => write!(
                f,
                "The image changed from {} to {} since you confirmed; nothing was written.",
                file(&changed.reviewed.image),
                file(&changed.found.image)
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

    fn image(name: &str) -> ImageId {
        ImageId { path: name.into(), sha256: "0".repeat(64) }
    }

    fn list(list: &mut DriveList, at: Instant, drives: Vec<Drive>) {
        let ticket = list.request(at).expect("a listing is due");
        list.listed(DriveListing::new(ticket, Ok(drives)));
    }

    fn chosen_stick(t0: Instant) -> DriveList {
        let mut l = DriveList::new();
        list(&mut l, t0, vec![stick("sdb", "A")]);
        l.choose(&stick("sdb", "A"));
        l
    }

    #[test]
    fn lists_one_at_a_time_and_every_refresh() {
        let t0 = Instant::now();
        let mut l = DriveList::new();
        let first = l.request(t0).unwrap();
        assert!(l.request(t0 + REFRESH * 3).is_none(), "one listing at a time");
        l.listed(DriveListing::new(first, Ok(vec![stick("sdb", "A")])));
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
        l.listed(DriveListing::new(old, Ok(vec![stick("sdb", "A")])));
        let _new = l.request(t0 + REFRESH).unwrap();
        l.listed(DriveListing::new(old, Ok(vec![])));
        assert!(matches!(l.drives(), Listed::Ready(d) if d.len() == 1));
    }

    #[test]
    fn the_choice_follows_the_drive_not_its_name() {
        let t0 = Instant::now();
        let mut l = chosen_stick(t0);
        assert_eq!(l.chosen(None).map(|d| d.serial.as_str()), Some("A"));
        list(&mut l, t0 + REFRESH, vec![stick("sdb", "B")]);
        assert!(l.chosen(None).is_none());
        assert!(!l.is_chosen(&stick("sdb", "B")));
    }

    #[test]
    fn a_failed_listing_offers_nothing_but_keeps_the_choice() {
        let t0 = Instant::now();
        let mut l = chosen_stick(t0);
        let ticket = l.request(t0 + REFRESH).unwrap();
        l.listed(DriveListing::new(ticket, Err("lsblk failed".into())));
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
        let mut l = chosen_stick(t0);
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        let mut mounted = stick("sdb", "A");
        mounted.mounts = vec!["/run/media/stick".into()];
        list(&mut l, t0 + REFRESH, vec![mounted]);
        let confirmed = l.confirm(review, None, Some(image("sekka.iso"))).unwrap();
        // The proof carries the drive as it is now.
        assert_eq!(confirmed.target().drive().mounts, ["/run/media/stick"]);
        // A review is confirmed once.
        assert!(matches!(l.confirm(review, None, Some(image("sekka.iso"))), Err(Unconfirmed::NoReview)));
    }

    #[test]
    fn confirm_refuses_a_swapped_unplugged_unlisted_or_excluded_drive() {
        let t0 = Instant::now();
        let mut l = chosen_stick(t0);
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        assert!(matches!(
            l.confirm(review, Some("sdb"), Some(image("sekka.iso"))),
            Err(Unconfirmed::Excluded(_, Excluded::HoldsImage))
        ));

        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        list(&mut l, t0 + REFRESH, vec![stick("sdb", "B")]);
        let err = l.confirm(review, None, Some(image("sekka.iso"))).unwrap_err();
        assert!(matches!(&err, Unconfirmed::Changed(c) if c.found.drive().serial == "B"), "{err:?}");
        assert!(err.to_string().contains("was replaced by"), "{err}");

        let mut l = chosen_stick(t0);
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        list(&mut l, t0 + REFRESH, vec![]);
        assert!(matches!(l.confirm(review, None, Some(image("sekka.iso"))), Err(Unconfirmed::Gone(_))));

        let mut l = chosen_stick(t0);
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        let ticket = l.request(t0 + REFRESH).unwrap();
        l.listed(DriveListing::new(ticket, Err("lsblk failed".into())));
        let err = l.confirm(review, None, Some(image("sekka.iso"))).unwrap_err();
        assert!(matches!(err, Unconfirmed::Unlisted(_)) && err.to_string().contains("couldn't be read"), "{err}");
    }

    #[test]
    fn confirm_refuses_a_stick_just_like_it_in_another_port() {
        let t0 = Instant::now();
        let in_port = |port: &str| Drive { serial: String::new(), port: Some(port.into()), ..stick("sdb", "") };
        let mut l = DriveList::new();
        list(&mut l, t0, vec![in_port("2-1")]);
        l.choose(&in_port("2-1"));
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        // Pulled, and one just like it, without a serial either, plugged into
        // another port, where it took the name sdb.
        list(&mut l, t0 + REFRESH, vec![in_port("2-2")]);
        assert!(l.chosen(None).is_none());
        let err = l.confirm(review, None, Some(image("sekka.iso"))).unwrap_err();
        assert!(matches!(err, Unconfirmed::Changed(_)), "{err:?}");
        assert!(err.to_string().contains("(no serial, USB port 2-1) was replaced by"), "{err}");
    }

    #[test]
    fn confirm_refuses_another_image() {
        let t0 = Instant::now();
        let mut l = chosen_stick(t0);
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        // A download finished under the dialog and became the image.
        let err = l.confirm(review, None, Some(image("nightly.iso"))).unwrap_err();
        assert!(err.to_string().contains("The image changed from sekka.iso to nightly.iso"), "{err}");
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        assert!(matches!(l.confirm(review, None, None), Err(Unconfirmed::NoImage(_))));
    }

    #[test]
    fn only_the_open_dialog_confirms() {
        let t0 = Instant::now();
        let mut l = chosen_stick(t0);
        // A dialog cancelled: its ticket confirms nothing later.
        let (cancelled, _) = l.open_review(None, image("sekka.iso")).unwrap();
        l.cancel_review(cancelled);
        assert!(matches!(l.confirm(cancelled, None, Some(image("sekka.iso"))), Err(Unconfirmed::NoReview)));
        // A dialog dismissed without a cancel, then another opened: only the second confirms.
        let (first, _) = l.open_review(None, image("sekka.iso")).unwrap();
        let (second, _) = l.open_review(None, image("sekka.iso")).unwrap();
        assert!(matches!(l.confirm(first, None, Some(image("sekka.iso"))), Err(Unconfirmed::NoReview)));
        assert!(l.confirm(second, None, Some(image("sekka.iso"))).is_ok());
        // Choosing again drops the review.
        let (review, _) = l.open_review(None, image("sekka.iso")).unwrap();
        l.choose(&stick("sdb", "A"));
        assert!(matches!(l.confirm(review, None, Some(image("sekka.iso"))), Err(Unconfirmed::NoReview)));
    }
}
