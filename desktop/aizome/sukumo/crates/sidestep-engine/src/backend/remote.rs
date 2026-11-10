//! Export only on compositor request. Buffers are still painted and committed.
use super::*;
use aizome_remote_protocol::client::{
    aizome_remote_paint_manager_v1::AizomeRemotePaintManagerV1 as Manager,
    aizome_remote_paint_surface_v1::{self as surface, AizomeRemotePaintSurfaceV1 as Surface},
};
use std::io::Write;
use std::os::fd::AsFd;

pub(super) struct Export {
    object: Surface,
    active: bool,
    full: bool,
    exporter: crate::remote::Exporter,
    serial: u32,
}
impl Export {
    pub(super) fn new(manager: &Manager, surface: &WlSurface, qh: &QueueHandle<State>, window: WindowId) -> Self {
        Self {
            object: manager.get_surface(surface, qh, window),
            active: false,
            full: true,
            exporter: Default::default(),
            serial: 0,
        }
    }
    fn unsupported(&mut self) {
        self.object.unsupported();
        self.full = true;
        self.exporter = Default::default();
        self.serial = 0;
    }
}
impl Drop for Export {
    fn drop(&mut self) {
        self.object.destroy();
    }
}
pub(super) fn paint(win: &mut Win, pending: &[Pending]) {
    let supported = !win.transparent && tiles::is_empty(&win.layers);
    let Some(remote) = win.remote.as_mut().filter(|r| r.active) else { return };
    if !supported {
        remote.unsupported();
        return;
    }
    let batches = pending
        .iter()
        .map(|p| match p {
            Pending::Paint(d, o) => Some((d.as_slice(), o.as_slice())),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(batches) = batches.filter(|b| !b.is_empty()) else {
        remote.unsupported();
        return;
    };
    let full = remote.full;
    if full
        && !replaced(batches[0].0, batches[0].1, win.px_width, win.px_height, win.scale)
            .iter()
            .any(|r| r.x0 <= 0.0 && r.y0 <= 0.0 && r.x1 >= win.px_width as f32 && r.y1 >= win.px_height as f32)
    {
        remote.unsupported();
        return;
    }
    let bytes = match remote.exporter.batches(win.px_width, win.px_height, win.scale as f32, full, &batches) {
        Ok(bytes) => bytes,
        Err(_) => {
            remote.unsupported();
            return;
        }
    };
    let result = (|| -> std::io::Result<()> {
        use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
        let fd = memfd_create("aizome-retained-paint", MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)?;
        let mut file = std::fs::File::from(fd);
        file.write_all(&bytes)?;
        fcntl_add_seals(&file, SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL)?;
        remote.serial = remote.serial.wrapping_add(1);
        remote.object.paint(
            remote.serial,
            file.as_fd(),
            bytes.len() as u32,
            win.px_width,
            win.px_height,
            win.scale,
            u32::from(full),
        );
        Ok(())
    })();
    if result.is_err() {
        remote.unsupported();
    } else {
        remote.full = false;
    }
}
impl Dispatch<Manager, ()> for State {
    fn event(_: &mut Self, _: &Manager, _: <Manager as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<Surface, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &Surface,
        event: surface::Event,
        window: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let surface::Event::Enabled { enabled } = event else { return };
        let Some(win) = state.windows.get_mut(window) else { return };
        let Some(remote) = &mut win.remote else { return };
        remote.active = enabled != 0;
        remote.full = true;
        remote.serial = 0;
        remote.exporter = Default::default();
        if remote.active && win.configured {
            let rects = vec![Rect::new(0.0, 0.0, win.width as f32, win.height as f32)];
            let _ = state.to_main.send(FromRender::Repaint { window: *window, target: Target::Root, rects });
        }
    }
}
