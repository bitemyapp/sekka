//! The lock screen: ext-session-lock surfaces on every output — the
//! wallpaper under a `kachi` scrim, the time in IBM Plex Mono 64pt and the
//! date, and on every output the user's picture or initial, their
//! name, the password field (a focus rail while focused) and the mark.
//! PAM runs on a thread; a failure shakes the field and says what PAM
//! said. The states are types ([`screen`]); the handlers here route the
//! shell's events to their transitions.

use std::time::Duration;

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Image};
use aizome_sys::power::SleepCycle;
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::shell::{Msg, Shell, Surface};
use crate::widgets::drawn::{around, drawn};
use crate::widgets::field::field;

pub mod screen;

pub use screen::LockState;
use screen::{Io, Stage};

/// Where locking is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Unlocked,
    /// Asked the compositor; its surfaces are open.
    Locking,
    Locked,
    /// Locked, and PAM is checking a password.
    Checking,
}

/// What the lock screen's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum LockMsg {
    Password(String),
    Submit,
    Caps(bool),
    /// PAM's answer to a check.
    Checked(screen::Checked),
    /// A step of the shake.
    Shake(u32),
    Escape,
}

/// The field's sideways offset at a step of the shake.
pub fn shake_offset(step: u32) -> f64 {
    const STEPS: [f64; 9] = [0.0, -10.0, 9.0, -7.0, 6.0, -4.0, 3.0, -1.0, 0.0];
    STEPS.get(step as usize).copied().unwrap_or(0.0)
}

/// Everything one output's lock surface shows.
#[derive(Clone)]
pub struct LockView {
    pub time: String,
    pub date: String,
    pub name: String,
    pub initial: String,
    pub picture: Option<Image>,
    pub password: String,
    pub busy: bool,
    /// What the last check said: only the lock screen's machine makes one.
    pub error: Option<screen::Note>,
    pub caps: bool,
    pub shake: f64,
    pub wallpaper: Option<Image>,
    pub size: Size,
    /// `[lock] show_clock`.
    pub show_clock: bool,
    /// Notifications that came while locked, newest first.
    pub notes: Vec<LockNote>,
}

/// A notification on the lock screen: whose, and (with previews on) what.
#[derive(Clone, Debug, PartialEq)]
pub struct LockNote {
    pub app: String,
    pub glyph: Icon,
    /// Empty without previews: "N notifications" stands in.
    pub summary: String,
    pub body: String,
    /// How many it stands for (without previews, one row per app).
    pub count: usize,
}

/// The most notification rows the lock screen shows.
pub const NOTES: usize = 3;
const NOTE_H: f64 = 46.0;
const NOTE_W: f64 = 320.0;

/// The rows for notifications that came while locked: each one with
/// previews, else one per app with how many.
pub fn lock_notes(notes: &[(String, Icon, String, String)], previews: bool) -> Vec<LockNote> {
    let mut out: Vec<LockNote> = Vec::new();
    for (app, glyph, summary, body) in notes {
        if previews {
            out.push(LockNote {
                app: app.clone(),
                glyph: *glyph,
                summary: summary.clone(),
                body: body.clone(),
                count: 1,
            });
        } else if let Some(n) = out.iter_mut().find(|n| n.app == *app) {
            n.count += 1;
        } else {
            out.push(LockNote {
                app: app.clone(),
                glyph: *glyph,
                summary: String::new(),
                body: String::new(),
                count: 1,
            });
        }
    }
    out.truncate(NOTES);
    out
}

fn note_card(n: &LockNote) -> Element<LockMsg> {
    let n2 = n.clone();
    let label = if n.summary.is_empty() {
        format!("{} {}", n.app, count_words(n.count))
    } else {
        format!("{} {}", n.app, n.summary)
    };
    drawn(NOTE_W, NOTE_H, (&n.app, &n.summary, &n.body, n.count), move |cx| {
        let c = aizome_kit::theme::color;
        let night = aizome_theme::NIGHT;
        let b = cx.bounds();
        let card = Rect::new(0.0, 0.0, b.x1, NOTE_H - 6.0);
        cx.canvas.fill_rect(card, c(aizome_theme::Rgba::hexa(0x1f2335, 0.85)));
        aizome_kit::draw::border(cx.canvas, card, c(night.line2), 1.0);
        n2.glyph.draw(cx.canvas, Point::new(10.0, 12.0), 16.0, c(night.text2));
        let ns = look::ui(12.5, typeface::WEIGHT_SEMIBOLD, c(night.text));
        let right = card.x1 - 10.0;
        if n2.summary.is_empty() {
            let w = cx.draw_text_in(&n2.app, &ns, Rect::new(34.0, 0.0, right, card.y1), Align::Start);
            let ms = look::ui(12.5, typeface::WEIGHT_REGULAR, c(night.text3));
            cx.draw_text_in(&count_words(n2.count), &ms, Rect::new(34.0 + w + 8.0, 0.0, right, card.y1), Align::Start);
        } else {
            let w = cx.draw_text_in(&n2.app, &ns, Rect::new(34.0, 3.0, right, 21.0), Align::Start);
            let ss = look::ui(12.5, typeface::WEIGHT_REGULAR, c(night.text));
            cx.draw_text_in(&n2.summary, &ss, Rect::new(34.0 + w + 8.0, 3.0, right, 21.0), Align::Start);
            let bs = look::ui(12.0, typeface::WEIGHT_REGULAR, c(night.text3));
            cx.draw_text_in(&n2.body, &bs, Rect::new(34.0, 20.0, right, card.y1 - 2.0), Align::Start);
        }
    })
    .text(label, SemanticRole::Label)
    .into()
}

fn count_words(n: usize) -> String {
    if n == 1 { "1 notification".to_owned() } else { format!("{n} notifications") }
}

const FIELD_W: f64 = 260.0;

pub fn view(v: &LockView) -> Element<LockMsg> {
    let size = v.size;
    let wall = v.wallpaper.clone();
    let ground = drawn(size.width, size.height, wall.as_ref().map(|i| i.pixel_size()), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, aizome_kit::theme::color(aizome_theme::Rgba::hex(0x16161e)));
        if let Some(img) = &wall {
            // In its pixels: it was scaled to the output already.
            let s = Size::new(img.pixel_size().0 as f64, img.pixel_size().1 as f64);
            if s.width > 0.0 && s.height > 0.0 {
                let k = (b.width() / s.width).max(b.height() / s.height);
                let (cw, ch) = (b.width() / k, b.height() / k);
                let src =
                    Rect::new((s.width - cw) / 2.0, (s.height - ch) / 2.0, (s.width + cw) / 2.0, (s.height + ch) / 2.0);
                cx.canvas.draw_image_with(img, Some(src), b, crate::wallpaper::options(img, b, cx.canvas.scale()));
            }
        }
        // The kachi scrim, at 70%.
        cx.canvas.fill_rect(b, aizome_kit::theme::color(aizome_theme::Rgba::hexa(0x16161e, 0.7)));
        let _ = t;
    });
    let (time, date) = (v.time.clone(), v.date.clone());
    let show_clock = v.show_clock;
    let clock = drawn(size.width, 120.0, (time.clone(), date.clone(), show_clock), move |cx| {
        if !show_clock {
            return;
        }
        let t = cx.theme();
        let b = cx.bounds();
        let ts = look::digits(64.0, typeface::WEIGHT_SEMIBOLD, aizome_kit::theme::color(aizome_theme::NIGHT.text));
        cx.draw_text_in(&time, &ts, Rect::new(0.0, 0.0, b.x1, 86.0), Align::Center);
        let ds = look::ui(17.0, typeface::WEIGHT_REGULAR, aizome_kit::theme::color(aizome_theme::NIGHT.text2));
        cx.draw_text_in(&date, &ds, Rect::new(0.0, 86.0, b.x1, 116.0), Align::Center);
        let _ = t;
    })
    .text(if show_clock { v.time.clone() } else { String::new() }, SemanticRole::Label);
    let mut column: Vec<Element<LockMsg>> = vec![clock.into()];
    {
        let (initial, picture, name) = (v.initial.clone(), v.picture.clone(), v.name.clone());
        let who = drawn(size.width, 120.0, (name.clone(), picture.is_some()), move |cx| {
            let b = cx.bounds();
            let tile = Rect::new(((b.width() - 72.0) / 2.0).round(), 8.0, ((b.width() + 72.0) / 2.0).round(), 80.0);
            match &picture {
                Some(p) => cx.canvas.draw_image(p, tile),
                None => {
                    cx.canvas.fill_rect(tile, aizome_kit::theme::color(aizome_theme::NIGHT.selection));
                    let s = look::ui(
                        34.0,
                        typeface::WEIGHT_SEMIBOLD,
                        aizome_kit::theme::color(aizome_theme::NIGHT.on_selection),
                    );
                    cx.draw_text_in(&initial, &s, tile, Align::Center);
                }
            }
            aizome_kit::draw::border(cx.canvas, tile, aizome_kit::theme::color(aizome_theme::NIGHT.bevel_lo), 1.0);
            let ns = look::ui(16.0, typeface::WEIGHT_TAB, aizome_kit::theme::color(aizome_theme::NIGHT.text));
            cx.draw_text_in(&name, &ns, Rect::new(0.0, 88.0, b.x1, 112.0), Align::Center);
        })
        .text(v.name.clone(), SemanticRole::Label);
        column.push(who.into());
        let f = field(&v.password, |_| {
            look::ui(15.0, typeface::WEIGHT_REGULAR, aizome_kit::theme::color(aizome_theme::NIGHT.text))
        })
        .secure()
        .placeholder(if v.busy { "Checking…" } else { "Password" })
        .height(32.0)
        .on_change(LockMsg::Password)
        .on_submit(LockMsg::Submit)
        .on_caps(LockMsg::Caps);
        let boxed = around(
            Element::from(f).id("lock-password"),
            [0.0, 10.0, 0.0, 12.0],
            Size::new(FIELD_W, 34.0),
            ("lock-field", v.busy),
            |cx| {
                let n = aizome_theme::NIGHT;
                let b = cx.bounds();
                let c = aizome_kit::theme::color;
                cx.canvas.fill_rect(b, c(n.sunk));
                aizome_kit::draw::border(cx.canvas, b, c(n.line2), 1.0);
                // The focus rail while it has the keyboard.
                cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), c(n.focus));
            },
        );
        let off = v.shake;
        let row = aizome_kit::container(boxed).padding([0.0, 0.0, 0.0, ((size.width - FIELD_W) / 2.0 + off).max(0.0)]);
        column.push(row.into());
        let msg = if let Some(e) = &v.error {
            Some((e.text().to_owned(), Hue::Red))
        } else if v.caps {
            Some(("Caps Lock is on".to_owned(), Hue::Amber))
        } else {
            None
        };
        let line = drawn(size.width, 30.0, msg.as_ref().map(|(m, _)| m.clone()), move |cx| {
            if let Some((m, hue)) = &msg {
                let night = aizome_kit::Theme::night();
                let s = look::ui(13.0, typeface::WEIGHT_REGULAR, hue.color(&night));
                cx.draw_text_in(m, &s, Rect::new(0.0, 8.0, cx.bounds().x1, 28.0), Align::Center);
            }
        });
        column.push(line.into());
        // What came while locked, under the field.
        for n in &v.notes {
            column.push(
                aizome_kit::container(note_card(n))
                    .padding([0.0, 0.0, 0.0, ((size.width - NOTE_W) / 2.0).max(0.0)])
                    .into(),
            );
        }
    }
    let notes_h = v.notes.len() as f64 * NOTE_H;
    let block_h = 120.0 + 120.0 + 34.0 + 30.0 + notes_h;
    let top = ((size.height - block_h) / 2.0 - 40.0).max(24.0);
    let mark = drawn(size.width, 40.0, (), move |cx| {
        let b = cx.bounds();
        let night = aizome_kit::Theme::night();
        look::mark(cx.canvas, &night, Point::new(((b.width() - 24.0) / 2.0).round(), 8.0), 24.0);
    });
    let content = aizome_kit::column(column).padding([top, 0.0, 0.0, 0.0]);
    aizome_kit::stack(vec![
        ground.into(),
        content.into(),
        aizome_kit::container(mark).padding([size.height - 72.0, 0.0, 0.0, 0.0]).into(),
    ])
    .width(size.width)
    .height(size.height)
    .into()
}

impl Shell {
    /// The lock's state, and what its transitions act through.
    fn lock_parts<'a, 'c>(&'a mut self, cx: &'a mut Cx<'c, Msg>) -> (&'a mut LockState, Io<'a, 'c>) {
        (&mut self.lock, Io::new(cx, self.sys.logind.as_ref(), &mut self.surfaces))
    }

    /// Menus, panels and the palette go, and a drag of the Deskbar's edge
    /// or grip ends where it began: nothing but the lock shows.
    fn put_away_for_lock(&mut self, cx: &mut Cx<Msg>) {
        self.close_palette(cx);
        self.close_panel(cx);
        self.end_deskbar_drags(cx);
        for s in self.screens.clone() {
            if let Some(b) = s.bar {
                cx.close_menus(b);
            }
        }
    }

    /// Lock with a complete, keyboard-ready prompt on every enabled output.
    pub fn lock(&mut self, cx: &mut Cx<Msg>) {
        if self.lock.phase() != Phase::Unlocked {
            return;
        }
        self.put_away_for_lock(cx);
        let (lock, mut io) = self.lock_parts(cx);
        lock.with_typed(|state| {
            let next = match state {
                Stage::Unlocked(unlocked) => unlocked.lock(&mut io).into(),
                state @ (Stage::Locking(_) | Stage::Locked(_) | Stage::Checking(_)) => state,
            };
            (next, ())
        });
        self.sync_lock_surfaces(cx);
    }

    /// logind announced a sleep: lock first, if `[lock] before_sleep`
    /// says so, and let it sleep once the lock is confirmed.
    ///
    /// Every sleep goes here: the lock decides when it may go (at once
    /// when already locked or not locking for sleep, else at the lock's
    /// confirmation or end). Code that routes logind's events must not
    /// give a sleep back itself (`Logind::sleep_ready`): while the lock
    /// isn't confirmed yet, that lets the machine sleep unlocked.
    pub(crate) fn lock_for_sleep(&mut self, sleep: SleepCycle, cx: &mut Cx<Msg>) {
        let before_sleep = self.lock_cfg.before_sleep;
        if before_sleep && self.lock.phase() == Phase::Unlocked {
            self.put_away_for_lock(cx);
        }
        let (lock, mut io) = self.lock_parts(cx);
        lock.with_typed(|state| {
            let next = match state {
                Stage::Unlocked(unlocked) if before_sleep => unlocked.lock_for_sleep(sleep, &mut io).into(),
                Stage::Unlocked(unlocked) => {
                    unlocked.let_sleep(sleep, &io);
                    unlocked.into()
                }
                // Not confirmed yet: the sleep waits for it.
                Stage::Locking(locking) => locking.sleep(sleep).into(),
                Stage::Locked(locked) => {
                    locked.let_sleep(sleep, &io);
                    locked.into()
                }
                Stage::Checking(checking) => {
                    checking.let_sleep(sleep, &io);
                    checking.into()
                }
            };
            (next, ())
        });
        self.sync_lock_surfaces(cx);
    }

    /// The machine woke: a sleep that waited for the lock is over.
    pub(crate) fn lock_resumed(&mut self) {
        self.lock.with_typed(|state| {
            let next = match state {
                Stage::Locking(locking) => locking.resumed().into(),
                state @ (Stage::Unlocked(_) | Stage::Locked(_) | Stage::Checking(_)) => state,
            };
            (next, ())
        });
    }

    /// Follow Wayland's advertised outputs, including changes while asleep
    /// or still waiting for lock confirmation. Disabled outputs aren't
    /// advertised, and mirrors display their source's lock surface.
    pub fn sync_lock_surfaces(&mut self, cx: &mut Cx<Msg>) {
        let Some(lock_surfaces) = self.lock.surfaces_mut() else { return };
        lock_surfaces.retain(|(key, output)| {
            if self.screens.iter().any(|s| s.output.id == *output) {
                true
            } else {
                cx.close_window(*key);
                self.surfaces.remove(key);
                false
            }
        });
        for screen in &self.screens {
            let output = screen.output.id;
            if lock_surfaces.iter().any(|(_, id)| *id == output) {
                continue;
            }
            let key = cx.open_window(WindowSpec::lock(output));
            self.surfaces.insert(key, Surface::Lock { output });
            lock_surfaces.push((key, output));
            // Focus each new window immediately: a single focus after the
            // loop only prepares the last one, which may not get the keys.
            cx.focus("lock-password");
        }
    }

    /// The compositor locked the session.
    pub fn lock_confirmed(&mut self, cx: &mut Cx<Msg>) {
        let (lock, mut io) = self.lock_parts(cx);
        lock.with_typed(|state| {
            let next = match state {
                Stage::Locking(locking) => locking.confirmed(&mut io).into(),
                state @ (Stage::Unlocked(_) | Stage::Locked(_) | Stage::Checking(_)) => state,
            };
            (next, ())
        });
    }

    /// The lock ended without the shell unlocking (the compositor refused
    /// or ended it).
    pub fn lock_ended(&mut self, cx: &mut Cx<Msg>) {
        let (lock, mut io) = self.lock_parts(cx);
        lock.with_typed(|state| {
            let next = match state {
                Stage::Unlocked(unlocked) => unlocked,
                Stage::Locking(locking) => locking.ended(&mut io),
                Stage::Locked(locked) => locked.ended(&mut io),
                Stage::Checking(checking) => checking.ended(&mut io),
            };
            (next.into(), ())
        });
    }

    /// Unlock (logind asked), or withdraw a lock not confirmed yet.
    pub fn unlock(&mut self, cx: &mut Cx<Msg>) {
        let (lock, mut io) = self.lock_parts(cx);
        lock.with_typed(|state| {
            let next = match state {
                Stage::Unlocked(unlocked) => unlocked,
                Stage::Locking(locking) => locking.unlock(&mut io),
                Stage::Locked(locked) => locked.unlock(&mut io),
                Stage::Checking(checking) => checking.unlock(&mut io),
            };
            (next.into(), ())
        });
    }

    pub fn lock_msg(&mut self, m: LockMsg, cx: &mut Cx<Msg>) {
        match m {
            // Typing is ignored while PAM checks.
            LockMsg::Password(p) => self.lock_typed(p),
            LockMsg::Escape => self.lock_typed(String::new()),
            LockMsg::Caps(on) => self.lock.set_caps(on),
            LockMsg::Submit => {
                let user = self.user.0.clone();
                let auth = self.authenticator.clone();
                let (lock, mut io) = self.lock_parts(cx);
                lock.with_typed(|state| {
                    let next = match state {
                        Stage::Locked(locked) => locked.submit(&user, auth, &mut io).into(),
                        state @ (Stage::Unlocked(_) | Stage::Locking(_) | Stage::Checking(_)) => state,
                    };
                    (next, ())
                });
            }
            LockMsg::Checked(heard) => {
                let heard = match self.lock.check(heard) {
                    Ok(heard) => heard,
                    Err(stale) => {
                        log::debug!("unlocking: dropped an answer to an earlier check, {stale:?}");
                        return;
                    }
                };
                let motion = self.motion();
                let (lock, mut io) = self.lock_parts(cx);
                lock.with_typed(|state| {
                    let next = match state {
                        Stage::Checking(checking) => match checking.pam_done(heard, motion, &mut io) {
                            Ok(answered) => answered.into(),
                            Err(checking) => checking.into(),
                        },
                        state @ (Stage::Unlocked(_) | Stage::Locking(_) | Stage::Locked(_)) => state,
                    };
                    (next, ())
                });
            }
            LockMsg::Shake(step) => {
                self.lock.set_shake(if step >= 9 { 0 } else { step });
                if step < 9 {
                    cx.after(Duration::from_millis(30), Msg::Lock(LockMsg::Shake(step + 1)));
                }
            }
        }
    }

    /// The field's text changed (or Escape cleared it).
    fn lock_typed(&mut self, password: String) {
        self.lock.with_typed(|state| {
            let next = match state {
                Stage::Locking(locking) => locking.typed(password).into(),
                Stage::Locked(locked) => locked.typed(password).into(),
                state @ (Stage::Unlocked(_) | Stage::Checking(_)) => state,
            };
            (next, ())
        });
    }

    /// Whether things may move: not in Austere, not with reduced motion.
    pub fn motion(&self) -> bool {
        let a = &self.settings.appearance;
        !a.reduced_motion && a.effects != aizome_theme::Effects::Austere
    }

    /// The notifications that came since the screen locked, as the lock
    /// screen shows them (`[notifications] previews_when_locked`).
    pub fn lock_notes(&self) -> Vec<LockNote> {
        let notes: Vec<(String, Icon, String, String)> = self
            .notes
            .unread_since(self.lock.since())
            .into_iter()
            .map(|n| {
                let (name, glyph, _) = self.app_look(&n.filed_under);
                (name, glyph, n.summary.clone(), n.first_line().to_owned())
            })
            .collect();
        lock_notes(&notes, self.notes_cfg.previews_when_locked)
    }

    pub fn lock_view(&self, output: u32) -> Element<Msg> {
        let Some(screen) = self.screens.iter().find(|s| s.output.id == output) else { return aizome_kit::space(0.0) };
        let name = self.user.1.clone();
        let picture = self.user_picture.clone();
        let v = LockView {
            time: self.now.time_label(self.bar_cfg.clock, false),
            date: self.now.date_label(),
            initial: name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(),
            name,
            picture,
            password: self.lock.password().to_owned(),
            busy: self.lock.phase() == Phase::Checking,
            error: self.lock.note().cloned(),
            caps: self.lock.caps(),
            shake: shake_offset(self.lock.shake()),
            wallpaper: self.wallpaper_for(output).map(|(i, _)| i),
            size: screen.output.frame.size(),
            show_clock: self.lock_cfg.show_clock,
            notes: self.lock_notes(),
        };
        view(&v).map(Msg::Lock)
    }
}

/// The glyph a lock screen's error would use.
pub fn error_glyph() -> Icon {
    Icon::CircleAlert
}
