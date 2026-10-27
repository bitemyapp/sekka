//! Koyomi 暦, Aizome's calendar.
//!
//! Calendars are iCalendar files (RFC 5545), one per calendar, in
//! `$XDG_DATA_HOME/aizome/koyomi/` ([`store`]). The library reads and
//! writes them ([`ics`], [`model`]), expands recurring events
//! ([`rrule`], [`occur`]) in the reader's time zone ([`tz`]), changes
//! them ([`edit`]), and answers the shell ([`upcoming`]) and the alert
//! agent ([`agent`]); the window is [`app`] and [`ui`].

pub mod agent;
pub mod app;
pub mod edit;
pub mod format;
pub mod ics;
pub mod instance;
pub mod locale;
pub mod model;
pub mod occur;
pub mod rrule;
pub mod settings;
pub mod store;
pub mod tz;
pub mod ui;
pub mod upcoming;
pub mod vtimezone;
