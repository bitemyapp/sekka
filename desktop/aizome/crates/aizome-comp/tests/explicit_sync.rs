//! Real DRM timelines and DMA-BUFs, through a private headless compositor.
//! Run the ignored test with AIZOME_RENDER_NODE pointing to the GPU under test.
mod common;

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use aizome_comp_core::smithay::reexports::{drm, gbm};
use common::{Compositor, Options};
use drm::control::Device as _;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};
use wayland_protocols::wp::linux_drm_syncobj::v1::client::{
    wp_linux_drm_syncobj_manager_v1, wp_linux_drm_syncobj_surface_v1, wp_linux_drm_syncobj_timeline_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

struct Device(File);
impl AsFd for Device {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl drm::Device for Device {}
impl drm::control::Device for Device {}

#[derive(Default)]
struct Probe {
    configured: bool,
}

#[test]
fn software_compositor_does_not_advertise_explicit_sync() {
    // Even when asked for: pixman has no GPU fences to wait on.
    let settings = "[appearance]\neffects = \"austere\"\n[performance]\nexplicit_sync = \"on\"\n";
    let c = Compositor::start(Options { renderer: Some("pixman"), settings, ..Default::default() });
    assert!(!advertised(&c));
}

fn advertised(c: &Compositor) -> bool {
    let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, _) = registry_queue_init::<Probe>(&conn).unwrap();
    globals.contents().clone_list().iter().any(|g| g.interface == "wp_linux_drm_syncobj_manager_v1")
}

#[test]
#[ignore = "requires a real GPU; set AIZOME_RENDER_NODE"]
fn explicit_sync_is_offered_by_default_only_on_nvidia() {
    let path = std::env::var("AIZOME_RENDER_NODE").expect("set AIZOME_RENDER_NODE to the GPU under test");
    let file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let nvidia = drm::Device::get_driver(&Device(file)).unwrap().name() == "nvidia-drm";
    let c = Compositor::start(Options { renderer: Some("gles"), ..Default::default() });
    assert!(c.log_text().contains(&format!("rendering with GLES on {path}")), "{}", c.log_text());
    assert_eq!(advertised(&c), nvidia, "{}", c.log_text());
    let settings = "[appearance]\neffects = \"austere\"\n[performance]\nexplicit_sync = \"off\"\n";
    let c = Compositor::start(Options { renderer: Some("gles"), settings, ..Default::default() });
    assert!(!advertised(&c), "{}", c.log_text());
}

#[test]
#[ignore = "requires a real GPU with syncobj eventfd and DMA-BUF import; set AIZOME_RENDER_NODE"]
fn acquire_waits_and_release_retires_on_real_gpu() {
    let path = std::env::var("AIZOME_RENDER_NODE").expect("set AIZOME_RENDER_NODE to the GPU under test");
    let node = std::fs::canonicalize(&path).unwrap();
    // Offered by default only on NVIDIA; this test applies to any GPU.
    let settings = "[appearance]\neffects = \"austere\"\n[performance]\nexplicit_sync = \"on\"\n";
    let c = Compositor::start(Options { renderer: Some("gles"), settings, ..Default::default() });
    assert!(c.log_text().contains(&format!("rendering with GLES on {path}")), "{}", c.log_text());
    assert!(c.log_text().contains("explicit synchronization available"), "{}", c.log_text());
    let stream = UnixStream::connect(c.dir.join(&c.socket)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let conn = Connection::from_socket(stream).unwrap();
    let (globals, mut queue) = registry_queue_init::<Probe>(&conn).unwrap();
    let qh = queue.handle();
    let mut probe = Probe::default();
    let comp: wl_compositor::WlCompositor = globals.bind(&qh, 5..=6, ()).unwrap();
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
    let dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 = globals.bind(&qh, 3..=3, ()).unwrap();
    let sync: wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
    let surface = comp.create_surface(&qh, ());
    let xs = wm.get_xdg_surface(&surface, &qh, ());
    let tl = xs.get_toplevel(&qh, ());
    tl.set_app_id("org.test.explicit-sync".into());
    surface.commit();
    while !probe.configured {
        queue.roundtrip(&mut probe).unwrap();
    }
    let ss = sync.get_surface(&surface, &qh, ());

    let file = OpenOptions::new().read(true).write(true).open(&node).unwrap();
    let dev = Device(file.try_clone().unwrap());
    let allocator = gbm::Device::new(file).unwrap();
    let timeline = dev.create_syncobj(false).unwrap();
    let fd = dev.syncobj_to_fd(timeline, false).unwrap();
    let wt = sync.import_timeline(fd.as_fd(), &qh, ());
    let point = || {
        let mut points = [0];
        dev.syncobj_timeline_query(&[timeline], &mut points, false).unwrap();
        points[0]
    };
    let bo = allocator
        .create_buffer_object::<()>(300, 200, gbm::Format::Xrgb8888, gbm::BufferObjectFlags::RENDERING)
        .unwrap();
    let params = dmabuf.create_params(&qh, ());
    let modifier: u64 = bo.modifier().into();
    for plane in 0..bo.plane_count() as i32 {
        params.add(
            bo.fd_for_plane(plane).unwrap().as_fd(),
            plane as u32,
            bo.offset(plane),
            bo.stride_for_plane(plane),
            (modifier >> 32) as u32,
            modifier as u32,
        );
    }
    let buffer = params.create_immed(
        300,
        200,
        gbm::Format::Xrgb8888 as u32,
        zwp_linux_buffer_params_v1::Flags::empty(),
        &qh,
        (),
    );
    params.destroy();
    queue.roundtrip(&mut probe).unwrap();
    // A point with no submitted fence yet must wait too (WAIT_FOR_SUBMIT).
    ss.set_acquire_point(&wt, 0, 1);
    ss.set_release_point(&wt, 0, 2);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, 300, 200);
    surface.commit();
    queue.roundtrip(&mut probe).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!(c.window("org.test.explicit-sync").is_none(), "sampled before acquire");
    assert_eq!(point(), 0, "released before acquire");
    // The compositor remains responsive while this one client's buffer waits.
    c.ok(aizome_ipc::Request::ReloadConfig);
    dev.syncobj_timeline_signal(&[timeline], &[1]).unwrap();
    c.wait_window("org.test.explicit-sync");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(point(), 1, "released a buffer still in use");

    // Explicit sync survives destroying the per-surface protocol object:
    // the committed buffer still owns its release point until retirement.
    ss.destroy();
    surface.attach(None, 0, 0);
    surface.commit();
    queue.roundtrip(&mut probe).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while point() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(point(), 2, "release timeline wasn't signalled after unmapping: {}", c.log_text());
    assert!(c.window("org.test.explicit-sync").is_none());
    // Abandoned acquire points must not leak eventfds/registrations.
    let pid = match c.request(aizome_ipc::Request::Version) {
        aizome_ipc::Response::Version { pid, .. } => pid,
        other => panic!("{other:?}"),
    };
    let fd_count = || std::fs::read_dir(format!("/proc/{pid}/fd")).unwrap().count();
    let before = fd_count();
    for n in 0..32 {
        let abandoned = comp.create_surface(&qh, ());
        let sync_surface = sync.get_surface(&abandoned, &qh, ());
        sync_surface.set_acquire_point(&wt, 0, 100 + n * 2);
        sync_surface.set_release_point(&wt, 0, 101 + n * 2);
        abandoned.attach(Some(&buffer), 0, 0);
        abandoned.commit();
        queue.roundtrip(&mut probe).unwrap();
        // wl_surface.destroy must cancel the wait even when the surface
        // has never been mapped, and the client remains connected.
        abandoned.destroy();
        sync_surface.destroy();
        queue.roundtrip(&mut probe).unwrap();
    }
    assert!(fd_count() <= before + 2, "abandoned fences leaked descriptors: {before} -> {}", fd_count());
    dev.destroy_syncobj(timeline).unwrap();
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
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
impl Dispatch<xdg_surface::XdgSurface, ()> for Probe {
    fn event(
        state: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            state.configured = true;
        }
    }
}
impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Probe {
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
macro_rules! ignore {
    ($($module:ident::$ty:ident),* $(,)?) => {$(
        impl Dispatch<$module::$ty, ()> for Probe {
            fn event(_: &mut Self, _: &$module::$ty, _: $module::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_surface::WlSurface,
    wl_buffer::WlBuffer,
    xdg_toplevel::XdgToplevel,
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
    wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1,
    wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1,
    wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1
);
