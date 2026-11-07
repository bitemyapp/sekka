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
/// the one set), its host name, keyboard, desktop and language. Nothing
/// about the disk: this installation's partitions are identity.nix's
/// (`calamares.appleSilicon.identity`), so a setup.nix copied to another
/// installation mounts nothing of this one.
pub fn setup_nix(s: &Staged) -> String {
    let groups = s.groups_declared.iter().map(|g| q(g)).collect::<Vec<_>>().join(" ");
    format!(
        r#"# Written by Sekka's first-boot setup: what was chosen when this computer
# was set up. configuration.nix imports it. Change it as you would the rest
# of the configuration; the next update builds it.
{{ lib, ... }}:
{{
  calamares.firstBootSetup.applied = true;
  calamares.installUser = {name};
  # Over the image's default, and under a plain value in configuration.nix,
  # which is where Yukimi changes the default desktop.
  calamares.defaultDesktop = lib.mkOverride 900 {desktop};
  users.users.{name} = {{
    isNormalUser = true;
    uid = {uid};
    description = {full_name};
    extraGroups = [ {groups} ];
    # The shell it was made with (users.defaultUserShell), by its path: the
    # system has it already, and naming its package again would rebuild the
    # system's packages for nothing.
    shell = {shell};
    # The password is the one set at setup, in /etc/shadow
    # (users.mutableUsers keeps it); change it with passwd or in Settings.
  }};
  networking.hostName = {hostname};
  services.xserver.xkb.layout = {keyboard};
  # The language, {locale_text}, and the time zone, {zone}, are
  # the system's own settings: Settings changes them, and updates keep
  # them. The line below only seeds /etc/locale.conf, should it ever be
  # missing.
  i18n.defaultLocale = {locale};
}}
"#,
        name = q(&s.username),
        desktop = q(&s.desktop),
        uid = s.uid,
        full_name = q(&s.full_name),
        shell = q(&s.shell),
        hostname = q(&s.hostname),
        locale = q(&s.locale),
        keyboard = q(&s.keyboard),
        locale_text = s.locale,
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
            shell: "/run/current-system/sw/bin/bash".into(),
            hostname: "analytical".into(),
            locale: "en_GB.UTF-8".into(),
            keyboard: "gb".into(),
            time_zone: "Europe/London".into(),
            desktop: "tatami".into(),
            groups: vec!["networkmanager".into(), "wheel".into(), "render".into()],
            groups_declared: vec!["networkmanager".into(), "wheel".into()],
            before: Default::default(),
            applied: Default::default(),
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
        assert!(text.contains(r#"shell = "/run/current-system/sw/bin/bash";"#));
        assert!(text.contains(r#"networking.hostName = "analytical";"#));
        assert!(text.contains(r#"calamares.defaultDesktop = lib.mkOverride 900 "tatami";"#));
        // The disk is identity.nix's alone.
        assert!(!text.contains("fileSystems") && !text.contains("partuuid") && !text.contains("identity"));
        assert!(!text.to_lowercase().contains("hashedpassword") && !text.contains("$6$"));
    }

    /// `test-data/setup.nix` is what setup writes for the staged choices
    /// below; the module's check evaluates a whole system with it
    /// (default.nix), and the Mac image's VM check its flake.
    /// `SEKKA_SETUP_GOLDEN=write` writes it anew.
    #[test]
    fn the_evaluated_example_is_what_setup_writes() {
        let mut s = staged();
        s.full_name = "Ada Lovelace".into();
        s.groups = vec!["networkmanager".into(), "wheel".into(), "render".into()];
        let text = setup_nix(&s);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/setup.nix");
        if std::env::var("SEKKA_SETUP_GOLDEN").as_deref() == Ok("write") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(text, include_str!("../test-data/setup.nix"));
    }

    #[test]
    fn keyboard_files() {
        assert_eq!(read_greeter_keyboard(&greeter_keyboard("de")).as_deref(), Some("de"));
        assert_eq!(read_greeter_keyboard("variant = \"\"\n"), None);
        assert!(user_keyboard("fr").contains("[input.keyboard]\nlayouts = [\"fr\"]\n"));
    }
}
