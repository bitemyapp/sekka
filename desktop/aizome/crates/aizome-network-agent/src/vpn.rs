//! Use installed VPN plugins' authentication protocol. Plugins decide
//! which secrets/challenges are needed; external-UI plugins use Aizome's
//! dialog, others retain their own supported authentication UI.
use crate::{
    AgentError, Result,
    model::{ALLOW_INTERACTION, Field, Profile, Prompt, REQUEST_NEW, dict, string},
    process::{self, Cancel},
    prompt::Prompter,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

type Ini = HashMap<String, HashMap<String, String>>;
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('s') => out.push(' '),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn ini(text: &str) -> Ini {
    let mut data = Ini::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(s) = line.trim_end().strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.into();
        } else if let Some((k, v)) = line.split_once('=') {
            data.entry(section.clone()).or_default().insert(k.trim().into(), unescape(v.trim_start()));
        }
    }
    data
}
fn get<'a>(ini: &'a Ini, group: &str, key: &str) -> &'a str {
    ini.get(group).and_then(|g| g.get(key)).map(String::as_str).unwrap_or("")
}
pub struct Plugins {
    pub dirs: Vec<PathBuf>,
}
impl Default for Plugins {
    fn default() -> Self {
        let dirs = std::env::var_os("AIZOME_NM_VPN_PLUGIN_DIRS")
            .map(|s| std::env::split_paths(&s).collect())
            .unwrap_or_else(|| vec!["/etc/NetworkManager/VPN".into(), "/usr/lib/NetworkManager/VPN".into()]);
        Self { dirs }
    }
}
impl Plugins {
    pub fn secrets(
        &self,
        p: &Profile,
        hints: &[String],
        flags: u32,
        secrets: HashMap<String, String>,
        ui: &dyn Prompter,
        cancel: &Cancel,
    ) -> Result<HashMap<String, String>> {
        let service = string(&p.settings, "vpn", "service-type");
        let descriptor = self
            .dirs
            .iter()
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten())
            .filter_map(|f| f.ok())
            .filter(|f| f.path().extension().is_some_and(|e| e == "name"))
            .filter_map(|f| std::fs::read_to_string(f.path()).ok())
            .map(|s| ini(&s))
            .find(|d| get(d, "VPN Connection", "service") == service)
            .ok_or_else(|| AgentError::NoSecrets("The VPN authentication plugin is not installed".into()))?;
        let helper = Path::new(get(&descriptor, "GNOME", "auth-dialog"));
        if !helper.is_absolute() || !helper.is_file() {
            return Err(AgentError::Failed("The VPN plugin has no installed authentication helper".into()));
        }
        let external = get(&descriptor, "GNOME", "supports-external-ui-mode") == "true";
        let mut command = Command::new(helper);
        command.args(["-u", &p.uuid, "-n", &p.name, "-s", &service]).env_remove("G_MESSAGES_DEBUG");
        if flags & ALLOW_INTERACTION != 0 {
            command.arg("-i");
        }
        if flags & REQUEST_NEW != 0 {
            command.arg("-r");
        }
        if get(&descriptor, "GNOME", "supports-hints") == "true" {
            for hint in hints {
                command.args(["-t", hint]);
            }
        }
        if external {
            command.arg("--external-ui-mode");
        }
        let mut input = String::new();
        for (tag, values) in [("DATA", dict(&p.settings, "vpn", "data")), ("SECRET", secrets)] {
            for (key, value) in values {
                // The plugin protocol is line oriented.
                if key.contains(['\n', '\r']) || value.contains(['\n', '\r']) {
                    return Err(AgentError::InvalidConnection("VPN credential contains a line break".into()));
                }
                input.push_str(&format!("{tag}_KEY={key}\n{tag}_VAL={value}\n"));
            }
        }
        input.push_str("DONE\n\nQUIT\n\n");
        // As in nm-applet, a helper that exits non-zero was cancelled.
        let output = process::run(
            &mut command,
            input.into_bytes(),
            cancel,
            AgentError::UserCanceled("Authentication was cancelled or the helper could not complete".into()),
        )?;
        let output = String::from_utf8(output).map_err(|_| AgentError::Failed("Invalid VPN helper encoding".into()))?;
        if external {
            external_reply(&output, flags, ui, cancel)
        } else {
            let mut lines = output.lines();
            let mut values = HashMap::new();
            while let Some(key) = lines.next().filter(|l| !l.is_empty()) {
                let value = lines.next().ok_or_else(|| AgentError::Failed("Incomplete VPN helper response".into()))?;
                values.insert(key.into(), value.into());
            }
            Ok(values)
        }
    }
}
fn external_reply(output: &str, flags: u32, ui: &dyn Prompter, cancel: &Cancel) -> Result<HashMap<String, String>> {
    let parsed = ini(output);
    if get(&parsed, "VPN Plugin UI", "Version") != "2" {
        return Err(AgentError::Failed("Unsupported VPN authentication protocol".into()));
    }
    let mut values = HashMap::new();
    let mut fields = Vec::new();
    for (key, props) in &parsed {
        if key == "VPN Plugin UI" {
            continue;
        }
        let value = props.get("Value").cloned().unwrap_or_default();
        let secret = props.get("IsSecret").is_some_and(|v| v == "true");
        if props.get("ShouldAsk").is_some_and(|v| v == "true") {
            fields.push(Field {
                key: key.clone(),
                label: props.get("Label").cloned().unwrap_or_else(|| key.clone()),
                value,
                secret,
                required: true,
            });
        } else if !value.is_empty() {
            values.insert(key.clone(), value);
        }
    }
    fields.sort_by(|a, b| a.key.cmp(&b.key));
    if !fields.is_empty() {
        if flags & ALLOW_INTERACTION == 0 {
            return Err(AgentError::NoSecrets("VPN authentication needs user interaction".into()));
        }
        let requested: Vec<_> = fields.iter().map(|f| f.key.clone()).collect();
        let entered = ui.ask(
            Prompt {
                title: get(&parsed, "VPN Plugin UI", "Title").into(),
                message: get(&parsed, "VPN Plugin UI", "Description").into(),
                fields,
            },
            cancel,
        )?;
        for key in requested {
            let value = entered
                .get(&key)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| AgentError::NoSecrets("A required VPN credential was not supplied".into()))?;
            values.insert(key, value.clone());
        }
    }
    if values.is_empty() {
        return Err(AgentError::NoSecrets("The VPN helper returned no credentials".into()));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    #[test]
    fn keyfile_values_preserve_password_spaces_and_escapes() {
        let parsed = super::ini("[password]\nValue=\\sleading and trailing \nLabel=Token\\ncode\n");
        assert_eq!(super::get(&parsed, "password", "Value"), " leading and trailing ");
        assert_eq!(super::get(&parsed, "password", "Label"), "Token\ncode");
    }
}
