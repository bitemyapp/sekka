//! The primary selection on the render thread: what was last selected,
//! which a middle click pastes, through zwp_primary_selection_device_v1.
//!
//! It works as the clipboard does in [`super::selection`], sharing its
//! bounded pipe readers and writers: our offer names this process with the
//! owner marker, other clients' text is read ahead once the program has
//! read the primary selection, and reads answer under the same tokens, in
//! the shared state for [`Source::Primary`]. A compositor without the
//! protocol has no primary selection; offers then do nothing and reads
//! come back empty.

use std::sync::Arc;
use std::time::Duration;

use smithay_client_toolkit::data_device_manager::WritePipe;
use smithay_client_toolkit::delegate_primary_selection;
use smithay_client_toolkit::primary_selection::PrimarySelectionManagerState;
use smithay_client_toolkit::primary_selection::device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler};
use smithay_client_toolkit::primary_selection::selection::{PrimarySelectionSource, PrimarySelectionSourceHandler};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1;
use smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1;

use super::State;
use super::selection::{PipeRead, read_pipe, write_pipe};
use crate::clipboard::{Contents, READ_AHEAD_LIMIT, ReadAhead, Source, owner_mime, shared_for, text_mime};

/// As the clipboard's: a read ahead may go this long without data.
const READ_AHEAD_GIVE_UP: Duration = Duration::from_secs(5);

pub struct Primary {
    manager: Option<PrimarySelectionManagerState>,
    devices: Vec<(WlSeat, PrimarySelectionDevice)>,
    source: Option<(PrimarySelectionSource, Arc<Contents>)>,
    /// Contents to offer once there is an input serial to offer them with.
    pending: Option<Option<Arc<Contents>>>,
    /// We cleared it; the compositor's word of that is no news.
    cleared_by_us: bool,
}

impl Primary {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<State>) -> Self {
        Primary {
            manager: PrimarySelectionManagerState::bind(globals, qh).ok(),
            devices: Vec::new(),
            source: None,
            pending: None,
            cleared_by_us: false,
        }
    }
}

pub(super) fn add_seat(state: &mut State, seat: &WlSeat) {
    if let Some(manager) = &state.selection.primary.manager {
        let device = manager.get_selection_device(&state.qh, seat);
        state.selection.primary.devices.push((seat.clone(), device));
    }
}

pub(super) fn remove_seat(state: &mut State, seat: &WlSeat) {
    state.selection.primary.devices.retain(|(s, _)| s != seat);
}

pub(super) fn focus_changed(state: &mut State) {
    if let Some(contents) = state.selection.primary.pending.take() {
        set(state, contents);
    }
}

/// Offer `contents` as the primary selection, or clear it.
pub(super) fn set(state: &mut State, contents: Option<Arc<Contents>>) {
    let Some(manager) = &state.selection.primary.manager else { return };
    let latest = state.seats.latest_serial();
    let device = latest.as_ref().and_then(|(seat, _)| state.selection.primary.devices.iter().find(|(s, _)| s == seat));
    let (Some((_, device)), Some((_, serial))) = (device, latest) else {
        state.selection.primary.pending = Some(contents);
        return;
    };
    match contents {
        Some(contents) => {
            let mimes = contents.items.iter().map(|(m, _)| m.clone()).chain([owner_mime().to_owned()]);
            let source = manager.create_selection_source(&state.qh, mimes);
            source.set_selection(device, serial);
            state.selection.primary.source = Some((source, contents));
        }
        None => {
            device.unset_selection(serial);
            state.selection.primary.source = None;
            state.selection.primary.cleared_by_us = true;
        }
    }
}

/// A pipe receiving the primary selection's `mime`, if another client offers it.
pub(super) fn receive(state: &State, mime: &str) -> Option<smithay_client_toolkit::data_device_manager::ReadPipe> {
    state
        .selection
        .primary
        .devices
        .iter()
        .filter_map(|(_, d)| d.data().selection_offer())
        .find(|o| o.with_mime_types(|m| m.iter().any(|m| m == mime)))
        .and_then(|o| o.receive(mime.to_owned()).ok())
}

impl PrimarySelectionDeviceHandler for State {
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &ZwpPrimarySelectionDeviceV1) {
        let offer = self.selection.primary.devices.iter().find(|(_, d)| d.inner() == device);
        let Some(offer) = offer.map(|(_, d)| d.data().selection_offer()) else { return };
        let shared = shared_for(Source::Primary);
        let Some(offer) = offer else {
            // Cleared: by us, which we know, or by its owner going.
            if !std::mem::take(&mut self.selection.primary.cleared_by_us) {
                shared.foreign_offered(Vec::new(), false);
            }
            return;
        };
        let mimes = offer.with_mime_types(|m| m.to_vec());
        if mimes.iter().any(|m| m == owner_mime()) {
            return;
        }
        let text = text_mime(&mimes);
        let Some(read) = shared.foreign_offered(mimes, text.is_some()) else { return };
        let Some(pipe) = text.and_then(|mime| offer.receive(mime).ok()) else {
            shared.text_arrived(read, ReadAhead::Failed);
            return;
        };
        read_pipe(
            self,
            pipe,
            READ_AHEAD_LIMIT,
            READ_AHEAD_GIVE_UP,
            || {},
            move |outcome| {
                let outcome = match outcome {
                    PipeRead::Done(data) => ReadAhead::Text(Some(String::from_utf8_lossy(&data).into_owned())),
                    PipeRead::TooLong => ReadAhead::TooLong,
                    PipeRead::Failed => ReadAhead::Failed,
                };
                shared_for(Source::Primary).text_arrived(read, outcome);
            },
        );
    }
}

impl PrimarySelectionSourceHandler for State {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &ZwpPrimarySelectionSourceV1,
        mime: String,
        fd: WritePipe,
    ) {
        let data = self
            .selection
            .primary
            .source
            .as_ref()
            .filter(|(s, _)| s.inner() == source)
            .and_then(|(_, c)| c.data(&mime))
            .flatten();
        // The primary selection holds no promised data. Without data the
        // pipe closes here, and the reader gets nothing.
        if let Some(data) = data {
            write_pipe(self, fd, data);
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &ZwpPrimarySelectionSourceV1) {
        if self.selection.primary.source.as_ref().is_some_and(|(s, _)| s.inner() == source) {
            self.selection.primary.source = None;
        }
    }
}

delegate_primary_selection!(State);
