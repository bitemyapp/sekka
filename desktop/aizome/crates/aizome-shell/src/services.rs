//! Services: the app menu's verbs over the selected text — asked of the
//! focused app (kit apps answer over the shell socket) or read from the
//! primary selection — and what each verb does.

use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_shell_proto as proto;

use crate::actions::{Action, ServiceVerb};
use crate::launch::Launch;
use crate::shell::{Msg, Shell};

/// The primary selection's text, if `wl-paste` can read it quickly.
pub fn primary_selection() -> Option<String> {
    let child = std::process::Command::new("wl-paste")
        .args(["--primary", "--no-newline", "--type", "text/plain"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !text.trim().is_empty()).then_some(text)
}

/// `text` for a query string.
pub fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Shell {
    /// The menu at the pointer, once the selected text for its Services is
    /// known (asked of the app, or read from the primary selection; at
    /// most 150 ms).
    pub fn pointer_menu_soon(&mut self, x: f64, y: f64, cx: &mut Cx<Msg>) {
        self.serial += 1;
        let serial = self.serial;
        self.services_wait = Some((serial, x, y));
        let conn = self.wm.focused_window().and_then(|w| self.registry.for_window(w.pid, &w.app_id)).map(|c| c.conn);
        match (conn, &self.server) {
            (Some(c), Some(server)) => server.send(c, &proto::ShellMessage::ServicesText { serial }),
            _ if self.opts.live => cx.spawn(move || Msg::Selection(serial, primary_selection())),
            _ => {}
        }
        cx.after(Duration::from_millis(150), Msg::Selection(serial, None));
    }

    /// The selected text came (or the wait ran out): show the menu waiting.
    pub fn selection_came(&mut self, serial: u64, text: Option<String>, cx: &mut Cx<Msg>) {
        match self.services_wait {
            Some((s, x, y)) if s == serial => {
                self.services_wait = None;
                self.selection = text.filter(|t| !t.trim().is_empty());
                self.pointer_menu(x, y, cx);
            }
            _ => {}
        }
    }

    /// Carry out a service over the selected text.
    pub fn run_service(&mut self, verb: ServiceVerb, cx: &mut Cx<Msg>) {
        let Some(text) = self.selection.clone() else { return };
        let t = text.trim().to_owned();
        match verb {
            ServiceVerb::CopyAsPlainText => cx.set_clipboard_text(&text),
            ServiceVerb::OpenUrl => {
                let url = if t.contains("://") || t.starts_with("mailto:") { t } else { format!("https://{t}") };
                self.run(Action::OpenUrl { url }, cx);
            }
            ServiceVerb::SearchWeb => {
                self.run(Action::OpenUrl { url: format!("https://duckduckgo.com/?q={}", url_encode(&t)) }, cx)
            }
            ServiceVerb::LookUpInFiles => self.open_palette(Some(aizome_search::Scope::File), t, cx),
            ServiceVerb::Calculate => {
                if let Ok(v) = aizome_search::calc::eval(&t) {
                    let r = aizome_search::calc::format(v);
                    cx.set_clipboard_text(&r);
                    let n = crate::notifications::store::Incoming {
                        app_name: "Calculate".into(),
                        summary: format!("{t} = {r}"),
                        body: "Copied".into(),
                        transient: true,
                        ..Default::default()
                    };
                    let id = self.fresh_note_id();
                    self.take_note(crate::notifications::server::NoteEvent::Notify(id, Box::new(n)), cx);
                }
            }
            ServiceVerb::NewNote | ServiceVerb::OpenInSumi => {
                // The text in a file of its own, opened by the app.
                let dir = aizome_ipc::runtime_dir().join("aizome-services");
                let _ = std::fs::create_dir_all(&dir);
                self.serial += 1;
                let file = dir.join(format!("selection-{}.txt", self.serial));
                if std::fs::write(&file, &text).is_ok() {
                    let app = if verb == ServiceVerb::NewNote { "org.aizome.Fusen" } else { "org.aizome.Sumi" };
                    self.launch(
                        Launch::App { app_id: app.into(), targets: vec![file.to_string_lossy().into_owned()] },
                        cx,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_encode() {
        assert_eq!(url_encode("tiles at 1.5×"), "tiles+at+1.5%C3%97");
    }
}
