// SPDX-License-Identifier: MIT OR Apache-2.0
//! BlueZ Agent1, inspired by Aizome's pairing sheets. Keep invocations pending
//! while the user answers; never accept a comparison or authorization silently.
use crate::{
    bluez::AGENT_PATH,
    model,
    window::{self, Ui},
};
use adw::prelude::*;
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const XML: &str = r#"<node><interface name="org.bluez.Agent1">
<method name="Release"/>
<method name="RequestPinCode"><arg type="o" direction="in"/><arg type="s" direction="out"/></method>
<method name="DisplayPinCode"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
<method name="RequestPasskey"><arg type="o" direction="in"/><arg type="u" direction="out"/></method>
<method name="DisplayPasskey"><arg type="o" direction="in"/><arg type="u" direction="in"/><arg type="q" direction="in"/></method>
<method name="RequestConfirmation"><arg type="o" direction="in"/><arg type="u" direction="in"/></method>
<method name="RequestAuthorization"><arg type="o" direction="in"/></method>
<method name="AuthorizeService"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
<method name="Cancel"/>
</interface></node>"#;

pub struct Prompt {
    pub path: String,
    dialog: adw::AlertDialog,
    invocation: RefCell<Option<gio::DBusMethodInvocation>>,
    /// Closing due to BlueZ/operation completion must not cancel a later Pair.
    finished: Cell<bool>,
}

impl Prompt {
    pub fn finish(&self) {
        self.finished.set(true);
        if let Some(call) = self.invocation.borrow_mut().take() {
            call.return_dbus_error("org.bluez.Error.Canceled", "Pairing cancelled");
        }
        self.dialog.force_close();
    }
}

pub fn export(
    ui: &Rc<Ui>,
    connection: &gio::DBusConnection,
) -> Result<gio::RegistrationId, glib::Error> {
    let info = gio::DBusNodeInfo::for_xml(XML)?
        .lookup_interface("org.bluez.Agent1")
        .unwrap();
    let weak = Rc::downgrade(ui);
    connection
        .register_object(AGENT_PATH, &info)
        .method_call(move |_, sender, _, _, method, params, call| {
            let Some(ui) = weak.upgrade() else {
                call.return_dbus_error("org.bluez.Error.Rejected", "Window closed");
                return;
            };
            // Do not let another process impersonate BlueZ or inject a pairing sheet.
            let authorized = ui
                .backend
                .borrow()
                .as_ref()
                .is_some_and(|b| Some(b.owner.as_str()) == sender);
            if !authorized {
                call.return_dbus_error("org.bluez.Error.Rejected", "Not the Bluetooth service");
                return;
            }
            if method == "Cancel" || method == "Release" {
                close(&ui, None);
                if method == "Release" {
                    if let Some(b) = ui.backend.borrow().as_ref() {
                        b.agent_ready.set(false);
                    }
                    window::refresh(&ui);
                }
                call.return_value(Some(&().to_variant()));
                return;
            }
            let path = params.child_value(0).str().unwrap_or_default().to_owned();
            // We register an application agent, not a default agent. Only the
            // device explicitly being paired in this window may prompt here.
            if ui.pairing.borrow().as_deref() != Some(&path) {
                call.return_dbus_error(
                    "org.bluez.Error.Rejected",
                    "No pairing requested for this device",
                );
                return;
            }
            show(&ui, &path, method, &params, call);
        })
        .build()
}

pub fn close(ui: &Ui, device: Option<&str>) {
    let matches = ui
        .prompt
        .borrow()
        .as_ref()
        .is_some_and(|p| device.is_none_or(|d| p.path == d));
    if matches {
        let prompt = ui.prompt.borrow_mut().take();
        if let Some(prompt) = prompt {
            prompt.finish();
        }
    }
}

fn show(
    ui: &Rc<Ui>,
    path: &str,
    method: &str,
    params: &glib::Variant,
    call: gio::DBusMethodInvocation,
) {
    close(ui, None);
    let name = ui
        .snapshot
        .borrow()
        .devices
        .iter()
        .find(|d| d.path == path)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| path.to_owned());
    let display = method.starts_with("Display");
    let entry = gtk::Entry::builder().activates_default(true).build();
    entry.set_max_length(if method == "RequestPinCode" { 16 } else { 6 });
    let body = match method {
        "RequestPinCode" => "Enter the PIN shown on the device or supplied with it.".into(),
        "RequestPasskey" => "Enter the six-digit code shown on the device.".into(),
        "DisplayPinCode" => format!(
            "Type {} on the device, then press Enter.",
            params.child_get::<String>(1)
        ),
        "DisplayPasskey" => format!(
            "Type {:06} on the device, then press Enter.\n{} of 6 digits entered.",
            params.child_get::<u32>(1),
            params.child_get::<u16>(2)
        ),
        "RequestConfirmation" => format!(
            "Does the device show this same code?\n\n{:06}",
            params.child_get::<u32>(1)
        ),
        "RequestAuthorization" => "Allow this device to pair with your computer?".into(),
        "AuthorizeService" => "Allow this device to use a Bluetooth service?".into(),
        _ => {
            call.return_dbus_error("org.bluez.Error.Rejected", "Unsupported request");
            return;
        }
    };
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Pair with “{name}”?"))
        .body(body)
        .body_use_markup(false)
        .heading_use_markup(false)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
    let typed = matches!(method, "RequestPinCode" | "RequestPasskey");
    if !display {
        dialog.add_response(
            "pair",
            if method == "RequestConfirmation" {
                "Codes Match"
            } else {
                "Pair"
            },
        );
        dialog.set_response_appearance("pair", adw::ResponseAppearance::Suggested);
        if typed {
            dialog.set_extra_child(Some(&entry));
            dialog.set_response_enabled("pair", false);
            let numeric = method == "RequestPasskey";
            if numeric {
                entry.set_input_purpose(gtk::InputPurpose::Digits);
            }
            let dialog_weak = dialog.downgrade();
            entry.connect_changed(move |entry| {
                if let Some(d) = dialog_weak.upgrade() {
                    d.set_response_enabled(
                        "pair",
                        if numeric {
                            model::passkey(&entry.text()).is_some()
                        } else {
                            model::pin(&entry.text())
                        },
                    );
                }
            });
            dialog.set_default_response(Some("pair"));
        } else {
            // Enter must not accidentally confirm a code the person hasn't read.
            dialog.set_default_response(Some("cancel"));
        }
    }
    let invocation = if display {
        call.return_value(Some(&().to_variant()));
        None
    } else {
        Some(call)
    };
    let prompt = Rc::new(Prompt {
        path: path.into(),
        dialog: dialog.clone(),
        invocation: RefCell::new(invocation),
        finished: Cell::new(false),
    });
    *ui.prompt.borrow_mut() = Some(prompt.clone());
    let weak = Rc::downgrade(ui);
    let kind = method.to_owned();
    let prompt_weak = Rc::downgrade(&prompt);
    dialog.connect_response(None, move |_, response| {
        let Some(prompt) = prompt_weak.upgrade() else {
            return;
        };
        if prompt.finished.replace(true) {
            return;
        }
        if let Some(call) = prompt.invocation.borrow_mut().take() {
            if response == "pair" {
                let answer = match kind.as_str() {
                    "RequestPinCode" => {
                        model::pin(&entry.text()).then(|| (entry.text().as_str(),).to_variant())
                    }
                    "RequestPasskey" => {
                        model::passkey(&entry.text()).map(|code| (code,).to_variant())
                    }
                    _ => Some(().to_variant()),
                };
                if let Some(answer) = answer {
                    call.return_value(Some(&answer));
                } else {
                    call.return_dbus_error("org.bluez.Error.Rejected", "Invalid code");
                }
            } else {
                call.return_dbus_error("org.bluez.Error.Rejected", "Pairing declined");
            }
        }
        if let Some(ui) = weak.upgrade() {
            ui.prompt.borrow_mut().take();
            if response != "pair" {
                window::cancel_pairing(&ui);
            }
        }
    });
    dialog.present(Some(&ui.window));
}

#[cfg(test)]
pub fn respond(ui: &Ui, response: &str, text: Option<&str>) {
    let prompt = ui.prompt.borrow().clone().expect("pairing prompt");
    if let Some(text) = text {
        prompt
            .dialog
            .extra_child()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap()
            .set_text(text);
        assert!(prompt.dialog.is_response_enabled(response));
    }
    prompt.dialog.emit_by_name::<()>("response", &[&response]);
    prompt.dialog.force_close();
}
