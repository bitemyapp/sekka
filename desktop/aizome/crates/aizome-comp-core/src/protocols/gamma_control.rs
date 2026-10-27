//! wlr-gamma-control-unstable-v1: a privileged client (gammastep, wlsunset)
//! sets an output's gamma ramps. Only outputs whose CRTC takes a gamma LUT
//! (udev) offer it; others answer `failed`. While a client holds an
//! output's ramps Night Light leaves that output alone, and when the
//! client goes Night Light's ramps come back.

use std::io::Read;
use std::os::fd::OwnedFd;

use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr::gamma_control::v1::server::{
    zwlr_gamma_control_manager_v1, zwlr_gamma_control_v1,
};
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New};
use tracing::debug;

use crate::state::State;

use zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1 as Manager;
use zwlr_gamma_control_v1::ZwlrGammaControlV1 as Control;

pub struct GlobalData {
    filter: Box<dyn Fn(&Client) -> bool + Send + Sync>,
}

/// One control: the output it holds, if it got it.
pub struct ControlData {
    output: Option<Output>,
}

pub fn create_global(dh: &DisplayHandle, filter: impl Fn(&Client) -> bool + Send + Sync + 'static) {
    dh.create_global::<State, Manager, _>(1, GlobalData { filter: Box::new(filter) });
}

impl GlobalDispatch<Manager, GlobalData> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<Manager>,
        _: &GlobalData,
        init: &mut DataInit<'_, State>,
    ) {
        init.init(resource, ());
    }

    fn can_view(client: Client, data: &GlobalData) -> bool {
        (data.filter)(&client)
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &Manager,
        request: zwlr_gamma_control_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        if let zwlr_gamma_control_manager_v1::Request::GetGammaControl { id, output } = request {
            let output = Output::from_resource(&output);
            let size = output.as_ref().map(|o| state.backend.gamma_size(o)).unwrap_or(0);
            let taken = output.as_ref().is_some_and(|o| state.az.out.get(o).is_some_and(|s| s.gamma_client));
            if size == 0 || taken {
                let c = init.init(id, ControlData { output: None });
                c.failed();
                return;
            }
            let output = output.expect("an output");
            if let Some(s) = state.az.out.get_mut(&output) {
                s.gamma_client = true;
            }
            let c = init.init(id, ControlData { output: Some(output) });
            c.gamma_size(size as u32);
        }
    }
}

impl Dispatch<Control, ControlData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        control: &Control,
        request: zwlr_gamma_control_v1::Request,
        data: &ControlData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        match request {
            zwlr_gamma_control_v1::Request::SetGamma { fd } => {
                let Some(output) = &data.output else { return };
                let size = state.backend.gamma_size(output);
                match read_ramps(fd, size).and_then(|(r, g, b)| state.backend.set_gamma(output, &r, &g, &b)) {
                    Ok(()) => {}
                    Err(e) => {
                        debug!("gamma control on {}: {e}", output.name());
                        control.failed();
                        release(state, output);
                    }
                }
            }
            zwlr_gamma_control_v1::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut State,
        _: smithay::reexports::wayland_server::backend::ClientId,
        _: &Control,
        data: &ControlData,
    ) {
        if let Some(output) = &data.output {
            release(state, output);
        }
    }
}

/// The client let go: Night Light's ramps again.
fn release(state: &mut State, output: &Output) {
    let held = state.az.out.get_mut(output).map(|s| std::mem::replace(&mut s.gamma_client, false)).unwrap_or(false);
    if held {
        state.update_night_light(true);
    }
}

/// Red, green and blue ramps, 16-bit entries each.
type Ramps = (Vec<u16>, Vec<u16>, Vec<u16>);

/// Red, green and blue ramps of `size` 16-bit entries each, from the
/// client's file.
fn read_ramps(fd: OwnedFd, size: usize) -> Result<Ramps, String> {
    let mut f = std::fs::File::from(fd);
    let mut bytes = vec![0u8; size * 3 * 2];
    f.read_exact(&mut bytes).map_err(|e| format!("reading the ramps: {e}"))?;
    let all: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_ne_bytes([c[0], c[1]])).collect();
    Ok((all[..size].to_vec(), all[size..2 * size].to_vec(), all[2 * size..].to_vec()))
}
