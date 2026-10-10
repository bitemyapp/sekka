// SPDX-License-Identifier: MIT OR Apache-2.0
//! Running an operation: a dialog with what Nix is doing right now, how far
//! along it is, and its log, which closes itself when everything worked and
//! stays with the error when it did not.
//!
//! The operation's steps run in a thread of their own. What the dialog
//! shows comes from there now and then, with only the log lines that are
//! new, so a busy build never keeps the window from answering.
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;
use yukimi_system::log::Progress;
use yukimi_system::nix;

use super::Ctx;
use crate::ops::Operation;

/// What the dialog shows, taken from the progress now and then.
struct Snapshot {
    current: Option<String>,
    summary: String,
    fraction: Option<f64>,
    /// Lines logged since the last snapshot.
    lines: Vec<String>,
}

enum Event {
    /// A step starts a command; `as_root` when it asks for permission first.
    Started {
        as_root: bool,
    },
    Progress(Snapshot),
    Done(Result<String, String>),
}

/// How long Nix may say nothing before the dialog mentions it.
const QUIET: Duration = Duration::from_secs(180);
/// How often the dialog hears how things are going.
const EVERY: Duration = Duration::from_millis(150);
/// How many log lines the dialog keeps.
const KEPT_LINES: i32 = 1000;

/// Words for failures a person can act on.
fn explain(error: &str, system: bool, stopped: bool) -> String {
    if error.contains("dismissed") || error.contains("Not authorized") || error.contains("126") {
        return "Cancelled. Nothing was changed.".to_owned();
    }
    if error.contains("must be setuid root") || error.contains("pkexec\": No such file") {
        return "This system can't ask for an administrator password the way Yukimi does: pkexec isn't set up. \
                Yukimi's NixOS module sets it up (programs.yukimi.enable = true). Nothing was changed."
            .to_owned();
    }
    if stopped {
        return if system {
            "Stopped. Your configuration was put back as it was, and the running system was not changed.".to_owned()
        } else {
            "Stopped. Nothing was changed.".to_owned()
        };
    }
    if system {
        format!("{error}\n\nYour configuration was put back as it was, and the running system was not changed.")
    } else {
        error.to_owned()
    }
}

/// What to show of the progress, with the lines logged since `seen`.
fn snapshot(progress: &Progress, seen: &mut u64) -> Snapshot {
    let lines = progress.since(*seen).map(str::to_owned).collect();
    *seen = progress.logged;
    Snapshot { current: progress.current(), summary: progress.summary(), fraction: progress.fraction(), lines }
}

/// Run the steps in order, telling `sender` how it goes.
fn work(steps: Vec<crate::ops::Step>, stop: nix::Stop, sender: async_channel::Sender<Event>) {
    let mut out = String::new();
    for step in steps {
        if stop.requested() {
            let _ = sender.send_blocking(Event::Done(Err("Stopped".into())));
            return;
        }
        let run = match step() {
            Ok(Some(run)) => run,
            Ok(None) => continue,
            Err(e) => {
                let _ = sender.send_blocking(Event::Done(Err(e)));
                return;
            }
        };
        stop.signal(!run.as_root);
        let _ = sender.send_blocking(Event::Started { as_root: run.as_root });
        let (mut last, mut seen) = (Instant::now() - EVERY, 0u64);
        let result = nix::stream(run.command, &stop, |progress| {
            if last.elapsed() >= EVERY {
                last = Instant::now();
                let _ = sender.send_blocking(Event::Progress(snapshot(progress, &mut seen)));
            }
        });
        match result {
            Ok((output, progress)) => {
                let _ = sender.send_blocking(Event::Progress(snapshot(&progress, &mut seen)));
                out = output;
            }
            Err(e) => {
                let _ = sender.send_blocking(Event::Done(Err(e.to_string())));
                return;
            }
        }
    }
    let _ = sender.send_blocking(Event::Done(Ok(out)));
}

/// Add lines at the end of the log, keeping only the last few, and follow
/// them unless the reader has scrolled back.
fn append(log: &gtk::TextView, scroll: &gtk::ScrolledWindow, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let adjustment = scroll.vadjustment();
    let following = adjustment.value() + adjustment.page_size() >= adjustment.upper() - 4.0;
    let buffer = log.buffer();
    let mut end = buffer.end_iter();
    let mut text = String::new();
    for line in lines {
        if buffer.char_count() > 0 || !text.is_empty() {
            text.push('\n');
        }
        text.push_str(line);
    }
    buffer.insert(&mut end, &text);
    let extra = buffer.line_count() - KEPT_LINES;
    if extra > 0 {
        let mut start = buffer.start_iter();
        if let Some(mut cut) = buffer.iter_at_line(extra) {
            buffer.delete(&mut start, &mut cut);
        }
    }
    if following {
        let mut end = buffer.end_iter();
        log.scroll_to_iter(&mut end, 0.0, false, 0.0, 0.0);
    }
}

pub fn run(ctx: &Ctx, operation: Operation) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let status = gtk::Label::new(Some("Starting…"));
    status.add_css_class("title-4");
    status.set_xalign(0.0);
    status.set_wrap(true);
    let bar = gtk::ProgressBar::new();
    let summary = super::widgets::dim("");
    summary.set_xalign(0.0);
    let error = super::widgets::wrapping("", &["error"]);
    error.set_selectable(true);
    error.set_visible(false);

    let log = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    log.add_css_class("job-log");
    let log_scroll = gtk::ScrolledWindow::builder().min_content_height(220).max_content_height(220).child(&log).build();
    let details = gtk::Expander::new(Some("Details"));
    details.set_child(Some(&log_scroll));

    let quiet = super::widgets::wrapping(
        "Nix hasn't reported anything for a few minutes. It may be waiting on a slow download. You can wait, or \
         stop and try again later.",
        &["dim-label"],
    );
    quiet.set_visible(false);

    let close = gtk::Button::with_label("Close");
    close.add_css_class("pill");
    close.set_halign(gtk::Align::Center);
    close.set_visible(false);
    let stop_button = gtk::Button::with_label("Stop");
    stop_button.add_css_class("pill");
    stop_button.set_halign(gtk::Align::Center);

    content.append(&status);
    content.append(&bar);
    content.append(&summary);
    content.append(&quiet);
    content.append(&error);
    content.append(&details);
    content.append(&stop_button);
    content.append(&close);

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    let dialog = adw::Dialog::builder().title(operation.title()).content_width(600).child(&view).build();
    dialog.set_can_close(false);
    {
        let dialog = dialog.clone();
        close.connect_clicked(move |_| {
            dialog.force_close();
        });
    }
    dialog.present(Some(ctx.window()));

    // Pulse until Nix says how much there is to do.
    let pulsing = std::rc::Rc::new(std::cell::Cell::new(true));
    {
        let (bar, pulsing, running) = (bar.clone(), pulsing.clone(), stop_button.clone());
        glib::timeout_add_local(Duration::from_millis(120), move || {
            if !running.is_visible() {
                return glib::ControlFlow::Break;
            }
            if pulsing.get() {
                bar.pulse();
            }
            glib::ControlFlow::Continue
        });
    }

    // Stopping: the helper (as root) is asked through its input; this
    // user's own Nix is also signalled.
    let stop = nix::Stop::new(false);
    {
        let (stop, status) = (stop.clone(), status.clone());
        stop_button.connect_clicked(move |button| {
            stop.request();
            button.set_sensitive(false);
            status.set_text("Stopping…");
        });
    }
    // Mention it when Nix goes quiet for long.
    let heard = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
    {
        let (heard, quiet, running) = (heard.clone(), quiet.clone(), stop_button.clone());
        glib::timeout_add_seconds_local(5, move || {
            if !running.is_visible() {
                return glib::ControlFlow::Break;
            }
            quiet.set_visible(heard.get().elapsed() >= QUIET);
            glib::ControlFlow::Continue
        });
    }

    let (sender, receiver) = async_channel::unbounded::<Event>();
    let steps = operation.steps(&ctx.model());
    {
        let stop = stop.clone();
        std::thread::spawn(move || work(steps, stop, sender));
    }

    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let _busy = crate::stalls::doing("showing progress");
            match event {
                Event::Started { as_root } => {
                    heard.set(Instant::now());
                    pulsing.set(true);
                    if !stop.requested() {
                        status.set_text(if as_root { "Waiting for permission…" } else { "Working…" });
                    }
                }
                Event::Progress(snapshot) => {
                    heard.set(Instant::now());
                    quiet.set_visible(false);
                    if !stop.requested() {
                        status.set_text(&snapshot.current.unwrap_or_else(|| "Working…".to_owned()));
                    }
                    summary.set_text(&snapshot.summary);
                    if let Some(fraction) = snapshot.fraction {
                        pulsing.set(false);
                        bar.set_fraction(fraction);
                    }
                    append(&log, &log_scroll, &snapshot.lines);
                }
                Event::Done(result) => {
                    pulsing.set(false);
                    dialog.set_can_close(true);
                    stop_button.set_visible(false);
                    quiet.set_visible(false);
                    match result {
                        Ok(out) => {
                            bar.set_fraction(1.0);
                            status.set_text(&operation.done());
                            ctx.toast(&operation.outcome(&out));
                            ctx.reload();
                            let dialog = dialog.clone();
                            glib::timeout_add_local_once(Duration::from_millis(900), move || {
                                dialog.force_close();
                            });
                        }
                        Err(message) => {
                            status.set_text(if stop.requested() { "Stopped" } else { "That did not work" });
                            error.set_text(&explain(&message, operation.is_system(), stop.requested()));
                            error.set_visible(true);
                            details.set_expanded(true);
                            close.set_visible(true);
                            close.grab_focus();
                            ctx.reload();
                        }
                    }
                    break;
                }
            }
        }
    });
}
