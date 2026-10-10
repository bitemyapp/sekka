// SPDX-License-Identifier: MIT OR Apache-2.0
//! One persistent completion request, with reboot taking precedence over logout.
use super::Ctx;
use adw::prelude::*;
use gtk::{gio, glib};
use yukimi_system::completion::{Action, Completion};

pub fn row(ctx: &Ctx, completion: &Completion) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::builder().title(completion.title()).subtitle(completion.description()).build();
    let command: Option<(&str, &str, &[&str])> = match completion.action {
        Action::Reboot => Some(("Restart now", "systemctl", &["--check-inhibitors=yes", "reboot"])),
        Action::Logout => Some(("Sign out", "uwsm", &["stop"])),
        _ => None,
    };
    if let Some((label, program, args)) = command {
        let button = gtk::Button::with_label(label);
        button.add_css_class("suggested-action");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Save your work first. You can also leave this until later."));
        let ctx = ctx.clone();
        let program = program.to_owned();
        let args: Vec<String> = args.iter().map(|a| (*a).into()).collect();
        let expected = completion.action.clone();
        button.connect_clicked(move |button| {
            button.set_sensitive(false);
            let (ctx, button, program, args, expected) =
                (ctx.clone(), button.clone(), program.clone(), args.clone(), expected.clone());
            glib::spawn_future_local(async move {
                // Respect logind inhibitors and authorization; never force a
                // restart past another user's session or an ongoing workload.
                let result = gio::spawn_blocking(move || {
                    if yukimi_system::completion::read().action != expected {
                        return Err(std::io::Error::other("Update status changed; checking again"));
                    }
                    std::process::Command::new(program).args(args).output()
                })
                .await;
                match result {
                    Ok(Ok(out)) if out.status.success() => (),
                    Ok(Ok(out)) => {
                        ctx.toast(&format!("Could not finish: {}", String::from_utf8_lossy(&out.stderr).trim()));
                        button.set_sensitive(true);
                    }
                    _ => {
                        ctx.toast("Could not start that action. Your session is still running.");
                        button.set_sensitive(true);
                    }
                }
                ctx.reload();
            });
        });
        row.add_suffix(&button);
    }
    group.add(&row);
    group
}
