//! Yukimi's own edits (apps/yukimi, yukimi-config) on the configuration.nix
//! Sekka's Mac images ship, which tests.nix names in SEKKA_MAC_CONFIGURATION:
//! installing a package for everyone adds ./yukimi.nix to its `imports`, and
//! the Default desktop choice rewrites calamares.defaultDesktop.
use yukimi_config::{edit, packages};

fn configuration() -> String {
    let path = std::env::var("SEKKA_MAC_CONFIGURATION").expect("SEKKA_MAC_CONFIGURATION");
    std::fs::read_to_string(path).expect("the shipped configuration.nix")
}

#[test]
fn installing_for_everyone_imports_yukimi_nix() {
    let text = configuration();
    let edited = packages::import_into(&text).expect("import_into on the shipped configuration.nix");
    assert!(edited.contains(packages::IMPORT), "{edited}");
    assert!(edited.contains("./imports.nix"), "{edited}");
    assert_eq!(packages::import_into(&edited).unwrap(), edited);
}

#[test]
fn the_former_forms_are_refused() {
    // The failing control: what configuration.nix shipped before.
    let old = "{ lib, ... }:\n{\n  imports = builtins.filter builtins.pathExists [ ./identity.nix ];\n  calamares.defaultDesktop = lib.mkDefault \"aizome\";\n}\n";
    assert!(packages::import_into(old).is_err());
    assert!(edit::set_string(old, &["calamares", "defaultDesktop"], "tatami").is_err());
}

#[test]
fn the_default_desktop_is_a_string_yukimi_can_change() {
    let text = configuration();
    let edited = edit::set_string(&text, &["calamares", "defaultDesktop"], "tatami")
        .expect("set_string on the shipped configuration.nix");
    assert!(edited.contains("defaultDesktop = \"tatami\";"), "{edited}");
    assert_eq!(edited.matches("defaultDesktop").count(), 1, "{edited}");
}
