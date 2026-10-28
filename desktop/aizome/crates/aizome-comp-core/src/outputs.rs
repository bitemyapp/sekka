//! Outputs in the global layout: adding them, changing their scale,
//! position and transform, and describing them over IPC.

use aizome_ipc::{Event, Mode as IpcMode, OutputInfo, Rect as IpcRect, Transform as IpcTransform};
use smithay::desktop::layer_map_for_output;
use smithay::output::{Mode, Output, Scale};
use smithay::reexports::wayland_server::backend::GlobalId;
use smithay::utils::Transform;

use crate::state::{Aizome, OutputState};

/// A requested change to an output (from IPC, the settings file or
/// wlr-output-management).
#[derive(Clone, Debug, Default)]
pub struct OutputChange {
    pub name: String,
    /// Width, height, refresh (mHz).
    pub mode: Option<(i32, i32, Option<i32>)>,
    pub scale: Option<f64>,
    pub position: Option<(i32, i32)>,
    pub transform: Option<Transform>,
    pub enabled: Option<bool>,
}

/// smithay's scale for a number: integer where it is one.
pub fn scale_of(s: f64) -> Scale {
    if (s - s.round()).abs() < 1e-6 { Scale::Integer(s.round() as i32) } else { Scale::Fractional(s) }
}

/// Puts an output into the layout and advertises it.
pub fn add_output(az: &mut Aizome, output: &Output, mode: Mode, scale: f64, position: (i32, i32), _serial: &str) {
    output.change_current_state(Some(mode), Some(Transform::Normal), Some(scale_of(scale)), Some(position.into()));
    az.out.insert(output.clone(), OutputState::new(mode.refresh));
    attach(az, output);
}

/// Takes an output out of the layout for good (unplugged or turned off).
pub fn remove_output(az: &mut Aizome, output: &Output) {
    detach(az, output);
    az.out.remove(output);
    az.disabled_outputs.retain(|o| o != output);
    forget_mirrors(az, output);
}

/// An output leaves: it mirrors nothing, and what mirrored it gets its own
/// picture back.
fn forget_mirrors(az: &mut Aizome, output: &Output) {
    let freed: Vec<Output> = az.mirrors.iter().filter(|(_, s)| s == output).map(|(m, _)| m.clone()).collect();
    az.mirrors.retain(|(m, s)| m != output && s != output);
    for m in freed {
        let pos = free_position(az, None);
        m.change_current_state(None, None, None, Some(pos.into()));
        attach(az, &m);
    }
}

/// The wl_output global an output is advertised by.
struct OutputGlobal(std::sync::Mutex<Option<GlobalId>>);

/// Gives an output a place in the layout (its location as it stands) and
/// advertises it to clients.
pub fn attach(az: &mut Aizome, output: &Output) {
    output.user_data().insert_if_missing_threadsafe(|| OutputGlobal(std::sync::Mutex::new(None)));
    if let Some(g) = output.user_data().get::<OutputGlobal>() {
        let mut g = g.0.lock().unwrap();
        if g.is_none() {
            *g = Some(output.create_global::<crate::state::State>(&az.dh));
        }
    }
    if !az.outputs.contains(output) {
        az.outputs.push(output.clone());
    }
    az.disabled_outputs.retain(|o| o != output);
    layer_map_for_output(output).arrange();
    az.window_outputs_dirty = true;
    az.queue_redraw(output);
    outputs_changed(az);
}

/// Takes an output out of the layout and stops advertising it: its layer
/// surfaces close, and windows on it move to the outputs left.
pub fn detach(az: &mut Aizome, output: &Output) {
    az.outputs.retain(|o| o != output);
    let mut map = layer_map_for_output(output);
    let layers: Vec<_> = map.layers().cloned().collect();
    for l in layers {
        l.layer_surface().send_close();
        map.unmap_layer(&l);
    }
    drop(map);
    if let Some(g) = output.user_data().get::<OutputGlobal>()
        && let Some(id) = g.0.lock().unwrap().take()
    {
        // Clients still binding it get a moment before it goes.
        az.dh.disable_global::<crate::state::State>(id.clone());
        let dh = az.dh.clone();
        let _ = az.lh.insert_source(
            smithay::reexports::calloop::timer::Timer::from_duration(std::time::Duration::from_secs(5)),
            move |_, _, _| {
                dh.remove_global::<crate::state::State>(id.clone());
                smithay::reexports::calloop::timer::TimeoutAction::Drop
            },
        );
    }
    az.window_outputs_dirty = true;
    az.outputs_dirty = true;
    outputs_changed(az);
}

/// Turns an output off: out of the layout, kept to be turned on again.
pub fn disable(az: &mut Aizome, output: &Output) {
    detach(az, output);
    az.out.remove(output);
    forget_mirrors(az, output);
    if !az.disabled_outputs.contains(output) {
        az.disabled_outputs.push(output.clone());
    }
    outputs_changed(az);
}

/// Where an output turned on again goes: the place asked for, else right
/// of the others.
pub fn free_position(az: &Aizome, asked: Option<(i32, i32)>) -> (i32, i32) {
    asked.unwrap_or_else(|| (az.outputs.iter().map(|o| az.output_rect(o).right()).max().unwrap_or(0), 0))
}

/// Shows `source`'s picture on `output`, which leaves the layout (or, with
/// no source, gets its own picture back, right of the others).
pub fn set_mirror(az: &mut Aizome, output: &Output, source: Option<&Output>) -> Result<(), String> {
    match source {
        Some(s) => {
            if s == output {
                return Err(format!("{} can't mirror itself", output.name()));
            }
            if !az.outputs.contains(s) {
                return Err(format!("{} isn't showing a picture of its own", s.name()));
            }
            if az.mirrors.iter().any(|(m, src)| m == output && src == s) {
                return Ok(());
            }
            // What mirrored this one now mirrors the source too.
            for m in az.mirrors.iter_mut() {
                if &m.1 == output {
                    m.1 = s.clone();
                }
            }
            az.mirrors.retain(|(m, _)| m != output);
            if az.outputs.contains(output) {
                detach(az, output);
            }
            az.mirrors.push((output.clone(), s.clone()));
            az.queue_redraw(output);
        }
        None => {
            if !az.mirrors.iter().any(|(m, _)| m == output) {
                return Ok(());
            }
            az.mirrors.retain(|(m, _)| m != output);
            let pos = free_position(az, None);
            output.change_current_state(None, None, None, Some(pos.into()));
            attach(az, output);
        }
    }
    outputs_changed(az);
    Ok(())
}

/// Applies scale, position and transform (the parts every backend shares).
pub fn apply_logical(az: &mut Aizome, change: &OutputChange) -> Result<(), String> {
    let output = az.known_output(&change.name).ok_or_else(|| format!("no output {}", change.name))?;
    let scale = change.scale.map(|s| {
        if !(0.5..=4.0).contains(&s) {
            return Err(format!("scale {s} is out of range"));
        }
        Ok(scale_of(s))
    });
    let scale = match scale {
        Some(Ok(s)) => Some(s),
        Some(Err(e)) => return Err(e),
        None => None,
    };
    output.change_current_state(None, change.transform, scale, change.position.map(|p| p.into()));
    layer_map_for_output(&output).arrange();
    az.window_outputs_dirty = true;
    az.queue_redraw_all();
    outputs_changed(az);
    Ok(())
}

pub fn transform_to_ipc(t: Transform) -> IpcTransform {
    match t {
        Transform::Normal => IpcTransform::Normal,
        Transform::_90 => IpcTransform::_90,
        Transform::_180 => IpcTransform::_180,
        Transform::_270 => IpcTransform::_270,
        Transform::Flipped => IpcTransform::Flipped,
        Transform::Flipped90 => IpcTransform::Flipped90,
        Transform::Flipped180 => IpcTransform::Flipped180,
        Transform::Flipped270 => IpcTransform::Flipped270,
    }
}

pub fn transform_from_ipc(t: IpcTransform) -> Transform {
    match t {
        IpcTransform::Normal => Transform::Normal,
        IpcTransform::_90 => Transform::_90,
        IpcTransform::_180 => Transform::_180,
        IpcTransform::_270 => Transform::_270,
        IpcTransform::Flipped => Transform::Flipped,
        IpcTransform::Flipped90 => Transform::Flipped90,
        IpcTransform::Flipped180 => Transform::Flipped180,
        IpcTransform::Flipped270 => Transform::Flipped270,
    }
}

pub fn transform_from_config(t: aizome_config::Transform) -> Transform {
    use aizome_config::Transform as C;
    match t {
        C::Normal => Transform::Normal,
        C::_90 => Transform::_90,
        C::_180 => Transform::_180,
        C::_270 => Transform::_270,
        C::Flipped => Transform::Flipped,
        C::Flipped90 => Transform::Flipped90,
        C::Flipped180 => Transform::Flipped180,
        C::Flipped270 => Transform::Flipped270,
    }
}

fn ipc_rect(r: crate::Rect) -> IpcRect {
    IpcRect::new(r.x, r.y, r.w, r.h)
}

pub fn info(az: &Aizome, output: &Output) -> OutputInfo {
    let props = output.physical_properties();
    let mode = output.current_mode();
    let preferred = output.preferred_mode();
    let modes = output
        .modes()
        .into_iter()
        .map(|m| IpcMode { width: m.size.w, height: m.size.h, refresh: m.refresh, preferred: Some(m) == preferred })
        .collect();
    let serial = output.user_data().get::<OutputSerial>().map(|s| s.0.clone()).unwrap_or_default();
    let enabled = !az.disabled_outputs.contains(output);
    let mirror = az.mirrors.iter().find(|(m, _)| m == output).map(|(_, s)| s.clone());
    // A mirror shows its source's part of the layout.
    let place = mirror.as_ref().unwrap_or(output);
    let (logical, usable) = if enabled {
        (ipc_rect(az.output_rect(place)), ipc_rect(az.usable_area(place)))
    } else {
        (IpcRect::default(), IpcRect::default())
    };
    OutputInfo {
        name: output.name(),
        make: props.make,
        model: props.model,
        serial,
        enabled,
        current_mode: mode.map(|m| IpcMode {
            width: m.size.w,
            height: m.size.h,
            refresh: m.refresh,
            preferred: Some(m) == preferred,
        }),
        modes,
        scale: output.current_scale().fractional_scale(),
        transform: transform_to_ipc(output.current_transform()),
        logical,
        usable,
        physical_size_mm: (props.size.w, props.size.h),
        powered: enabled && az.out.get(output).is_none_or(|o| o.powered),
        primary: az.primary_output().as_ref() == Some(output),
        mirror: mirror.map(|s| s.name()),
        gamma: az.out.get(output).is_some_and(|o| o.gamma),
    }
}

impl Aizome {
    /// The main display: the session's choice, else the settings'
    /// (`primary = true`), else the first in the layout.
    pub fn primary_output(&self) -> Option<Output> {
        if let Some(name) = &self.primary_override
            && let Some(o) = self.outputs.iter().find(|o| &o.name() == name)
        {
            return Some(o.clone());
        }
        self.outputs
            .iter()
            .find(|o| {
                let props = o.physical_properties();
                let serial = o.user_data().get::<OutputSerial>().map(|s| s.0.clone()).unwrap_or_default();
                self.settings.output(&o.name(), &props.make, &props.model, &serial).is_some_and(|c| c.primary())
            })
            .or_else(|| self.outputs.first())
            .cloned()
    }

    /// Any output the backend drives or knows: in the layout, mirroring,
    /// or turned off.
    pub fn known_outputs(&self) -> Vec<Output> {
        let mut v = self.outputs.clone();
        v.extend(self.mirrors.iter().map(|(m, _)| m.clone()));
        v.extend(self.disabled_outputs.iter().cloned());
        v
    }

    pub fn known_output(&self, name: &str) -> Option<Output> {
        self.known_outputs().into_iter().find(|o| o.name() == name)
    }

    /// The output a mirror shows.
    pub fn mirror_source(&self, output: &Output) -> Option<Output> {
        self.mirrors.iter().find(|(m, _)| m == output).map(|(_, s)| s.clone())
    }
}

/// An output's serial number, kept in its user data.
pub struct OutputSerial(pub String);

pub fn all_info(az: &Aizome) -> Vec<OutputInfo> {
    az.known_outputs().iter().map(|o| info(az, o)).collect()
}

pub fn outputs_changed(az: &mut Aizome) {
    // Existing lock surfaces must follow mode, scale and rotation changes
    // too. Smithay suppresses unchanged/pending duplicate configures.
    for output in &az.outputs {
        if let Some(lock) = az.out.get(output).and_then(|o| o.lock_surface.as_ref()).filter(|l| l.alive()) {
            let size = az.output_geometry(output).size;
            lock.with_pending_state(|s| s.size = Some((size.w as u32, size.h as u32).into()));
            lock.send_configure();
        }
    }
    // The logic hears before the next frame (windows off every output
    // come back, filled ones refit).
    az.outputs_dirty = true;
    let outputs = all_info(az);
    az.ipc.emit(&Event::OutputsChanged { outputs });
    crate::protocols::output_management::outputs_changed(az);
}

impl crate::state::State {
    /// Changes an output (IPC, settings, wlr-output-management): turning
    /// one on that was off sets it up afresh on udev.
    pub fn configure_output(&mut self, change: &OutputChange) -> Result<(), String> {
        let off = self.az.disabled_outputs.iter().any(|o| o.name() == change.name);
        if off && change.enabled == Some(true) && matches!(self.backend, crate::backend::Backend::Udev(_)) {
            crate::backend::udev::enable_output(self, &change.name, change.position)?;
            let rest = OutputChange { enabled: None, position: None, ..change.clone() };
            return self.backend.configure_output(&mut self.az, &rest);
        }
        self.backend.configure_output(&mut self.az, change)?;
        if change.enabled == Some(false) {
            // An explicit disable while docked stays disabled on lid-open.
            self.az.power.lid_disabled.retain(|saved| saved.name != change.name);
        }
        if change.enabled == Some(true) {
            self.update_night_light(true);
        }
        Ok(())
    }

    /// Shows `source`'s picture on an output, or (none) its own again.
    pub fn mirror_output(&mut self, name: &str, source: Option<&str>) -> Result<(), String> {
        let output = self.az.known_output(name).ok_or_else(|| format!("no output {name}"))?;
        if self.az.disabled_outputs.contains(&output) {
            return Err(format!("{name} is off"));
        }
        let source = match source {
            Some(s) => Some(self.az.output_by_name(s).ok_or_else(|| format!("no output {s} in the layout"))?),
            None => None,
        };
        set_mirror(&mut self.az, &output, source.as_ref())
    }
}

impl crate::state::State {
    /// The pointer starts in the middle of the main display.
    pub fn place_pointer_on_primary(&mut self) {
        let Some(o) = self.az.primary_output() else { return };
        let g = self.az.output_geometry(&o).to_f64();
        let p = g.loc + smithay::utils::Point::from((g.size.w / 2.0, g.size.h / 2.0));
        self.az.pointer_pos = p;
        if let Some(pointer) = self.az.seat.get_pointer() {
            pointer.set_location(p);
        }
    }
}
