//! `tasuke checkup`: the routine check-up. It runs the sekka-checkup skill
//! with Act off (it can only look), keeps the result as a conversation the
//! window lists like any other, and with `--notify` raises a desktop
//! notification only when something needs attention. The Nix module's
//! weekly timer runs `tasuke checkup --notify` at low priority.

use std::time::Duration;

use tasuke_agent::{Event, FinishReason, Options, Session};

const REQUEST: &str = "Give my computer a check-up. This is the routine weekly check, so I'm not watching: report \
                       in your usual check-up format.";

pub fn run(args: &[String]) -> i32 {
    let mut notify = false;
    let mut always = false;
    for a in args {
        match a.as_str() {
            "--notify" => notify = true,
            "--always" => always = true,
            "-h" | "--help" => {
                println!("usage: tasuke checkup [--notify [--always]]\n\nA read-only check-up of this computer; \
                          --notify shows a notification when something needs attention (--always: every time).");
                return 0;
            }
            other => {
                eprintln!("tasuke checkup: unknown option {other}");
                return 2;
            }
        }
    }
    let mut o = Options::from_config();
    if o.model.is_empty() {
        eprintln!("tasuke checkup: no model is chosen (tasuke use ID)");
        return 2;
    }
    o.act = false;
    o.desktop_control = false;
    o.time_limit = Some(Duration::from_secs(900));
    let session = match Session::create(&o.data_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tasuke checkup: {e}");
            return 1;
        }
    };
    let turn = tasuke_agent::Agent::start(&session, REQUEST, o);
    let mut answer = String::new();
    let mut reason = None;
    for e in turn.events() {
        match e {
            Event::ToolCall { .. } => answer.clear(),
            Event::TextDelta { text } => answer.push_str(&text),
            Event::TurnFinished { reason: r, .. } => reason = Some(r),
            _ => {}
        }
    }
    turn.join();
    println!("{}", answer.trim());
    if reason != Some(FinishReason::Answered) {
        return 1;
    }
    let attention = needs_attention(&answer);
    if notify && (attention || always) {
        let verdict = answer.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Check-up finished.");
        let title = if attention { "Your computer needs attention" } else { "Your computer looks healthy" };
        notify_desktop(title, &strip_markdown(verdict));
    }
    0
}

/// The report marks problems ✕ and warnings ⚠ (the skill's format).
fn needs_attention(report: &str) -> bool {
    report.contains('✕') || report.contains('⚠')
}

fn strip_markdown(s: &str) -> String {
    s.replace("**", "").replace('`', "").trim_start_matches('#').trim().to_owned()
}

/// A notification through the freedesktop interface (busctl is part of
/// systemd, so always there). Its desktop-entry hint lets the shell open Tasuke.
fn notify_desktop(title: &str, body: &str) {
    let status = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            "susssasa{sv}i",
            "Assistant",
            "0",
            "org.aizome.Tasuke",
            title,
            body,
            "0",
            "1",
            "desktop-entry",
            "s",
            "org.aizome.Tasuke",
            "-1",
        ])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        eprintln!("tasuke checkup: the notification couldn't be shown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_follows_the_report_marks() {
        assert!(needs_attention("Two things need attention.\n✕ Disk 94% full\n✓ the rest"));
        assert!(needs_attention("⚠ Updated 23 days ago"));
        assert!(!needs_attention("Your computer looks healthy.\n✓ everything"));
        assert_eq!(strip_markdown("**Two things** need `attention`"), "Two things need attention");
    }
}
