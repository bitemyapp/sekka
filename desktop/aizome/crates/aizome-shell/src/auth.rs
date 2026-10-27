//! The authentication dialog: polkit's agent (`aizome-sys`) asks, and the
//! shell shows an overlay dialog with the mark, the action's message, who
//! authenticates (a pop-up if several may), the password, PAM's prompts and
//! errors inline, Cancel and Authenticate.

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{KeyboardInteractivity, Layer, LayerOptions, button, dropdown};
use aizome_sys::polkit::{AuthEvent, AuthRequest};
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::{around, drawn};
use crate::widgets::field::field;

pub const WIDTH: f64 = 440.0;
pub const HEIGHT: f64 = 300.0;
pub const SHADOW: f64 = 3.0;

/// What polkit's agent says, in a form messages can carry.
#[derive(Clone, Debug)]
pub enum AuthIn {
    Begin(AuthRequest),
    Prompt { cookie: String, text: String, echo: bool },
    Error { cookie: String, text: String },
    Info { cookie: String, text: String },
    Failed { cookie: String },
    Done { cookie: String },
}

impl From<AuthEvent> for AuthIn {
    fn from(e: AuthEvent) -> AuthIn {
        match e {
            AuthEvent::Begin(r) => AuthIn::Begin(r),
            AuthEvent::Prompt { cookie, text, echo } => AuthIn::Prompt { cookie, text, echo },
            AuthEvent::Error { cookie, text } => AuthIn::Error { cookie, text },
            AuthEvent::Info { cookie, text } => AuthIn::Info { cookie, text },
            AuthEvent::Failed { cookie } => AuthIn::Failed { cookie },
            AuthEvent::Done { cookie, .. } => AuthIn::Done { cookie },
        }
    }
}

/// What the dialog's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthMsg {
    Identity(usize),
    Password(String),
    Authenticate,
    Cancel,
}

/// The dialog while it's open.
#[derive(Clone, Debug)]
pub struct AuthState {
    pub window: WindowKey,
    pub request: AuthRequest,
    pub identity: usize,
    /// PAM's prompt ("Password:") and whether what's typed shows.
    pub prompt: Option<(String, bool)>,
    pub password: String,
    pub info: Option<String>,
    pub error: Option<String>,
    pub busy: bool,
}

/// What the dialog shows (testable without polkit).
#[derive(Clone, Debug, PartialEq)]
pub struct AuthView {
    pub message: String,
    pub identities: Vec<String>,
    pub identity: usize,
    pub prompt: String,
    pub echo: bool,
    pub password: String,
    pub note: Option<(String, bool)>,
    pub busy: bool,
}

pub fn view(v: &AuthView) -> Element<AuthMsg> {
    let message = v.message.clone();
    let head = drawn(WIDTH - 48.0, 76.0, message.clone(), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::mark(cx.canvas, t, Point::new(0.0, 2.0), 32.0);
        let ts = look::ui(15.0 * t.metrics.factor, typeface::WEIGHT_TAB, t.text());
        cx.draw_text_in("Authentication Required", &ts, Rect::new(48.0, 0.0, b.x1, 22.0), Align::Start);
        let mut ms = look::ui(13.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text2());
        ms.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        ms.max_lines = 3;
        let l = cx.layout_text(&message, &ms, Some(b.width() - 48.0));
        cx.canvas.draw_text(&l, Point::new(48.0, 26.0));
    })
    .text(v.message.clone(), SemanticRole::Label);
    let who: Element<AuthMsg> = if v.identities.len() > 1 {
        Element::from(dropdown(v.identities.clone(), v.identity).on_select(AuthMsg::Identity))
            .width(aizome_kit::Length::Fill)
    } else {
        let n = v.identities.first().cloned().unwrap_or_default();
        drawn(WIDTH - 48.0, 24.0, n.clone(), move |cx| {
            let t = cx.theme();
            look::ui(13.0, typeface::WEIGHT_SEMIBOLD, t.text());
            cx.draw_text_in(
                &n,
                &look::ui(13.0 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, t.text()),
                cx.bounds(),
                Align::Start,
            );
        })
        .into()
    };
    let prompt = v.prompt.clone();
    let label = drawn(WIDTH - 48.0, 20.0, prompt.clone(), move |cx| {
        let t = cx.theme();
        cx.draw_text_in(
            &prompt,
            &look::ui(12.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text3()),
            cx.bounds(),
            Align::Start,
        );
    });
    let mut f = field(&v.password, |t| look::ui(14.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text()))
        .placeholder(if v.busy { "Checking…" } else { "" })
        .height(28.0)
        .on_change(AuthMsg::Password)
        .on_submit(AuthMsg::Authenticate);
    if !v.echo {
        f = f.secure();
    }
    let boxed = around(
        Element::from(f).id("auth-password"),
        [0.0, 10.0, 0.0, 12.0],
        Size::new(WIDTH - 48.0, 30.0),
        "auth-field",
        |cx| {
            let t = cx.theme();
            let b = cx.bounds();
            aizome_kit::draw::sunken(cx.canvas, t, b, t.surface(), t.line2());
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), t.focus());
        },
    );
    let note = v.note.clone();
    let note_el = drawn(WIDTH - 48.0, 22.0, note.clone(), move |cx| {
        let t = cx.theme();
        if let Some((n, err)) = &note {
            let c = if *err { Hue::Red.color(t) } else { t.text3() };
            cx.draw_text_in(
                n,
                &look::ui(12.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, c),
                cx.bounds(),
                Align::Start,
            );
        }
    });
    let buttons = aizome_kit::row(vec![
        aizome_kit::spacer().into(),
        button("Cancel").cancel().on_press(AuthMsg::Cancel).into(),
        button("Authenticate").default().on_press_maybe((!v.busy).then_some(AuthMsg::Authenticate)).into(),
    ])
    .spacing(8.0);
    let content = aizome_kit::column(vec![
        head.into(),
        who,
        aizome_kit::space(8.0),
        label.into(),
        boxed.into(),
        note_el.into(),
        aizome_kit::spacer().into(),
        buttons.into(),
    ])
    .spacing(4.0)
    .padding(24.0)
    .width(WIDTH)
    .height(HEIGHT);
    let framed = around(content, 0.0, Size::new(WIDTH, HEIGHT), "auth-frame", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::panel(cx.canvas, t, b);
        // The focus colour along the top, as a focused frame's rail.
        cx.canvas.fill_rect(Rect::new(1.0, 1.0, b.x1 - 1.0, 3.0), t.focus());
    });
    let keyed = crate::widgets::keys::keys(framed, |k| {
        matches!(k.key, aizome_kit::sidestep_ui::Key::Named(aizome_kit::sidestep_ui::NamedKey::Escape))
            .then_some(AuthMsg::Cancel)
    });
    aizome_kit::container(keyed).padding([0.0, SHADOW, SHADOW, 0.0]).into()
}

pub fn size() -> Size {
    Size::new(WIDTH + SHADOW, HEIGHT + SHADOW)
}

impl Shell {
    /// Serve polkit's agent for this session (in an Aizome session only),
    /// off the UI thread. The shell owns the handle; it follows polkit
    /// restarts and releases its registration when the shell goes away.
    pub fn start_polkit(&mut self) {
        if !crate::shell::real_session() {
            return;
        }
        let Some(tx) = self.sender() else { return };
        std::thread::Builder::new()
            .name("polkit".into())
            .spawn(move || {
                let t = tx.clone();
                let agent = aizome_sys::polkit::PolkitAgent::register(move |e| {
                    t.send(Msg::Auth(e.into()));
                })
                .map(std::sync::Arc::new)
                .map_err(|e| e.to_string());
                tx.send(Msg::PolkitReady(agent));
            })
            .ok();
    }

    pub fn auth_view(&self) -> Option<AuthView> {
        let a = self.auth.as_ref()?;
        let identities: Vec<String> =
            a.request.identities.iter().map(|i| format!("{} ({})", i.display_name(), i.name)).collect();
        let (prompt, echo) = a.prompt.clone().unwrap_or_else(|| ("Password:".into(), false));
        let note = a.error.clone().map(|e| (e, true)).or_else(|| a.info.clone().map(|i| (i, false)));
        Some(AuthView {
            message: a.request.message.clone(),
            identities,
            identity: a.identity,
            prompt,
            echo,
            password: a.password.clone(),
            note,
            busy: a.busy,
        })
    }

    pub fn auth_in(&mut self, e: AuthIn, cx: &mut Cx<Msg>) {
        let same = |a: &Option<AuthState>, c: &str| a.as_ref().is_some_and(|a| a.request.cookie == c);
        match e {
            AuthIn::Begin(request) => {
                if let Some(old) = self.auth.take() {
                    old.request.cancel();
                    cx.close_window(old.window);
                    self.surfaces.remove(&old.window);
                }
                let Some(output) = self.focused_screen().map(|s| s.output.id) else {
                    request.cancel();
                    return;
                };
                let opts = LayerOptions::new(Layer::Overlay, "aizome-auth")
                    .keyboard(KeyboardInteractivity::Exclusive)
                    .output(output);
                let s = size();
                let window = cx.open_window(layer(opts, s.width, s.height, "Authenticate"));
                self.surfaces.insert(window, Surface::Auth);
                let identity = request
                    .default_identity()
                    .and_then(|d| request.identities.iter().position(|i| i == d))
                    .unwrap_or(0);
                if let Some(i) = request.identities.get(identity) {
                    request.authenticate(i);
                }
                self.auth = Some(AuthState {
                    window,
                    request,
                    identity,
                    prompt: None,
                    password: String::new(),
                    info: None,
                    error: None,
                    busy: false,
                });
                cx.focus("auth-password");
            }
            AuthIn::Prompt { cookie, text, echo } if same(&self.auth, &cookie) => {
                let a = self.auth.as_mut().expect("open");
                a.prompt = Some((text, echo));
                a.busy = false;
                cx.focus("auth-password");
            }
            AuthIn::Error { cookie, text } if same(&self.auth, &cookie) => {
                let a = self.auth.as_mut().expect("open");
                a.error = Some(text);
                a.busy = false;
            }
            AuthIn::Info { cookie, text } if same(&self.auth, &cookie) => {
                self.auth.as_mut().expect("open").info = Some(text);
            }
            AuthIn::Failed { cookie } if same(&self.auth, &cookie) => {
                let a = self.auth.as_mut().expect("open");
                a.busy = false;
                a.password.clear();
                a.error = Some("That didn't work. Try again.".into());
                if let Some(i) = a.request.identities.get(a.identity) {
                    a.request.authenticate(i);
                }
            }
            AuthIn::Done { cookie } if same(&self.auth, &cookie) => self.close_auth(cx),
            _ => {}
        }
    }

    pub fn auth_msg(&mut self, m: AuthMsg, cx: &mut Cx<Msg>) {
        let Some(a) = &mut self.auth else { return };
        match m {
            AuthMsg::Identity(i) => {
                a.identity = i;
                a.error = None;
                if let Some(id) = a.request.identities.get(i) {
                    a.request.authenticate(id);
                }
            }
            AuthMsg::Password(p) => a.password = p,
            AuthMsg::Authenticate => {
                if a.busy {
                    return;
                }
                a.busy = true;
                a.error = None;
                let pw = std::mem::take(&mut a.password);
                a.request.respond(&pw);
            }
            AuthMsg::Cancel => {
                a.request.cancel();
                self.close_auth(cx);
            }
        }
    }

    fn close_auth(&mut self, cx: &mut Cx<Msg>) {
        if let Some(a) = self.auth.take() {
            cx.close_window(a.window);
            self.surfaces.remove(&a.window);
        }
    }
}
