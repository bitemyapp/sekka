//! A shell-like test client: layer surfaces with keyboard interactivity,
//! and xdg_popups (menus) on them, recording which of its surfaces has the
//! keyboard, the keys each gets, and the menus the compositor dismissed.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::{xdg_popup, xdg_positioner, xdg_surface, xdg_wm_base};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

pub use zwlr_layer_shell_v1::Layer;
pub use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity};

use super::{Compositor, dispatch_round, solid_buffer};

#[derive(Clone, Debug)]
pub struct LayerSpec {
    pub name: &'static str,
    pub layer: Layer,
    pub keyboard: KeyboardInteractivity,
    pub anchor: Anchor,
    /// A side of 0 is the output's (with both its edges anchored).
    pub size: (u32, u32),
    pub color: u32,
    pub exclusive_zone: i32,
    pub margin_top: i32,
}

impl LayerSpec {
    pub fn new(name: &'static str, layer: Layer, keyboard: KeyboardInteractivity) -> LayerSpec {
        LayerSpec {
            name,
            layer,
            keyboard,
            anchor: Anchor::Top,
            size: (400, 100),
            color: 0x40a040,
            exclusive_zone: 0,
            margin_top: 0,
        }
    }

    pub fn anchor(mut self, anchor: Anchor, size: (u32, u32)) -> LayerSpec {
        self.anchor = anchor;
        self.size = size;
        self
    }

    pub fn color(mut self, color: u32) -> LayerSpec {
        self.color = color;
        self
    }

    pub fn zone(mut self, zone: i32) -> LayerSpec {
        self.exclusive_zone = zone;
        self
    }

    pub fn top_margin(mut self, margin: i32) -> LayerSpec {
        self.margin_top = margin;
        self
    }
}

pub enum ShellCmd {
    Layer(LayerSpec),
    Resize {
        name: &'static str,
        size: (u32, u32),
    },
    /// A menu on a layer surface, at a place relative to it; `grab` asks
    /// for an explicit grab with the latest input serial (0 without one,
    /// as after a shortcut that came over IPC).
    Popup {
        name: &'static str,
        parent: &'static str,
        at: (i32, i32),
        size: (i32, i32),
        color: u32,
        grab: bool,
    },
    Frame(&'static str),
    Destroy(&'static str),
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct ShellSeen {
    /// The surface with the keyboard, by name.
    pub focus: Option<String>,
    /// Keys pressed (evdev codes), with the surface that had the keyboard.
    pub keys: Vec<(String, u32)>,
    /// Surfaces configured and drawn.
    pub shown: Vec<String>,
    /// Completed one-shot frame requests, by surface name.
    pub frames: HashMap<String, u32>,
    /// Menus the compositor dismissed (popup_done).
    pub dismissed: Vec<String>,
    /// The surface the pointer is over, by name.
    pub pointer: Option<String>,
    /// Pointer buttons with their actual Wayland recipient (not keyboard focus).
    pub buttons: Vec<(String, u32, bool)>,
}

impl ShellSeen {
    pub fn keys_to(&self, name: &str) -> Vec<u32> {
        self.keys.iter().filter(|(n, _)| n == name).map(|(_, k)| *k).collect()
    }
}

pub struct ShellClient {
    tx: Sender<ShellCmd>,
    pub seen: Arc<Mutex<ShellSeen>>,
    join: Option<JoinHandle<()>>,
}

impl ShellClient {
    pub fn spawn(c: &Compositor) -> ShellClient {
        let stream = UnixStream::connect(c.dir.join(&c.socket)).expect("connect to the compositor");
        let (tx, rx) = channel();
        let seen = Arc::new(Mutex::new(ShellSeen::default()));
        let seen2 = seen.clone();
        let join = std::thread::spawn(move || run(stream, rx, seen2));
        ShellClient { tx, seen, join: Some(join) }
    }

    pub fn send(&self, c: ShellCmd) {
        let _ = self.tx.send(c);
    }

    pub fn seen(&self) -> ShellSeen {
        self.seen.lock().unwrap().clone()
    }

    pub fn wait(&self, what: &str, f: impl Fn(&ShellSeen) -> bool) -> ShellSeen {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = self.seen();
            if f(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Shows a layer surface and waits for it to be drawn.
    pub fn layer(&self, spec: LayerSpec) {
        let name = spec.name;
        self.send(ShellCmd::Layer(spec));
        self.wait(name, |s| s.shown.iter().any(|n| n == name));
    }
}

impl Drop for ShellClient {
    fn drop(&mut self) {
        let _ = self.tx.send(ShellCmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct Surf {
    surface: wl_surface::WlSurface,
    layer: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    popup: Option<(xdg_surface::XdgSurface, xdg_popup::XdgPopup)>,
    size: (i32, i32),
    color: u32,
}

struct Shell {
    seen: Arc<Mutex<ShellSeen>>,
    shm: wl_shm::WlShm,
    surfaces: HashMap<String, Surf>,
    serial: Option<u32>,
}

impl Shell {
    fn name_of(&self, surface: &wl_surface::WlSurface) -> Option<String> {
        self.surfaces.iter().find(|(_, s)| &s.surface == surface).map(|(n, _)| n.clone())
    }

    fn draw(&mut self, name: &str, qh: &QueueHandle<Shell>) {
        let Some(s) = self.surfaces.get(name) else { return };
        let (w, h) = s.size;
        let buffer = solid_buffer(&self.shm, (w, h), s.color, qh);
        s.surface.attach(Some(&buffer), 0, 0);
        s.surface.damage_buffer(0, 0, w, h);
        s.surface.commit();
        self.seen.lock().unwrap().shown.push(name.to_string());
    }

    fn destroy(&mut self, name: &str) {
        {
            // No leave comes for a surface the client destroyed.
            let mut seen = self.seen.lock().unwrap();
            if seen.focus.as_deref() == Some(name) {
                seen.focus = None;
            }
            if seen.pointer.as_deref() == Some(name) {
                seen.pointer = None;
            }
        }
        if let Some(s) = self.surfaces.remove(name) {
            if let Some((x, p)) = s.popup {
                p.destroy();
                x.destroy();
            }
            if let Some(l) = s.layer {
                l.destroy();
            }
            s.surface.destroy();
        }
    }
}

fn run(stream: UnixStream, rx: Receiver<ShellCmd>, seen: Arc<Mutex<ShellSeen>>) {
    let conn = Connection::from_socket(stream).expect("wayland connection");
    let (globals, mut queue): (_, EventQueue<Shell>) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).expect("wl_compositor");
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).expect("xdg_wm_base");
    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 =
        globals.bind(&qh, 4..=5, ()).expect("zwlr_layer_shell_v1 4");
    let seat: wl_seat::WlSeat = globals.bind(&qh, 5..=9, ()).expect("wl_seat");
    let _keyboard = seat.get_keyboard(&qh, ());
    let _pointer = seat.get_pointer(&qh, ());
    let mut shell = Shell { seen, shm, surfaces: HashMap::new(), serial: None };
    let mut quit = false;
    while !quit {
        for cmd in rx.try_iter() {
            match cmd {
                ShellCmd::Resize { name, size } => {
                    let s = shell.surfaces.get(name).expect("layer to resize");
                    s.layer.as_ref().unwrap().set_size(size.0, size.1);
                    s.surface.commit();
                }
                ShellCmd::Layer(spec) => {
                    let surface = compositor.create_surface(&qh, ());
                    let l = layer_shell.get_layer_surface(
                        &surface,
                        None,
                        spec.layer,
                        spec.name.to_string(),
                        &qh,
                        spec.name.to_string(),
                    );
                    l.set_anchor(spec.anchor);
                    l.set_size(spec.size.0, spec.size.1);
                    l.set_keyboard_interactivity(spec.keyboard);
                    l.set_exclusive_zone(spec.exclusive_zone);
                    l.set_margin(spec.margin_top, 0, 0, 0);
                    surface.commit();
                    shell.surfaces.insert(
                        spec.name.to_string(),
                        Surf {
                            surface,
                            layer: Some(l),
                            popup: None,
                            size: (spec.size.0 as i32, spec.size.1 as i32),
                            color: spec.color,
                        },
                    );
                }
                ShellCmd::Popup { name, parent, at, size, color, grab } => {
                    let Some(parent_layer) = shell.surfaces.get(parent).and_then(|p| p.layer.clone()) else {
                        panic!("no layer surface {parent}");
                    };
                    let surface = compositor.create_surface(&qh, ());
                    let positioner = wm.create_positioner(&qh, ());
                    positioner.set_size(size.0, size.1);
                    positioner.set_anchor_rect(at.0, at.1, 1, 1);
                    positioner.set_anchor(xdg_positioner::Anchor::TopLeft);
                    positioner.set_gravity(xdg_positioner::Gravity::BottomRight);
                    let xs = wm.get_xdg_surface(&surface, &qh, name.to_string());
                    let popup = xs.get_popup(None, &positioner, &qh, name.to_string());
                    parent_layer.get_popup(&popup);
                    if grab {
                        popup.grab(&seat, shell.serial.unwrap_or(0));
                    }
                    positioner.destroy();
                    surface.commit();
                    shell
                        .surfaces
                        .insert(name.to_string(), Surf { surface, layer: None, popup: Some((xs, popup)), size, color });
                }
                ShellCmd::Frame(name) => {
                    let s = &shell.surfaces[name];
                    s.surface.frame(&qh, name.to_string());
                    s.surface.commit();
                }
                ShellCmd::Destroy(name) => shell.destroy(name),
                ShellCmd::Quit => quit = true,
            }
        }
        if !dispatch_round(&conn, &mut queue, &mut shell) {
            break;
        }
    }
    let names: Vec<String> = shell.surfaces.keys().cloned().collect();
    for n in names {
        shell.destroy(&n);
    }
    let _ = conn.flush();
}

impl Dispatch<wl_callback::WlCallback, String> for Shell {
    fn event(
        shell: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        name: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        *shell.seen.lock().unwrap().frames.entry(name.clone()).or_default() += 1;
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Shell {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Shell {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_seat::WlSeat,
    xdg_positioner::XdgPositioner,
    zwlr_layer_shell_v1::ZwlrLayerShellV1
);

impl Dispatch<wl_buffer::WlBuffer, ()> for Shell {
    fn event(
        _: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Shell {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Shell {
    fn event(
        shell: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { surface, .. } => {
                let name = shell.name_of(&surface);
                shell.seen.lock().unwrap().focus = name;
            }
            wl_keyboard::Event::Leave { .. } => shell.seen.lock().unwrap().focus = None,
            wl_keyboard::Event::Key { serial, key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                shell.serial = Some(serial);
                let mut seen = shell.seen.lock().unwrap();
                let to = seen.focus.clone().unwrap_or_default();
                seen.keys.push((to, key));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Shell {
    fn event(
        shell: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Button { serial, button, state: WEnum::Value(state), .. } => {
                let pressed = state == wl_pointer::ButtonState::Pressed;
                if pressed {
                    shell.serial = Some(serial);
                }
                let mut seen = shell.seen.lock().unwrap();
                let to = seen.pointer.clone().unwrap_or_default();
                seen.buttons.push((to, button, pressed));
            }
            wl_pointer::Event::Enter { surface, .. } => {
                let name = shell.name_of(&surface);
                shell.seen.lock().unwrap().pointer = name;
            }
            wl_pointer::Event::Leave { .. } => shell.seen.lock().unwrap().pointer = None,
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, String> for Shell {
    fn event(
        shell: &mut Self,
        l: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        name: &String,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                l.ack_configure(serial);
                if let Some(s) = shell.surfaces.get_mut(name) {
                    if width > 0 {
                        s.size.0 = width as i32;
                    }
                    if height > 0 {
                        s.size.1 = height as i32;
                    }
                }
                shell.draw(name, qh);
            }
            zwlr_layer_surface_v1::Event::Closed => shell.destroy(name),
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, String> for Shell {
    fn event(
        shell: &mut Self,
        xs: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        name: &String,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            shell.draw(name, qh);
        }
    }
}

impl Dispatch<xdg_popup::XdgPopup, String> for Shell {
    fn event(
        shell: &mut Self,
        _: &xdg_popup::XdgPopup,
        event: xdg_popup::Event,
        name: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_popup::Event::Configure { width, height, .. } => {
                if let Some(s) = shell.surfaces.get_mut(name)
                    && width > 0
                    && height > 0
                {
                    s.size = (width, height);
                }
            }
            xdg_popup::Event::PopupDone => {
                shell.seen.lock().unwrap().dismissed.push(name.clone());
                shell.destroy(name);
            }
            _ => {}
        }
    }
}
