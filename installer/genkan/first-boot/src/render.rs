// SPDX-License-Identifier: GPL-3.0-or-later
//! The files setup writes: `/etc/nixos/setup.nix`, the person's choices as
//! configuration, which the installed configuration imports, so the first
//! update builds exactly what they chose; and the keyboard files Aizome's
//! login screen and session read until then. None holds a secret: the
//! account's password stays in `/etc/shadow`, which NixOS keeps for
//! accounts it didn't make (`users.mutableUsers`).

use calamares_nixos::config::nix_string as q;

use crate::transaction::Staged;

/// The configuration for `s`: the account (without a password: NixOS keeps
/// the one set), its host name, locale, keyboard and desktop, and, when
/// the installer's handoff named this installation's partitions, those.
pub fn setup_nix(s: &Staged) -> String {
    let groups = s.groups_declared.iter().map(|g| q(g)).collect::<Vec<_>>().join(" ");
    let partitions = match &s.partitions {
        Some((root, esp)) => format!(
            "  # This installation's own partitions, as the Mac installer handed them\n  \
             # over: by GPT partition UUID, which no other installation shares.\n  \
             fileSystems.\"/\".device = lib.mkForce {};\n  \
             fileSystems.\"/boot\".device = lib.mkForce {};\n",
            q(&format!("/dev/disk/by-partuuid/{root}")),
            q(&format!("/dev/disk/by-partuuid/{esp}")),
        ),
        None => String::new(),
    };
    format!(
        r#"# Written by Sekka's first-boot setup: what was chosen when this computer
# was set up. configuration.nix imports it. Change it as you would the rest
# of the configuration; the next update builds it.
{{ lib, ... }}:
{{
  calamares.firstBootSetup.applied = true;
  calamares.installUser = {name};
  calamares.defaultDesktop = {desktop};
  users.users.{name} = {{
    isNormalUser = true;
    uid = {uid};
    description = {full_name};
    extraGroups = [ {groups} ];
    # The password is the one set at setup, in /etc/shadow
    # (users.mutableUsers keeps it); change it with passwd or in Settings.
  }};
  networking.hostName = {hostname};
  i18n.defaultLocale = {locale};
  services.xserver.xkb.layout = {keyboard};
  # The time zone, {zone}, is the system's own setting: Settings changes
  # it, and updates keep it.
{partitions}}}
"#,
        name = q(&s.username),
        desktop = q(&s.desktop),
        uid = s.uid,
        full_name = q(&s.full_name),
        hostname = q(&s.hostname),
        locale = q(&s.locale),
        keyboard = q(&s.keyboard),
        zone = s.time_zone,
    )
}

/// The login screen's keyboard (Aizome's greeter, `keyboard_file`).
pub fn greeter_keyboard(layout: &str) -> String {
    format!(
        "# Written by Sekka's first-boot setup: the login screen's keyboard until\n\
         # the configuration's own (services.xserver.xkb) is built.\n\
         layouts = [{}]\nvariant = \"\"\n",
        q(layout)
    )
}

/// The layout a greeter keyboard file names.
pub fn read_greeter_keyboard(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let v = l.strip_prefix("layouts = [")?.strip_suffix(']')?;
        serde_json::from_str::<String>(v.trim()).ok()
    })
}

/// The person's Aizome keyboard, as Settings writes it.
pub fn user_keyboard(layout: &str) -> String {
    format!(
        "# The keyboard chosen at setup. Settings ▸ Keyboard changes it.\n[input.keyboard]\nlayouts = [{}]\n",
        q(layout)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn staged() -> Staged {
        Staged {
            schema: crate::transaction::STAGED_SCHEMA.into(),
            username: "ada".into(),
            full_name: "Ada \"Countess\" ${Lovelace}".into(),
            uid: 1000,
            hostname: "analytical".into(),
            locale: "en_GB.UTF-8".into(),
            keyboard: "gb".into(),
            time_zone: "Europe/London".into(),
            desktop: "tatami".into(),
            groups: vec!["networkmanager".into(), "wheel".into(), "render".into()],
            groups_declared: vec!["networkmanager".into(), "wheel".into()],
            partitions: None,
        }
    }

    #[test]
    fn setup_nix_says_what_was_chosen_and_nothing_secret() {
        let text = setup_nix(&staged());
        assert!(text.contains("users.users.\"ada\" = {"), "{text}");
        assert!(text.contains("uid = 1000;"));
        // Quoted for Nix: no interpolation can come from a name.
        assert!(text.contains(r#"description = "Ada \"Countess\" \${Lovelace}";"#), "{text}");
        assert!(text.contains(r#"extraGroups = [ "networkmanager" "wheel" ];"#));
        assert!(text.contains(r#"networking.hostName = "analytical";"#));
        assert!(text.contains(r#"calamares.defaultDesktop = "tatami";"#));
        assert!(!text.contains("fileSystems"));
        assert!(!text.to_lowercase().contains("hashedpassword") && !text.contains("$6$"));
        let mut with = staged();
        with.partitions = Some(("63d89be1-72f7-4aff-9679-09cafa7e0c2d".into(), "910de5ca-7390-4716-9a54-8028916e4d78".into()));
        let text = setup_nix(&with);
        assert!(text.contains(r#"fileSystems."/".device = lib.mkForce "/dev/disk/by-partuuid/63d89be1-72f7-4aff-9679-09cafa7e0c2d";"#));
    }

    #[test]
    fn keyboard_files() {
        assert_eq!(read_greeter_keyboard(&greeter_keyboard("de")).as_deref(), Some("de"));
        assert_eq!(read_greeter_keyboard("variant = \"\"\n"), None);
        assert!(user_keyboard("fr").contains("[input.keyboard]\nlayouts = [\"fr\"]\n"));
    }
}
