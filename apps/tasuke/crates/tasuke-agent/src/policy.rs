//! Placeholder until the default-deny classifier lands from feat/assistant-pkg:
//! a few plain commands run, everything else asks, the obvious
//! session-enders are refused.

use std::path::Path;

use crate::event::Approval;

pub fn classify_command(command: &str, home: &Path) -> Approval {
    if let Some(reason) = refused(command, home) {
        return Approval::Refused { reason };
    }
    let plain = !command.contains([';', '|', '&', '>', '<', '$', '`', '\n']);
    if plain && matches!(command.split_whitespace().next(), Some("echo" | "true" | "ls" | "cat")) {
        return Approval::None;
    }
    Approval::Ask { reason: "The command policy isn't in this build yet.".into() }
}

pub fn classify_read(_path: &Path, _home: &Path) -> Approval {
    Approval::None
}

pub fn refused(command: &str, _home: &Path) -> Option<String> {
    let first = command.split_whitespace().next();
    matches!(first, Some("reboot" | "poweroff" | "shutdown")).then(|| "It would end the person's session.".to_owned())
}
