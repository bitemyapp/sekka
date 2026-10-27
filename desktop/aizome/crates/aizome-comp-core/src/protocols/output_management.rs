//! wlr-output-management-unstable-v1 (version 4), so wlr-randr, kanshi
//! and display settings tools can see and change outputs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_protocols_wlr::output_management::v1::server::{
    zwlr_output_configuration_head_v1 as conf_head, zwlr_output_configuration_v1 as conf, zwlr_output_head_v1 as head,
    zwlr_output_manager_v1 as manager, zwlr_output_mode_v1 as mode,
};
use smithay::reexports::wayland_server::protocol::wl_output::Transform as WlTransform;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};
use smithay::utils::Transform;
use tracing::warn;

use crate::outputs::OutputChange;
use crate::state::{Aizome, State};

use conf::ZwlrOutputConfigurationV1 as Configuration;
use conf_head::ZwlrOutputConfigurationHeadV1 as ConfigurationHead;
use head::ZwlrOutputHeadV1 as Head;
use manager::ZwlrOutputManagerV1 as Manager;
use mode::ZwlrOutputModeV1 as Mode;

pub struct OutputManagementState {
    instances: Vec<Instance>,
    serial: u32,
}

struct Instance {
    manager: Manager,
    /// Per output name: its head and its modes.
    heads: HashMap<String, (Head, Vec<(Mode, smithay::output::Mode)>)>,
}

pub struct GlobalData {
    filter: Box<dyn Fn(&Client) -> bool + Send + Sync>,
}

pub struct HeadData {
    name: String,
}

pub struct ModeData {
    mode: smithay::output::Mode,
}

pub struct ConfigData {
    serial: u32,
    inner: Arc<Mutex<ConfigInner>>,
}

#[derive(Default)]
struct ConfigInner {
    used: bool,
    heads: Vec<(String, Option<Arc<Mutex<HeadConfig>>>)>,
}

#[derive(Default, Clone)]
struct HeadConfig {
    mode: Option<(i32, i32, Option<i32>)>,
    position: Option<(i32, i32)>,
    transform: Option<Transform>,
    scale: Option<f64>,
}

pub struct ConfigHeadData {
    inner: Arc<Mutex<HeadConfig>>,
}

impl OutputManagementState {
    pub fn new<F>(dh: &DisplayHandle, filter: F) -> Self
    where
        F: Fn(&Client) -> bool + Send + Sync + 'static,
    {
        dh.create_global::<State, Manager, _>(4, GlobalData { filter: Box::new(filter) });
        OutputManagementState { instances: Vec::new(), serial: 1 }
    }
}

fn transform_to_wl(t: Transform) -> WlTransform {
    match t {
        Transform::Normal => WlTransform::Normal,
        Transform::_90 => WlTransform::_90,
        Transform::_180 => WlTransform::_180,
        Transform::_270 => WlTransform::_270,
        Transform::Flipped => WlTransform::Flipped,
        Transform::Flipped90 => WlTransform::Flipped90,
        Transform::Flipped180 => WlTransform::Flipped180,
        Transform::Flipped270 => WlTransform::Flipped270,
    }
}

fn transform_from_wl(t: WlTransform) -> Transform {
    match t {
        WlTransform::_90 => Transform::_90,
        WlTransform::_180 => Transform::_180,
        WlTransform::_270 => Transform::_270,
        WlTransform::Flipped => Transform::Flipped,
        WlTransform::Flipped90 => Transform::Flipped90,
        WlTransform::Flipped180 => Transform::Flipped180,
        WlTransform::Flipped270 => Transform::Flipped270,
        _ => Transform::Normal,
    }
}

/// Creates a head (with its modes) for an output on one manager.
fn new_head(az: &Aizome, inst: &mut Instance, output: &smithay::output::Output) {
    let dh = &az.dh;
    let Ok(client) = dh.get_client(inst.manager.id()) else { return };
    let v = inst.manager.version();
    let Ok(h) = client.create_resource::<Head, _, State>(dh, v, HeadData { name: output.name() }) else { return };
    inst.manager.head(&h);
    let props = output.physical_properties();
    h.name(output.name());
    h.description(format!("{} {} ({})", props.make, props.model, output.name()));
    if props.size.w > 0 {
        h.physical_size(props.size.w, props.size.h);
    }
    let mut modes = Vec::new();
    for m in output.modes() {
        let Ok(mr) = client.create_resource::<Mode, _, State>(dh, h.version().min(3), ModeData { mode: m }) else {
            continue;
        };
        h.mode(&mr);
        mr.size(m.size.w, m.size.h);
        mr.refresh(m.refresh);
        if output.preferred_mode() == Some(m) {
            mr.preferred();
        }
        modes.push((mr, m));
    }
    if h.version() >= 2 {
        h.make(props.make.clone());
        h.model(props.model.clone());
        let serial = output.user_data().get::<crate::outputs::OutputSerial>().map(|s| s.0.clone()).unwrap_or_default();
        h.serial_number(serial);
    }
    inst.heads.insert(output.name(), (h.clone(), modes));
    send_state(az, inst, output);
}

/// The head's changing state: enabled, mode, position, transform, scale.
fn send_state(az: &Aizome, inst: &Instance, output: &smithay::output::Output) {
    let Some((h, modes)) = inst.heads.get(&output.name()) else { return };
    if az.disabled_outputs.contains(output) {
        h.enabled(0);
        return;
    }
    h.enabled(1);
    if let Some(cur) = output.current_mode()
        && let Some((mr, _)) = modes.iter().find(|(_, m)| *m == cur)
    {
        h.current_mode(mr);
    }
    // A mirror sits where its source does.
    let r = az.output_rect(&az.mirror_source(output).unwrap_or_else(|| output.clone()));
    h.position(r.x, r.y);
    h.transform(transform_to_wl(output.current_transform()));
    h.scale(output.current_scale().fractional_scale());
}

/// The outputs changed: tell every manager, then `done`.
pub fn outputs_changed(az: &mut Aizome) {
    let mut st =
        std::mem::replace(&mut az.output_management, OutputManagementState { instances: Vec::new(), serial: 0 });
    st.serial = st.serial.wrapping_add(1);
    st.instances.retain(|i| i.manager.is_alive());
    let outputs = az.known_outputs();
    for inst in &mut st.instances {
        // Gone.
        let gone: Vec<String> =
            inst.heads.keys().filter(|n| !outputs.iter().any(|o| &o.name() == *n)).cloned().collect();
        for n in gone {
            if let Some((h, modes)) = inst.heads.remove(&n) {
                for (m, _) in modes {
                    m.finished();
                }
                h.finished();
            }
        }
        for o in &outputs {
            if inst.heads.contains_key(&o.name()) {
                send_state(az, inst, o);
            } else {
                new_head(az, inst, o);
            }
        }
        inst.manager.done(st.serial);
    }
    az.output_management = st;
}

impl GlobalDispatch<Manager, GlobalData> for State {
    fn bind(
        state: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<Manager>,
        _: &GlobalData,
        init: &mut DataInit<'_, State>,
    ) {
        let manager = init.init(resource, ());
        let mut inst = Instance { manager: manager.clone(), heads: HashMap::new() };
        for o in state.az.known_outputs() {
            new_head(&state.az, &mut inst, &o);
        }
        manager.done(state.az.output_management.serial);
        state.az.output_management.instances.push(inst);
    }

    fn can_view(client: Client, data: &GlobalData) -> bool {
        (data.filter)(&client)
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        m: &Manager,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        match request {
            manager::Request::CreateConfiguration { id, serial } => {
                init.init(id, ConfigData { serial, inner: Arc::new(Mutex::new(ConfigInner::default())) });
            }
            manager::Request::Stop => {
                m.finished();
                state.az.output_management.instances.retain(|i| &i.manager != m);
            }
            _ => {}
        }
    }
}

impl Dispatch<Head, HeadData> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &Head,
        _: head::Request,
        _: &HeadData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<Mode, ModeData> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &Mode,
        _: mode::Request,
        _: &ModeData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<Configuration, ConfigData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        c: &Configuration,
        request: conf::Request,
        data: &ConfigData,
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        let mut inner = data.inner.lock().unwrap();
        match request {
            conf::Request::EnableHead { id, head } => {
                let name = head.data::<HeadData>().map(|d| d.name.clone()).unwrap_or_default();
                if inner.heads.iter().any(|(n, _)| *n == name) {
                    c.post_error(conf::Error::AlreadyConfiguredHead, "head configured twice");
                    return;
                }
                let hc = Arc::new(Mutex::new(HeadConfig::default()));
                init.init(id, ConfigHeadData { inner: hc.clone() });
                inner.heads.push((name, Some(hc)));
            }
            conf::Request::DisableHead { head } => {
                let name = head.data::<HeadData>().map(|d| d.name.clone()).unwrap_or_default();
                if inner.heads.iter().any(|(n, _)| *n == name) {
                    c.post_error(conf::Error::AlreadyConfiguredHead, "head configured twice");
                    return;
                }
                inner.heads.push((name, None));
            }
            conf::Request::Apply | conf::Request::Test => {
                let test = matches!(request, conf::Request::Test);
                if inner.used {
                    c.post_error(conf::Error::AlreadyUsed, "configuration already used");
                    return;
                }
                inner.used = true;
                if data.serial != state.az.output_management.serial {
                    c.cancelled();
                    return;
                }
                let changes: Vec<OutputChange> = inner
                    .heads
                    .iter()
                    .map(|(name, hc)| match hc {
                        Some(hc) => {
                            let hc = hc.lock().unwrap().clone();
                            OutputChange {
                                name: name.clone(),
                                mode: hc.mode,
                                scale: hc.scale,
                                position: hc.position,
                                transform: hc.transform,
                                enabled: Some(true),
                            }
                        }
                        None => OutputChange { name: name.clone(), enabled: Some(false), ..Default::default() },
                    })
                    .collect();
                drop(inner);
                let ok = changes.iter().all(|ch| {
                    ch.scale.is_none_or(|s| (0.5..=4.0).contains(&s)) && state.az.known_output(&ch.name).is_some()
                });
                if !ok || test {
                    if ok {
                        c.succeeded()
                    } else {
                        c.failed()
                    }
                    return;
                }
                let mut failed = false;
                for ch in &changes {
                    if let Err(e) = state.configure_output(ch) {
                        warn!("output management: {e}");
                        failed = true;
                    }
                }
                if failed { c.failed() } else { c.succeeded() }
            }
            _ => {}
        }
    }
}

impl Dispatch<ConfigurationHead, ConfigHeadData> for State {
    fn request(
        _: &mut State,
        _: &Client,
        ch: &ConfigurationHead,
        request: conf_head::Request,
        data: &ConfigHeadData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let mut hc = data.inner.lock().unwrap();
        match request {
            conf_head::Request::SetMode { mode } => {
                if let Some(m) = mode.data::<ModeData>() {
                    hc.mode = Some((m.mode.size.w, m.mode.size.h, Some(m.mode.refresh)));
                }
            }
            conf_head::Request::SetCustomMode { width, height, refresh } => {
                if width <= 0 || height <= 0 {
                    ch.post_error(conf_head::Error::InvalidCustomMode, "bad size");
                    return;
                }
                hc.mode = Some((width, height, (refresh > 0).then_some(refresh)));
            }
            conf_head::Request::SetPosition { x, y } => hc.position = Some((x, y)),
            conf_head::Request::SetTransform { transform: WEnum::Value(t) } => {
                hc.transform = Some(transform_from_wl(t))
            }
            conf_head::Request::SetScale { scale } => {
                if scale <= 0.0 {
                    ch.post_error(conf_head::Error::InvalidScale, "bad scale");
                    return;
                }
                hc.scale = Some(scale);
            }
            _ => {}
        }
    }
}
