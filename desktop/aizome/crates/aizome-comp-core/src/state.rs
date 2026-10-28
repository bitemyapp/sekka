//! The compositor's state: smithay's protocol states, the window manager's
//! model and the windows behind it, outputs, input, settings and IPC.
//!
//! `State` pairs the backend with everything else (`Aizome`), so rendering
//! can borrow the renderer and the scene at once.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_config::Settings;
use aizome_theme::Palette;
use smithay::desktop::{PopupManager, Window, layer_map_for_output, space::SpaceElement};
use smithay::input::keyboard::{Keycode, XkbConfig};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatState};
use smithay::output::Output;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{Clock, Logical, Monotonic, Point, Rectangle, Size};
use smithay::wayland::alpha_modifier::AlphaModifierState;
use smithay::wayland::commit_timing::CommitTimingManagerState;
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::content_type::ContentTypeState;
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufState};
use smithay::wayland::fifo::FifoManagerState;
use smithay::wayland::foreign_toplevel_list::ForeignToplevelListState;
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::idle_inhibit::IdleInhibitManagerState;
use smithay::wayland::idle_notify::IdleNotifierState;
use smithay::wayland::input_method::InputMethodManagerState;
use smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::pointer_constraints::PointerConstraintsState;
use smithay::wayland::pointer_gestures::PointerGesturesState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::security_context::{SecurityContext, SecurityContextState};
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::selection::ext_data_control::DataControlState as ExtDataControlState;
use smithay::wayland::selection::primary_selection::PrimarySelectionState;
use smithay::wayland::selection::wlr_data_control::DataControlState as WlrDataControlState;
use smithay::wayland::session_lock::{LockSurface, SessionLockManagerState, SessionLocker};
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::dialog::XdgDialogState;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgShellState};
use smithay::wayland::shm::ShmState;
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::tablet_manager::TabletManagerState;
use smithay::wayland::text_input::TextInputManagerState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState;
use smithay::wayland::xdg_activation::XdgActivationState;
use smithay::wayland::xdg_foreign::XdgForeignState;
use tracing::{info, warn};

use crate::backend::Backend;
use crate::cursor::CursorTheme;
use crate::ipc::IpcServer;
use crate::logic::LogicHost;
use crate::protocols::foreign_toplevel::ForeignToplevelManagerState;
use crate::protocols::output_management::OutputManagementState;
use crate::protocols::screencopy::ScreencopyManagerState;
use crate::stats::FrameStats;
use crate::view::{View, WinView};
use crate::{Rect, WinId};

/// Per-client data.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// Connected through a security context (a sandbox): no privileged
    /// protocols.
    pub security_context: Option<SecurityContext>,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        // A client cut off for a protocol error sees only a broken pipe:
        // say here what it was.
        if let DisconnectReason::ProtocolError(e) = reason {
            warn!(
                client = ?client_id,
                "client disconnected for a protocol error: {}@{} error {}: {}",
                e.object_interface,
                e.object_id,
                e.code,
                e.message
            );
        }
    }
}

/// Whether a client may use privileged protocols (screencopy, foreign
/// toplevel management, output management, data control, virtual
/// keyboards…): not from inside a sandbox.
pub fn client_is_privileged(client: &smithay::reexports::wayland_server::Client) -> bool {
    client.get_data::<ClientState>().is_none_or(|d| d.security_context.is_none())
}

/// A toplevel that hasn't drawn yet.
pub struct Unmapped {
    pub window: Window,
    /// The client asked for client-side decorations (or never spoke
    /// xdg-decoration and so draws its own).
    pub wants_csd: bool,
}

/// A window on the desktop: the smithay window, and where the logic last
/// said it is.
pub struct Mapped {
    pub id: WinId,
    pub window: Window,
    /// Published by the logic ([`crate::view`]).
    pub view: WinView,
}

impl Mapped {
    pub fn toplevel(&self) -> &ToplevelSurface {
        self.window.toplevel().expect("wayland toplevel")
    }
    pub fn surface(&self) -> &WlSurface {
        self.toplevel().wl_surface()
    }
}

/// What the compositor keeps per output.
pub struct OutputState {
    pub redraw_queued: bool,
    pub stats: FrameStats,
    pub lock_surface: Option<LockSurface>,
    pub powered: bool,
    /// Frames rendered, for presentation sequence numbers.
    pub frame_seq: u64,
    /// Frames that drew something (screencopy waits on these).
    pub damaged_frames: u64,
    pub last_frame: Option<Instant>,
    /// When the display last showed a frame (its vblank, on the monotonic
    /// clock), where it says: the next frames' times count from it.
    pub presented: Option<Duration>,
    pub refresh: Duration,
    /// The display takes gamma ramps (Night Light).
    pub gamma: bool,
    /// A wlr-gamma-control client holds its ramps.
    pub gamma_client: bool,
    /// Last Deskbar geometry reported to the layout policy. Surface buffers
    /// have already changed by the time commit_layer runs, so reading the
    /// layer map there cannot recover the previous bounding box.
    pub deskbar_geometry: Option<Rect>,
}

impl OutputState {
    pub fn new(refresh_mhz: i32) -> OutputState {
        let refresh = if refresh_mhz > 0 {
            Duration::from_nanos(1_000_000_000_000 / refresh_mhz as u64)
        } else {
            Duration::from_nanos(16_666_667)
        };
        OutputState {
            redraw_queued: true,
            stats: FrameStats::default(),
            lock_surface: None,
            powered: true,
            frame_seq: 0,
            damaged_frames: 0,
            last_frame: None,
            presented: None,
            refresh,
            gamma: false,
            gamma_client: false,
            deskbar_geometry: None,
        }
    }
}

/// The session lock.
#[derive(Default)]
pub enum LockState {
    #[default]
    Unlocked,
    /// A client asked to lock; confirmed once every output shows its lock
    /// surface (or black).
    Locking(SessionLocker, Instant),
    Locked,
}

impl LockState {
    pub fn is_locked(&self) -> bool {
        !matches!(self, LockState::Unlocked)
    }
}

pub struct State {
    pub backend: Backend,
    pub az: Aizome,
    /// The compositor's logic: the loaded library and its state.
    pub lg: LogicHost,
}

// Protocol states live here to keep their globals alive; not all are read.
#[allow(dead_code)]
pub struct Aizome {
    pub dh: DisplayHandle,
    pub lh: LoopHandle<'static, State>,
    pub signal: LoopSignal,
    pub clock: Clock<Monotonic>,
    pub socket_name: OsString,
    pub start: Instant,
    pub options: crate::cli::Options,

    pub compositor: CompositorState,
    pub xdg_shell: XdgShellState,
    pub xdg_decoration: XdgDecorationState,
    pub xdg_dialog: XdgDialogState,
    pub layer_shell: WlrLayerShellState,
    pub shm: ShmState,
    pub dmabuf: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub seat_state: SeatState<State>,
    pub data_device: DataDeviceState,
    pub primary_selection: PrimarySelectionState,
    pub wlr_data_control: WlrDataControlState,
    pub ext_data_control: ExtDataControlState,
    pub output_manager: OutputManagerState,
    pub viewporter: ViewporterState,
    pub presentation: PresentationState,
    pub fractional_scale: FractionalScaleManagerState,
    pub xdg_activation: XdgActivationState,
    pub xdg_foreign: XdgForeignState,
    pub session_lock: SessionLockManagerState,
    pub idle_notifier: IdleNotifierState<State>,
    pub idle_inhibit: IdleInhibitManagerState,
    pub shortcuts_inhibit: KeyboardShortcutsInhibitState,
    pub pointer_constraints: PointerConstraintsState,
    pub relative_pointer: RelativePointerManagerState,
    pub pointer_gestures: PointerGesturesState,
    pub cursor_shape: CursorShapeManagerState,
    pub single_pixel: SinglePixelBufferState,
    pub text_input: TextInputManagerState,
    pub input_method: InputMethodManagerState,
    pub virtual_keyboard: VirtualKeyboardManagerState,
    pub security_context: SecurityContextState,
    pub content_type: ContentTypeState,
    pub tablet: TabletManagerState,
    pub system_bell: smithay::wayland::xdg_system_bell::XdgSystemBellState,
    pub alpha_modifier: AlphaModifierState,
    pub fifo: FifoManagerState,
    pub commit_timing: CommitTimingManagerState,
    /// What lets commits waiting on fifo barriers and times through.
    pub pacing: crate::pacing::Pacing,
    pub foreign_toplevel_list: ForeignToplevelListState,
    pub foreign_toplevel_mgmt: ForeignToplevelManagerState,
    pub screencopy: ScreencopyManagerState,
    pub output_management: OutputManagementState,

    pub seat: Seat<State>,
    pub popups: PopupManager,
    pub outputs: Vec<Output>,
    /// Outputs turned off: out of the layout, kept to turn on again.
    pub disabled_outputs: Vec<Output>,
    /// Outputs showing another's picture: (mirror, source). Out of the
    /// layout; the backend still drives them.
    pub mirrors: Vec<(Output, Output)>,
    /// The main display, as chosen for this session over IPC.
    pub primary_override: Option<String>,
    pub night: crate::night::Night,
    pub out: HashMap<Output, OutputState>,

    /// Every mapped toplevel, by its id.
    pub windows: HashMap<WinId, Mapped>,
    pub by_surface: HashMap<WlSurface, WinId>,
    pub unmapped: Vec<Unmapped>,
    next_win: WinId,
    /// What the logic last said about the desktop as a whole.
    pub view: View,
    /// Layer surfaces' last frames, kept while the logic moves them away.
    pub ghosts: Vec<crate::ghosts::Ghost>,
    pub next_ghost: u64,
    /// Window positions or visibility changed: surfaces' outputs need
    /// updating.
    pub window_outputs_dirty: bool,
    /// Outputs came, went or changed: the logic hears it before the next
    /// frame.
    pub outputs_dirty: bool,

    pub settings: Settings,
    /// The setting at session startup, independent of renderer overrides or
    /// fallback. Only later edits should request another login.
    pub startup_renderer: aizome_config::RendererChoice,
    pub palette: Palette,
    /// Ctrl+Alt+Del's overlay, drawn and run by the core so it works
    /// whatever the logic does.
    pub recovery: crate::recovery::Recovery,
    /// Text and app icons, for the core's overlays and the logic's
    /// decorations alike (kept across reloads).
    pub labels: crate::text::Labels,
    pub icons: crate::icons::Icons,
    pub ipc: IpcServer,

    pub cursor_status: CursorImageStatus,
    pub cursor_theme: CursorTheme,
    /// A cursor shown instead of the client's (resize arrows over frame
    /// edges, the arrow over tabs), set by the logic.
    pub cursor_override: Option<smithay::input::pointer::CursorIcon>,
    /// Where the pointer is (kept beside smithay's, which sits behind a lock
    /// held during pointer callbacks).
    pub pointer_pos: Point<f64, Logical>,
    /// Shown in the stack-drop feedback and the grab cursors.
    pub dnd_icon: Option<WlSurface>,
    /// Where the drag icon is, relative to the pointer.
    pub dnd_offset: Point<i32, Logical>,
    /// A layer surface that took keyboard focus on demand (by a click).
    pub layer_focus: Option<WlSurface>,
    /// The popup grab in force, if any.
    pub popup_grab: Option<crate::focus::PopupGrabbed>,
    /// Open popups in the order they opened (the popup layer's stacking).
    pub popup_order: Vec<smithay::desktop::PopupKind>,
    pub lock: LockState,
    /// Keys whose presses a binding swallowed, so their releases are
    /// swallowed too.
    pub suppressed_keys: HashSet<Keycode>,
    /// Command-key translation in force for these pressed keys.
    pub translated_keys: HashSet<Keycode>,
    /// Surfaces that keep the screen awake (idle-inhibit).
    pub idle_inhibitors: HashSet<WlSurface>,
    /// Activation tokens that may move focus.
    pub valid_tokens: HashSet<String>,
    pub idle: crate::idle::IdleState,
    /// What has the displays off: idleness, IPC, the laptop's lid.
    pub power: crate::power::Power,
    pub children: crate::session::Children,
    /// Buttons whose presses the seat was given and whose releases it
    /// hasn't been yet. The seat ends its grabs (a client's implicit grab,
    /// a drag and drop) only when it holds no button, so every press it
    /// sees must be followed by its release, whatever overlay comes up in
    /// between ([`State::forward_button`]).
    pub forwarded_buttons: HashSet<u32>,
}

impl State {
    /// Sets up the Wayland display and every global; the backend adds
    /// outputs afterwards.
    pub fn new(
        display: Display<State>,
        lh: LoopHandle<'static, State>,
        signal: LoopSignal,
        backend: Backend,
        options: crate::cli::Options,
        settings: Settings,
    ) -> State {
        let dh = display.handle();
        let clock = Clock::new();

        let socket = match &options.socket {
            Some(name) => ListeningSocketSource::with_name(name).expect("bind the Wayland socket"),
            None => ListeningSocketSource::new_auto().expect("bind a Wayland socket"),
        };
        let socket_name = socket.socket_name().to_os_string();
        lh.insert_source(socket, |stream, _, state| {
            if let Err(e) = state.az.dh.insert_client(stream, Arc::new(ClientState::default())) {
                warn!("couldn't add a Wayland client: {e}");
            }
        })
        .expect("listen for Wayland clients");
        lh.insert_source(Generic::new(display, Interest::READ, Mode::Level), |_, display, state| {
            // Safety: the display isn't dropped while its source is alive.
            unsafe {
                display.get_mut().dispatch_clients(state).unwrap();
            }
            Ok(PostAction::Continue)
        })
        .expect("dispatch Wayland clients");
        info!("listening on {}", socket_name.to_string_lossy());

        let compositor = CompositorState::new_v6::<State>(&dh);
        let xdg_shell = XdgShellState::new_with_capabilities::<State>(
            &dh,
            [
                smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities::Fullscreen,
                smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities::Maximize,
                smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities::Minimize,
                smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities::WindowMenu,
            ],
        );
        let xdg_decoration = XdgDecorationState::new::<State>(&dh);
        let xdg_dialog = XdgDialogState::new::<State>(&dh);
        let layer_shell = WlrLayerShellState::new_with_filter::<State, _>(&dh, client_is_privileged);
        let shm = ShmState::new::<State>(&dh, vec![]);
        let mut seat_state = SeatState::new();
        let data_device = DataDeviceState::new::<State>(&dh);
        let primary_selection = PrimarySelectionState::new::<State>(&dh);
        let wlr_data_control =
            WlrDataControlState::new::<State, _>(&dh, Some(&primary_selection), client_is_privileged);
        let ext_data_control =
            ExtDataControlState::new::<State, _>(&dh, Some(&primary_selection), client_is_privileged);
        let output_manager = OutputManagerState::new_with_xdg_output::<State>(&dh);
        let viewporter = ViewporterState::new::<State>(&dh);
        let presentation = PresentationState::new::<State>(&dh, clock.id() as u32);
        let fractional_scale = FractionalScaleManagerState::new::<State>(&dh);
        let xdg_activation = XdgActivationState::new::<State>(&dh);
        let xdg_foreign = XdgForeignState::new::<State>(&dh);
        let session_lock = SessionLockManagerState::new::<State, _>(&dh, client_is_privileged);
        let idle_notifier = IdleNotifierState::new(&dh, lh.clone());
        let idle_inhibit = IdleInhibitManagerState::new::<State>(&dh);
        let shortcuts_inhibit = KeyboardShortcutsInhibitState::new::<State>(&dh);
        let pointer_constraints = PointerConstraintsState::new::<State>(&dh);
        let relative_pointer = RelativePointerManagerState::new::<State>(&dh);
        let pointer_gestures = PointerGesturesState::new::<State>(&dh);
        let cursor_shape = CursorShapeManagerState::new::<State>(&dh);
        let single_pixel = SinglePixelBufferState::new::<State>(&dh);
        let text_input = TextInputManagerState::new::<State>(&dh);
        let input_method = InputMethodManagerState::new::<State, _>(&dh, client_is_privileged);
        let virtual_keyboard = VirtualKeyboardManagerState::new::<State, _>(&dh, client_is_privileged);
        let security_context = SecurityContextState::new::<State, _>(&dh, client_is_privileged);
        let content_type = ContentTypeState::new::<State>(&dh);
        let tablet = TabletManagerState::new::<State>(&dh);
        crate::protocols::toplevel_icon::create_global(&dh);
        let system_bell = smithay::wayland::xdg_system_bell::XdgSystemBellState::new::<State>(&dh);
        let alpha_modifier = AlphaModifierState::new::<State>(&dh);
        let fifo = FifoManagerState::new::<State>(&dh);
        let commit_timing = CommitTimingManagerState::new::<State>(&dh);
        let foreign_toplevel_list = ForeignToplevelListState::new_with_filter::<State>(&dh, client_is_privileged);
        let foreign_toplevel_mgmt = ForeignToplevelManagerState::new(&dh, client_is_privileged);
        let screencopy = ScreencopyManagerState::new(&dh, client_is_privileged);
        crate::protocols::gamma_control::create_global(&dh, client_is_privileged);
        let output_management = OutputManagementState::new(&dh, client_is_privileged);

        let mut seat = seat_state.new_wl_seat(&dh, backend.seat_name());
        seat.add_pointer();
        let k = &settings.input.keyboard;
        let layouts = k.layouts.join(",");
        let xkb = XkbConfig {
            rules: "",
            model: &k.model,
            layout: &layouts,
            variant: &k.variant,
            options: if k.options.is_empty() { None } else { Some(k.options.clone()) },
        };
        if let Err(e) = seat.add_keyboard(xkb, k.repeat_delay as i32, k.repeat_rate as i32) {
            warn!("keyboard layout {layouts} failed ({e:?}); using the default");
            seat.add_keyboard(XkbConfig::default(), k.repeat_delay as i32, k.repeat_rate as i32)
                .expect("a keyboard with the default layout");
        }
        // Settings' "Num Lock on at start".
        if k.numlock
            && let Some(kb) = seat.get_keyboard()
        {
            let mut mods = kb.modifier_state();
            mods.num_lock = true;
            kb.set_modifier_state(mods);
        }
        seat.add_touch();

        let palette = crate::config::palette_for(&settings);
        let cursor_theme = CursorTheme::load(&settings.appearance.cursor_theme, settings.appearance.cursor_size);
        let ipc = IpcServer::new();

        let az = Aizome {
            dh: dh.clone(),
            lh: lh.clone(),
            signal,
            clock,
            socket_name,
            start: Instant::now(),
            options,
            compositor,
            xdg_shell,
            xdg_decoration,
            xdg_dialog,
            layer_shell,
            shm,
            dmabuf: DmabufState::new(),
            dmabuf_global: None,
            seat_state,
            data_device,
            primary_selection,
            wlr_data_control,
            ext_data_control,
            output_manager,
            viewporter,
            presentation,
            fractional_scale,
            xdg_activation,
            xdg_foreign,
            session_lock,
            idle_notifier,
            idle_inhibit,
            shortcuts_inhibit,
            pointer_constraints,
            relative_pointer,
            pointer_gestures,
            cursor_shape,
            single_pixel,
            text_input,
            input_method,
            virtual_keyboard,
            security_context,
            content_type,
            tablet,
            system_bell,
            alpha_modifier,
            fifo,
            commit_timing,
            pacing: Default::default(),
            foreign_toplevel_list,
            foreign_toplevel_mgmt,
            screencopy,
            output_management,
            seat,
            popups: PopupManager::default(),
            outputs: Vec::new(),
            disabled_outputs: Vec::new(),
            mirrors: Vec::new(),
            primary_override: None,
            night: crate::night::Night::default(),
            out: HashMap::new(),
            windows: HashMap::new(),
            by_surface: HashMap::new(),
            unmapped: Vec::new(),
            next_win: 1,
            view: View::default(),
            ghosts: Vec::new(),
            next_ghost: 0,
            window_outputs_dirty: false,
            outputs_dirty: false,
            recovery: Default::default(),
            labels: {
                let mut l = crate::text::Labels::new();
                l.warm_up();
                l
            },
            icons: crate::icons::Icons::new(),
            ipc,
            palette,
            startup_renderer: settings.renderer_choice(),
            settings,
            cursor_status: CursorImageStatus::default_named(),
            cursor_override: None,
            pointer_pos: Point::from((0.0, 0.0)),
            cursor_theme,
            dnd_icon: None,
            dnd_offset: (0, 0).into(),
            layer_focus: None,
            popup_grab: None,
            popup_order: Vec::new(),
            lock: LockState::Unlocked,
            suppressed_keys: HashSet::new(),
            translated_keys: HashSet::new(),
            idle_inhibitors: HashSet::new(),
            valid_tokens: HashSet::new(),
            idle: Default::default(),
            power: Default::default(),
            children: Default::default(),
            forwarded_buttons: HashSet::new(),
        };
        State { backend, az, lg: LogicHost::default() }
    }

    /// Runs after every batch of events: tidies up, rolls a failed logic
    /// back, renders what is due, and sends everything queued to clients.
    pub fn on_loop_iteration(&mut self) {
        self.az.popups.cleanup();
        self.check_popup_grab();
        self.az.prune_popups();
        self.refresh_windows();
        // The safe point between frames: no logic code is on the stack.
        self.recover_logic();
        if std::mem::take(&mut self.az.outputs_dirty) {
            // An output that came while the displays are off, or a panel
            // behind the closed lid, goes dark before its first frame.
            self.apply_power();
            self.lg_call((), |vt, st| (vt.outputs_changed)(st));
        }
        self.lg_call((), |vt, st| (vt.loop_iteration)(st));
        self.lg_call((), |vt, st| (vt.publish)(st));
        self.az.refresh_window_outputs();
        self.az.ipc.flush();
        crate::protocols::foreign_toplevel::refresh(self);
        self.redraw_due();
        self.check_lock();
        crate::protocols::screencopy::flush_pending(self);
        if let Err(e) = self.az.dh.flush_clients() {
            warn!("flushing clients: {e}");
        }
    }

    /// Renders the outputs whose redraw is queued and due.
    pub fn redraw_due(&mut self) {
        let mut outputs: Vec<Output> = self.az.outputs.clone();
        outputs.extend(self.az.mirrors.iter().map(|(m, _)| m.clone()));
        let mut released = false;
        for output in outputs {
            let queued = self.az.out.get(&output).is_some_and(|o| o.redraw_queued);
            if !queued || !self.backend.frame_due(&mut self.az, &output) {
                continue;
            }
            // Timestamped commits due when this frame shows go in it.
            released |= self.release_timed(&output);
            // A mirror shows its source's picture, pointer and all.
            let source = self.az.mirror_source(&output);
            let started = Instant::now();
            let scene = match &source {
                Some(s) => self.scene(s, crate::scene::Purpose::Capture { cursor: true }),
                None => self.scene(&output, crate::scene::Purpose::Screen),
            };
            let clear = crate::render::clear_color(&self.az);
            self.backend.redraw(&mut self.az, &output, scene, clear, started);
            released |= self.release_cleared();
        }
        if released {
            // What came through draws next, maybe on an output already
            // passed over: come round again.
            self.az.signal.wakeup();
        }
    }

    /// Drops windows whose clients went away without saying so.
    fn refresh_windows(&mut self) {
        let dead: Vec<WinId> = self.az.windows.values().filter(|m| !m.toplevel().alive()).map(|m| m.id).collect();
        for id in dead {
            self.unmap_window(id);
        }
        self.az.unmapped.retain(|u| u.window.toplevel().is_some_and(|t| t.alive()));
    }

    /// Takes in what the logic says about the windows: each one's place
    /// and state, and the desktop's. Surfaces' outputs follow changes.
    pub fn publish_view(&mut self, view: View, windows: impl IntoIterator<Item = (WinId, WinView)>) {
        let mut moved = view.stacking != self.az.view.stacking;
        for (id, w) in windows {
            if let Some(m) = self.az.windows.get_mut(&id)
                && m.view != w
            {
                moved |= m.view.rect != w.rect || m.view.shown != w.shown;
                m.view = w;
            }
        }
        if moved {
            self.az.window_outputs_dirty = true;
        }
        self.az.view = view;
    }
}

impl Aizome {
    // ---- outputs ---------------------------------------------------------

    /// An output's rectangle in the global layout, in points.
    pub fn output_geometry(&self, output: &Output) -> Rectangle<i32, Logical> {
        let loc = output.current_location();
        let size = output
            .current_mode()
            .map(|m| {
                output
                    .current_transform()
                    .transform_size(m.size)
                    .to_f64()
                    .to_logical(output.current_scale().fractional_scale())
                    .to_i32_round()
            })
            .unwrap_or_else(|| Size::from((0, 0)));
        Rectangle::new(loc, size)
    }

    pub fn output_rect(&self, output: &Output) -> Rect {
        to_rect(self.output_geometry(output))
    }

    /// The area left for windows: the output minus layer-shell exclusive
    /// zones.
    pub fn usable_area(&self, output: &Output) -> Rect {
        let geo = self.output_geometry(output);
        let map = layer_map_for_output(output);
        let mut zone = map.non_exclusive_zone();
        zone.loc += geo.loc;
        to_rect(zone)
    }

    /// The Deskbar's rectangle on an output, if the shell shows one.
    pub fn deskbar_rect(&self, output: &Output) -> Option<Rect> {
        let geo = self.output_geometry(output);
        let map = layer_map_for_output(output);
        let layer = map.layers().find(|l| l.namespace() == "aizome-deskbar")?;
        let mut r = map.layer_geometry(layer)?;
        r.loc += geo.loc;
        Some(to_rect(r))
    }

    pub fn output_by_name(&self, name: &str) -> Option<Output> {
        self.outputs.iter().find(|o| o.name() == name).cloned()
    }

    /// The output containing a point, or the first.
    pub fn output_at(&self, p: Point<f64, Logical>) -> Option<Output> {
        self.outputs
            .iter()
            .find(|o| self.output_geometry(o).to_f64().contains(p))
            .or_else(|| self.outputs.first())
            .cloned()
    }

    /// The output showing most of a rectangle.
    pub fn output_for_rect(&self, r: Rect) -> Option<Output> {
        let best = self
            .outputs
            .iter()
            .map(|o| (o, self.output_rect(o).intersection(r).map(|i| i.area()).unwrap_or(0)))
            .max_by_key(|(_, a)| *a);
        match best {
            Some((o, a)) if a > 0 => Some(o.clone()),
            _ => self.output_at(Point::from((r.x as f64, r.y as f64))),
        }
    }

    /// Where new windows go: the output under the pointer.
    pub fn active_output(&self) -> Option<Output> {
        // Our copy of the pointer's place: asking the pointer from inside its
        // callbacks would deadlock on its lock.
        let p = self.pointer_pos;
        self.output_at(p)
    }

    pub fn queue_redraw(&mut self, output: &Output) {
        if let Some(o) = self.out.get_mut(output) {
            o.redraw_queued = true;
        }
    }

    pub fn queue_redraw_all(&mut self) {
        for o in self.out.values_mut() {
            o.redraw_queued = true;
        }
    }

    /// Queues a redraw on every output a rectangle (a window's content)
    /// touches, with room for its decorations.
    pub fn queue_redraw_rect(&mut self, r: Rect) {
        let r = r.grow(self.view.outset);
        let hit: Vec<Output> = self.outputs.iter().filter(|o| self.output_rect(o).intersects(r)).cloned().collect();
        for o in hit {
            self.queue_redraw(&o);
        }
    }

    // ---- windows -----------------------------------------------------------

    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<WinId> {
        self.by_surface.get(surface).copied()
    }

    /// A new window's id.
    pub fn next_window_id(&mut self) -> WinId {
        let id = self.next_win;
        self.next_win += 1;
        id
    }

    /// Where the logic last put a window (its content), if it shows.
    pub fn window_rect(&self, id: WinId) -> Option<Rect> {
        self.windows.get(&id).map(|m| m.view.rect)
    }

    /// A window's app id (what its client says).
    pub fn window_app(&self, id: WinId) -> Option<String> {
        self.windows.get(&id).map(|m| crate::shell::toplevel_ids(m.toplevel()).0)
    }

    /// The process behind a window (its client's credentials).
    pub fn window_pid(&self, id: WinId) -> Option<i32> {
        let m = self.windows.get(&id)?;
        use smithay::reexports::wayland_server::Resource;
        let client = self.dh.get_client(m.surface().id()).ok()?;
        client.get_credentials(&self.dh).ok().map(|c| c.pid)
    }

    /// An app's windows.
    pub fn windows_of_app(&self, app: &str) -> Vec<WinId> {
        let mut v: Vec<WinId> =
            self.windows.keys().copied().filter(|id| self.window_app(*id).as_deref() == Some(app)).collect();
        v.sort();
        v
    }

    /// Apps with windows, most recently used first, each with its windows
    /// in focus order (every window, on every Desk), as the logic last
    /// said.
    pub fn apps_mru(&self) -> Vec<(String, Vec<WinId>)> {
        let mut order: Vec<WinId> = self.view.mru.iter().copied().filter(|id| self.windows.contains_key(id)).collect();
        let mut rest: Vec<WinId> = self.windows.keys().copied().filter(|id| !order.contains(id)).collect();
        rest.sort();
        order.extend(rest);
        let mut apps: Vec<(String, Vec<WinId>)> = Vec::new();
        for id in order {
            let app = self.window_app(id).unwrap_or_default();
            match apps.iter_mut().find(|(a, _)| *a == app) {
                Some((_, ws)) => ws.push(id),
                None => apps.push((app, vec![id])),
            }
        }
        apps
    }

    /// Whether a window shows now (as the logic last said).
    pub fn window_shown(&self, id: WinId) -> bool {
        self.windows.get(&id).is_some_and(|m| m.view.shown)
    }

    /// Updates which outputs each window's surfaces are on.
    pub fn refresh_window_outputs(&mut self) {
        if !self.window_outputs_dirty {
            return;
        }
        self.window_outputs_dirty = false;
        for m in self.windows.values() {
            let shown = m.view.shown;
            let rect = m.view.rect;
            let geo = m.window.geometry();
            // The window's bounding box in global coordinates.
            let mut bbox = m.window.bbox_with_popups();
            bbox.loc += Point::from((rect.x, rect.y)) - geo.loc;
            for o in &self.outputs {
                let og = self.output_geometry(o);
                match bbox.intersection(og) {
                    Some(mut overlap) if shown => {
                        overlap.loc -= bbox.loc;
                        m.window.output_enter(o, overlap);
                    }
                    _ => m.window.output_leave(o),
                }
            }
        }
    }

    /// The palette follows the theme and focus colour; everything
    /// redraws (the logic rebuilds its labels when it hears).
    pub fn set_palette(&mut self, palette: Palette) {
        if palette != self.palette {
            self.palette = palette;
            self.queue_redraw_all();
        }
    }

    /// The current monotonic time, as smithay's frame callbacks want it.
    pub fn now(&self) -> Duration {
        Duration::from(self.clock.now())
    }

    pub fn motion_ms(&self) -> u32 {
        self.settings.appearance.motion_ms()
    }
}

pub fn to_rect(r: Rectangle<i32, Logical>) -> Rect {
    Rect::new(r.loc.x, r.loc.y, r.size.w, r.size.h)
}

pub fn from_rect(r: Rect) -> Rectangle<i32, Logical> {
    Rectangle::new((r.x, r.y).into(), (r.w, r.h).into())
}
