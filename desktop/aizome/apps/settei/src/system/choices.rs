//! The system's own choices, which only Yukimi changes
//! (`programs.yukimi.choices`, in `/etc/yukimi/system.json`): what one is
//! in the running system, and in the one the computer starts next (a change
//! Yukimi prepared waits for a restart). Settei proposes changes to them by
//! launching `aizome-yukimi --choose SETTING=VALUE`.

use serde::Deserialize;

/// The running system's choices.
pub const NOW: &str = "/etc/yukimi/system.json";
/// The choices of the system the computer starts next.
pub const NEXT: &str = "/nix/var/nix/profiles/system/etc/yukimi/system.json";

/// A choice in the running system and in the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Now<T> {
    pub now: T,
    pub next: T,
}

/// `setting`'s value in a `yukimi/system.json`, if the system offers it.
pub fn value(text: &str, setting: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Facts {
        #[serde(default)]
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        setting: String,
        value: serde_json::Value,
    }
    let facts: Facts = serde_json::from_str(text).ok()?;
    let choice = facts.choices.into_iter().find(|c| c.setting == setting)?;
    choice.value.as_str().map(str::to_owned)
}

/// `setting` now and from the next start, if the running system offers it
/// (the next one is as now when it doesn't say).
pub fn read(setting: &str) -> Option<Now<String>> {
    let read = |path: &str| std::fs::read_to_string(path).ok().and_then(|text| value(&text, setting));
    let now = read(NOW)?;
    let next = read(NEXT).unwrap_or_else(|| now.clone());
    Some(Now { now, next })
}

/// Propose `value` for `setting` in Yukimi, which asks for an administrator
/// password and builds the system once its Apply is pressed.
pub fn propose(setting: &str, value: &str) -> [String; 3] {
    ["aizome-yukimi".to_owned(), "--choose".to_owned(), format!("{setting}={value}")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_is_read_by_its_setting() {
        let facts = r#"{"nixpkgs":null,"choices":[
            {"setting":"calamares.loginScreen","title":"Login screen","value":"auto","options":[]},
            {"setting":"calamares.autoLogin","title":"Log in automatically","people":true,"value":"","options":[]},
            {"setting":"calamares.desktops","title":"Desktops","many":true,"value":["aizome"],"options":[]}]}"#;
        assert_eq!(value(facts, "calamares.loginScreen").as_deref(), Some("auto"));
        assert_eq!(value(facts, "calamares.autoLogin").as_deref(), Some(""));
        // A list isn't one value; and older systems offer fewer.
        assert_eq!(value(facts, "calamares.desktops"), None);
        assert_eq!(value(facts, "calamares.bootSplash.enable"), None);
        assert_eq!(value("{}", "calamares.autoLogin"), None);
        assert_eq!(value("not json", "calamares.autoLogin"), None);
    }

    #[test]
    fn a_proposal_is_yukimis_command_line() {
        assert_eq!(propose("calamares.autoLogin", "kara"), ["aizome-yukimi", "--choose", "calamares.autoLogin=kara"]);
        assert_eq!(propose("calamares.autoLogin", "")[2], "calamares.autoLogin=");
    }
}
