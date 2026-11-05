//! The authentication dialog: polkit's agent (`aizome-sys`) asks, and the
//! shell shows an overlay dialog with the mark, the action's message, who
//! authenticates (a pop-up if several may), the password, PAM's prompts and
//! errors inline, Cancel and Authenticate (Try Again once the agent has
//! stopped trying). Its states are in [`dialog`].

pub mod dialog;

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{KeyboardInteractivity, Layer, LayerOptions, button, dropdown};
use aizome_sys::polkit::{AuthEvent, AuthRequest, Heard, RequestId};
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::{around, drawn};
use crate::widgets::field::field;
use dialog::{Dialog, DialogAny};

pub const WIDTH: f64 = 440.0;
pub const HEIGHT: f64 = 300.0;
pub const SHADOW: f64 = 3.0;

/// What polkit's agent says, in a form messages can carry.
#[derive(Clone, Debug)]
pub enum AuthIn {
    Begin(AuthRequest),
    Heard(Heard),
    Done { request: RequestId },
}

impl From<AuthEvent> for AuthIn {
    fn from(e: AuthEvent) -> AuthIn {
        match e {
            AuthEvent::Begin(r) => AuthIn::Begin(r),
            AuthEvent::Heard(heard) => AuthIn::Heard(heard),
            AuthEvent::Done { request, .. } => AuthIn::Done { request },
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

/// What the dialog's default button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    /// Authenticate: answer the prompt (or the first one, if it hasn't
    /// come yet).
    Answer,
    /// Authenticate, dimmed: an answer is being checked, or the agent is
    /// about to try again.
    Wait,
    /// Try Again: the agent stopped trying.
    TryAgain,
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
    pub submit: Submit,
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
    let (action, press) = match v.submit {
        Submit::Answer => ("Authenticate", Some(AuthMsg::Authenticate)),
        Submit::Wait => ("Authenticate", None),
        Submit::TryAgain => ("Try Again", Some(AuthMsg::Authenticate)),
    };
    let buttons = aizome_kit::row(vec![
        aizome_kit::spacer().into(),
        button("Cancel").cancel().on_press(AuthMsg::Cancel).into(),
        button(action).default().on_press_maybe(press).into(),
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
        self.auth.as_ref().map(Dialog::view)
    }

    pub fn auth_in(&mut self, e: AuthIn, cx: &mut Cx<Msg>) {
        match e {
            AuthIn::Begin(request) => {
                if let Some(old) = self.auth.take() {
                    self.close_auth_window(old.cancel(), cx);
                }
                let Some(output) = self.focused_screen().map(|s| s.output.id) else {
                    request.cancel();
                    return;
                };
                let window = self.open_auth_window(output, cx);
                self.auth = Some(Dialog::begin(request, window));
            }
            AuthIn::Heard(heard) => {
                let Some(dialog) = &mut self.auth else { return };
                // Another request's, or about a conversation since superseded.
                let Ok(fresh) = dialog.check(heard) else { return };
                let asked = dialog.with_typed(|s| {
                    let next = s.heard(fresh);
                    let asked = match &next {
                        DialogAny::Prompted(_) => true,
                        DialogAny::Starting(_)
                        | DialogAny::Eager(_)
                        | DialogAny::Checking(_)
                        | DialogAny::Waiting(_)
                        | DialogAny::Unavailable(_) => false,
                    };
                    (next, asked)
                });
                if asked {
                    cx.focus("auth-password");
                }
            }
            AuthIn::Done { request } => {
                if self.auth.as_ref().is_some_and(|d| d.id() == request) {
                    let dialog = self.auth.take().map(Dialog::close);
                    self.close_auth_window(dialog.flatten(), cx);
                }
            }
        }
    }

    pub fn auth_msg(&mut self, m: AuthMsg, cx: &mut Cx<Msg>) {
        let Some(dialog) = &mut self.auth else { return };
        match m {
            AuthMsg::Identity(i) => dialog.with_typed(|s| (s.choose(i), ())),
            AuthMsg::Password(p) => dialog.type_password(p),
            AuthMsg::Authenticate => dialog.with_typed(|s| {
                let next = match s {
                    DialogAny::Starting(s) => s.submit().into(),
                    DialogAny::Prompted(s) => s.submit().into(),
                    DialogAny::Unavailable(s) => s.try_again().into(),
                    // Checking an answer, or the agent about to try again.
                    s @ (DialogAny::Eager(_) | DialogAny::Checking(_) | DialogAny::Waiting(_)) => s,
                };
                (next, ())
            }),
            AuthMsg::Cancel => {
                if let Some(dialog) = self.auth.take() {
                    self.close_auth_window(dialog.cancel(), cx);
                }
            }
        }
    }

    /// The dialog's overlay on `output`.
    fn open_auth_window(&mut self, output: u32, cx: &mut Cx<Msg>) -> WindowKey {
        let opts =
            LayerOptions::new(Layer::Overlay, "aizome-auth").keyboard(KeyboardInteractivity::Exclusive).output(output);
        let s = size();
        let window = cx.open_window(layer(opts, s.width, s.height, "Authenticate"));
        self.surfaces.insert(window, Surface::Auth);
        cx.focus("auth-password");
        window
    }

    fn close_auth_window(&mut self, window: Option<WindowKey>, cx: &mut Cx<Msg>) {
        if let Some(window) = window {
            cx.close_window(window);
            self.surfaces.remove(&window);
        }
    }

    /// The compositor took a window away (its output went): if it was the
    /// dialog's, the dialog shows again on a screen there is, once the kit
    /// has let the old one go.
    pub fn auth_window_closed(&mut self, window: WindowKey, cx: &mut Cx<Msg>) {
        if let Some(dialog) = &mut self.auth
            && dialog.window() == Some(window)
        {
            dialog.window_gone();
            cx.send(Msg::Nothing);
        }
    }

    /// Gives a dialog that lost its window another, on the focused screen.
    /// Without a screen it waits for one: polkitd's request stays pending.
    pub fn sync_auth(&mut self, cx: &mut Cx<Msg>) {
        if self.auth.as_ref().is_none_or(|d| d.window().is_some()) {
            return;
        }
        let Some(output) = self.focused_screen().map(|s| s.output.id) else { return };
        let window = self.open_auth_window(output, cx);
        if let Some(dialog) = &mut self.auth {
            dialog.shown_in(window);
        }
    }
}
