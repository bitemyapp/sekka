// SPDX-License-Identifier: GPL-3.0-or-later
use crate::*;
use desktop::DesktopSelection;
use filesystem::Identities;
const ROOT: &str = "11111111-2222-4333-8444-555555555555";
const SWAP: &str = "66666666-7777-4888-9999-aaaaaaaaaaaa";
/// Fixed identities matching a plan's firmware and swap choice.
fn ids(plan: &InstallPlan) -> Identities {
    Identities {
        root: ROOT.into(),
        efi: (plan.firmware() == Firmware::Uefi).then(|| "A1B2-C3D4".into()),
        swap: plan.swap().then(|| SWAP.into()),
    }
}
fn render(plan: &InstallPlan) -> String {
    config::configuration(plan, &ids(plan)).unwrap()
}
fn fixture() -> (tempfile::TempDir, Settings, RawRequest) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Etc")).unwrap();
    fs::write(dir.path().join("Etc/UTC"), "TZif").unwrap();
    let settings = Settings {
        template_dir: "/unused".into(),
        zoneinfo: dir.path().to_string_lossy().into_owned(),
        state_version: "26.11".into(),
        kernel: Kernel::Lts,
        test_diagnostics: false,
    };
    let request = RawRequest {
        disk: disk::Identity {
            path: "/dev/vda".into(),
            major_minor: "252:0".into(),
            bytes: 40 * 1024u64.pow(3),
            serial: "test".into(),
            wwn: "".into(),
            model: "test".into(),
        },
        firmware: Firmware::Uefi,
        filesystem: Filesystem::Ext4,
        hostname: "my-machine".into(),
        username: "alice".into(),
        full_name: "Alice ${literal}".into(),
        password: "long-enough-test-password".into(),
        locale: "en_US.UTF-8".into(),
        timezone: "Etc/UTC".into(),
        keyboard: "us".into(),
        desktops: vec![Desktop::Plasma],
        default_desktop: Desktop::Plasma,
        applications: applications::default_selection(),
        copy_wifi: false,
        wifi_profiles: vec![],
        allow_unfree: false,
        swap: true,
        tuning: true,
        graphics: Default::default(),
        github_user: String::new(),
        ssh_keys: vec![],
        ssh_server: false,
        git_name: String::new(),
        git_email: String::new(),
        confirmation: "ERASE /dev/vda".into(),
    };
    (dir, settings, request)
}

#[test]
fn review_and_confirmation_are_distinct_transitions() {
    for phrase in ["", "ERASE /dev/sda", "ERASE /dev/vda ", "erase /dev/vda"] {
        let (_dir, settings, mut raw) = fixture();
        raw.confirmation = phrase.into();
        let plan = raw.parse(&settings).unwrap();
        assert_eq!(plan.hostname().as_str(), "my-machine");
        assert!(plan.confirm(phrase).is_err());
        let (_dir, settings, mut raw) = fixture();
        raw.confirmation = phrase.into();
        assert!(raw.parse_confirmed(&settings).is_err());
    }
    let (_dir, settings, raw) = fixture();
    raw.parse(&settings)
        .unwrap()
        .confirm("ERASE /dev/vda")
        .unwrap();
}

#[test]
fn ipc_downgrades_to_raw_and_helper_parses_again() {
    let (_dir, settings, raw) = fixture();
    let plan = raw.parse(&settings).unwrap();
    let expected = render(&plan);
    let wire = plan.confirm("ERASE /dev/vda").unwrap().into_request();
    let json = zeroize::Zeroizing::new(serde_json::to_vec(&wire).unwrap());
    let received: RawRequest = serde_json::from_slice(&json).unwrap();
    let confirmed = received.parse_confirmed(&settings).unwrap();
    assert_eq!(render(&confirmed.into_plan()), expected);
    // A well-formed wire object is not trusted just because a GUI produced it.
    let mut received: RawRequest = serde_json::from_slice(&json).unwrap();
    received.disk.path = "/dev/sda".into();
    assert!(received.parse_confirmed(&settings).is_err());
    let mut received: RawRequest = serde_json::from_slice(&json).unwrap();
    received.username = "root".into();
    assert!(received.parse_confirmed(&settings).is_err());
}

#[test]
fn github_choices_survive_review_and_privileged_reparsing() {
    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3";
    let (_dir, settings, mut raw) = fixture();
    raw.github_user = "alice".into();
    raw.ssh_keys = vec![KEY.into()];
    raw.ssh_server = true;
    raw.git_name = "Alice".into();
    raw.git_email = "alice@example.com".into();
    let plan = raw.parse(&settings).unwrap();
    let expected = render(&plan);
    assert!(expected.contains(KEY) && expected.contains("services.openssh"));
    let wire = plan.confirm("ERASE /dev/vda").unwrap().into_request();
    let encoded = serde_json::to_vec(&wire).unwrap();
    let received: RawRequest = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        render(&received.parse_confirmed(&settings).unwrap().into_plan()),
        expected
    );
    // The helper rejects keys it would not write, whatever the GUI fetched.
    let tampering: [fn(&mut RawRequest); 5] = [
        |r: &mut RawRequest| r.github_user.clear(),
        |r: &mut RawRequest| r.github_user = "-alice".into(),
        |r: &mut RawRequest| r.ssh_keys.push("ssh-ed25519 AAAA\nrm -rf /".into()),
        |r: &mut RawRequest| r.ssh_keys[0] = format!("command=\"sh\" {}", r.ssh_keys[0]),
        |r: &mut RawRequest| r.git_email.clear(),
    ];
    for tamper in tampering {
        let mut received: RawRequest = serde_json::from_slice(&encoded).unwrap();
        tamper(&mut received);
        assert!(received.parse_confirmed(&settings).is_err());
    }
    // Requests from before the GitHub step parse with none of it.
    let mut old: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    for field in [
        "github_user",
        "ssh_keys",
        "ssh_server",
        "git_name",
        "git_email",
    ] {
        old.as_object_mut().unwrap().remove(field);
    }
    let plan = serde_json::from_value::<RawRequest>(old)
        .unwrap()
        .parse_confirmed(&settings)
        .unwrap()
        .into_plan();
    assert_eq!(plan.onboarding(), &github::Onboarding::default());
}

#[test]
fn application_choices_survive_review_and_privileged_reparsing() {
    let (_dir, settings, mut raw) = fixture();
    raw.applications = vec!["rustup".into(), "codex".into()];
    let plan = raw.parse(&settings).unwrap();
    assert_eq!(
        plan.applications().ids(),
        ["codex", "build-tools", "rustup"]
    );
    let expected = render(&plan);
    let wire = plan.confirm("ERASE /dev/vda").unwrap().into_request();
    let encoded = serde_json::to_vec(&wire).unwrap();
    let received: RawRequest = serde_json::from_slice(&encoded).unwrap();
    let parsed = received.parse_confirmed(&settings).unwrap().into_plan();
    assert_eq!(render(&parsed), expected);
    assert!(
        expected.contains("calamares.applications = [ \"codex\" \"build-tools\" \"rustup\" ];")
    );
    let mut received: RawRequest = serde_json::from_slice(&encoded).unwrap();
    received.applications.push("google-chrome".into());
    assert!(received.parse_confirmed(&settings).is_err());
}

#[test]
fn omitted_applications_preserve_old_requests_but_empty_means_none() {
    let (_dir, settings, mut raw) = fixture();
    raw.applications.clear();
    let mut wire = serde_json::to_value(&raw).unwrap();
    let parsed: RawRequest = serde_json::from_value(wire.clone()).unwrap();
    assert!(
        parsed
            .parse(&settings)
            .unwrap()
            .applications()
            .selected()
            .is_empty()
    );
    wire.as_object_mut().unwrap().remove("applications");
    let parsed: RawRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(
        parsed.parse(&settings).unwrap().applications().ids(),
        ["firefox"]
    );
}

#[test]
fn every_desktop_subset_and_default_obeys_the_contract() {
    let mut supported = 0;
    for bits in 0u16..(1 << Desktop::ALL.len()) {
        let selected: Vec<_> = Desktop::ALL
            .iter()
            .enumerate()
            .filter_map(|(i, d)| (bits & (1 << i) != 0).then_some(*d))
            .collect();
        let compatible = !selected.is_empty();
        supported += usize::from(compatible);
        for default in Desktop::ALL {
            let parsed = DesktopSelection::parse(selected.clone(), default);
            assert_eq!(parsed.is_ok(), compatible && selected.contains(&default));
            if let Ok(parsed) = parsed {
                assert_eq!(parsed.selected(), selected);
                assert_eq!(parsed.default(), default);
            }
        }
    }
    // Every nonempty subset of the offered desktops.
    assert_eq!(supported, (1 << Desktop::ALL.len()) - 1);
    assert!(DesktopSelection::parse(vec![Desktop::Plasma; 2], Desktop::Plasma).is_err());
    assert!(DesktopSelection::parse(vec![Desktop::Plasma; 6], Desktop::Plasma).is_err());
}

#[test]
fn single_desktop_configs_keep_selected_session() {
    for desktop in Desktop::ALL {
        let (_dir, settings, mut raw) = fixture();
        raw.desktops = vec![desktop];
        raw.default_desktop = desktop;
        let text = render(&raw.parse(&settings).unwrap());
        assert!(text.contains(&format!("calamares.desktops = [ \"{}\" ];", desktop.id())));
        assert!(text.contains(&format!("calamares.defaultDesktop = \"{}\";", desktop.id())));
        assert!(text.contains("networking.networkmanager.enable = true;"));
        assert!(text.contains("hardware.enableRedistributableFirmware = true;"));
        assert!(!text.contains("networking.wireless.enable = false"));
    }
}

#[test]
fn unfree_default_is_enabled_but_explicit_opt_out_survives() {
    let (_dir, settings, mut raw) = fixture();
    raw.allow_unfree = DEFAULT_ALLOW_UNFREE;
    let text = render(&raw.parse(&settings).unwrap());
    assert!(text.contains("nixpkgs.config.allowUnfree = true;"));
    let (_dir, settings, mut raw) = fixture();
    raw.allow_unfree = false;
    let plan = raw.parse(&settings).unwrap();
    let wire = plan.confirm("ERASE /dev/vda").unwrap().into_request();
    let confirmed = wire.parse_confirmed(&settings).unwrap();
    let text = render(&confirmed.into_plan());
    assert!(text.contains("nixpkgs.config.allowUnfree = false;"));
    assert!(text.contains("hardware.enableRedistributableFirmware = true;"));
}

#[test]
fn names_have_checked_constructors() {
    for value in [
        "",
        "-host",
        "host-",
        "a/b",
        "${builtins.abort \"x\"}",
        &"x".repeat(64),
    ] {
        assert!(Hostname::parse(value).is_err());
    }
    for value in ["a", "Abc-123", &"x".repeat(63)] {
        assert!(Hostname::parse(value).is_ok());
    }
    for value in [
        "root",
        "nixos",
        "nixbld23",
        "User",
        "a;id",
        "a/b",
        "",
        &"a".repeat(32),
    ] {
        assert!(Username::parse(value).is_err());
    }
    for value in ["a", "alice-2_test", &"a".repeat(31)] {
        assert!(Username::parse(value).is_ok());
    }
}

#[test]
fn aggregate_parser_checks_ordinary_fields_without_needless_newtypes() {
    let invalid: &[(&str, &str)] = &[
        ("password", "short"),
        ("password", "new\nlineeeeeeeee"),
        ("password", ""),
        ("timezone", "../etc/shadow"),
        ("timezone", "/Etc/UTC"),
        ("timezone", "Etc//UTC"),
        ("timezone", "Etc/missing"),
        ("locale", "unsupported"),
        ("keyboard", "unsupported"),
        ("full_name", "Alice:extra"),
        ("full_name", "Alice\nextra"),
    ];
    for &(field, value) in invalid {
        let (_dir, settings, mut raw) = fixture();
        match field {
            "password" => raw.password = value.into(),
            "timezone" => raw.timezone = value.into(),
            "locale" => raw.locale = value.into(),
            "keyboard" => raw.keyboard = value.into(),
            "full_name" => raw.full_name = value.into(),
            _ => unreachable!(),
        }
        assert!(raw.parse(&settings).is_err(), "accepted invalid {field}");
    }
    for (password, valid) in [
        ("x".repeat(11), false),
        ("é".repeat(12), true),
        ("é".repeat(512), true),
        ("é".repeat(513), false),
    ] {
        let (_dir, settings, mut raw) = fixture();
        raw.password = password;
        assert_eq!(raw.parse(&settings).is_ok(), valid);
    }
    for (name, valid) in [
        ("", true),
        (&"x".repeat(128), true),
        (&"x".repeat(129), false),
    ] {
        let (_dir, settings, mut raw) = fixture();
        raw.full_name = name.into();
        assert_eq!(raw.parse(&settings).is_ok(), valid);
    }
}

#[test]
fn wifi_opt_out_and_empty_snapshot_are_different_states() {
    let user = Username::parse("alice").unwrap();
    let skip = wifi::WifiTransfer::parse(false, vec![], &user).unwrap();
    let copy = wifi::WifiTransfer::parse(true, vec![], &user).unwrap();
    assert!(!skip.enabled());
    assert!(copy.enabled());
    assert_eq!(skip.profile_count(), 0);
    assert_eq!(copy.profile_count(), 0);
    for (enabled, profiles) in [
        (false, vec!["private data".into()]),
        (true, vec![String::new(); 33]),
        (true, vec!["x".repeat(16385)]),
    ] {
        assert!(wifi::WifiTransfer::parse(enabled, profiles, &user).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    skip.write_to(root.path()).unwrap();
    copy.write_to(root.path()).unwrap();
    assert!(!root.path().join("etc").exists());
}

#[test]
fn config_contains_no_password_and_no_test_services() {
    let (_dir, settings, raw) = fixture();
    let secret = raw.password.clone();
    let text = render(&raw.parse(&settings).unwrap());
    assert!(!text.contains(&secret));
    assert!(!text.contains("qemuGuest"));
    assert!(text.contains("hashedPasswordFile = \"/etc/nixos-secrets/"));
    assert!(text.contains("systemd-boot.enable = true"));
    assert!(!text.contains("grub.device"));
    assert!(text.contains("\\${literal}"));
}

#[test]
fn plan_keeps_settings_used_at_parse_and_wire_cannot_inject_them() {
    let (_dir, mut settings, mut raw) = fixture();
    raw.firmware = Firmware::Bios;
    settings.kernel = Kernel::Latest;
    settings.test_diagnostics = true;
    let mut value = serde_json::to_value(&raw).unwrap();
    value["test_diagnostics"] = true.into();
    assert!(serde_json::from_value::<RawRequest>(value).is_err());
    let plan = raw.parse(&settings).unwrap();
    settings.kernel = Kernel::Lts;
    settings.test_diagnostics = false;
    let text = render(&plan);
    assert!(text.contains("grub.device = \"/dev/vda\""));
    assert!(text.contains("linuxPackages_latest"));
    assert!(text.contains("qemuGuest.enable = true"));
}

#[test]
fn hashing_authenticates_only_correct_password() {
    let params = sha_crypt::Sha512Params::new(1000).unwrap();
    let a = sha_crypt::sha512_simple("secret", &params).unwrap();
    let b = sha_crypt::sha512_simple("secret", &params).unwrap();
    assert_ne!(a, b);
    sha_crypt::sha512_check("secret", &a).unwrap();
    assert!(sha_crypt::sha512_check("wrong", &a).is_err());
}

#[test]
fn configuration_requires_identities_for_the_reviewed_firmware_and_swap() {
    for filesystem in Filesystem::ALL {
        for firmware in [Firmware::Uefi, Firmware::Bios] {
            for swap in [true, false] {
                let (_dir, settings, mut raw) = fixture();
                raw.filesystem = filesystem;
                raw.firmware = firmware;
                raw.swap = swap;
                let plan = raw.parse(&settings).unwrap();
                let good = ids(&plan);
                let text = config::configuration(&plan, &good).unwrap();
                assert!(text.contains(&format!(
                    "device = lib.mkForce \"/dev/disk/by-uuid/{ROOT}\""
                )));
                assert!(text.contains(&format!("fsType = \"{}\"", filesystem.name())));
                assert!(text.contains("\"noatime\""));
                assert_eq!(
                    text.contains("/dev/disk/by-uuid/A1B2-C3D4"),
                    firmware == Firmware::Uefi
                );
                assert_eq!(
                    text.contains(&format!(
                        "boot.resumeDevice = \"/dev/disk/by-uuid/{SWAP}\";"
                    )),
                    swap
                );
                assert_eq!(text.contains("calamares.zswap.enable = true;"), swap);
                assert_eq!(text.contains("swapDevices = lib.mkForce [ ];"), !swap);
                for bad in [
                    Identities {
                        root: "A1B2-C3D4".into(),
                        ..good.clone()
                    },
                    Identities {
                        efi: good.efi.is_none().then(|| "A1B2-C3D4".into()),
                        ..good.clone()
                    },
                    Identities {
                        swap: good.swap.is_none().then(|| SWAP.into()),
                        ..good.clone()
                    },
                    Identities {
                        efi: good.efi.as_ref().map(|_| ROOT.into()),
                        ..good.clone()
                    },
                ] {
                    if bad != good {
                        assert!(config::configuration(&plan, &bad).is_err());
                    }
                }
            }
        }
    }
}

#[test]
fn swap_and_tuning_choices_survive_ipc_and_default_on_for_old_requests() {
    let (_dir, settings, mut raw) = fixture();
    raw.swap = false;
    raw.tuning = false;
    let plan = raw.parse(&settings).unwrap();
    assert!(render(&plan).contains("calamares.tuning.enable = false;"));
    let wire = plan.into_request();
    assert!(wire.confirmation.is_empty());
    let received: RawRequest = serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
    let plan = received.parse(&settings).unwrap();
    assert!(!plan.swap() && !plan.tuning());
    let (_dir, settings, raw) = fixture();
    let mut value = serde_json::to_value(&raw).unwrap();
    value.as_object_mut().unwrap().remove("swap");
    value.as_object_mut().unwrap().remove("tuning");
    let old: RawRequest = serde_json::from_value(value).unwrap();
    let plan = old.parse(&settings).unwrap();
    assert!(plan.swap() && plan.tuning());
    assert!(render(&plan).contains("calamares.tuning.enable = true;"));
}

#[test]
fn filesystem_choice_survives_review_confirmation_and_ipc() {
    for filesystem in Filesystem::ALL {
        let (_dir, settings, mut raw) = fixture();
        raw.filesystem = filesystem;
        let plan = raw.parse(&settings).unwrap();
        assert_eq!(plan.filesystem(), filesystem);
        assert_eq!(
            render(&plan).contains("compress=zstd"),
            filesystem == Filesystem::Btrfs
        );
        let wire = plan.confirm("ERASE /dev/vda").unwrap().into_request();
        let received: RawRequest =
            serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
        assert_eq!(
            received
                .parse_confirmed(&settings)
                .unwrap()
                .into_plan()
                .filesystem(),
            filesystem
        );
    }
    let (_dir, _settings, raw) = fixture();
    let mut value = serde_json::to_value(&raw).unwrap();
    value.as_object_mut().unwrap().remove("filesystem");
    assert_eq!(
        serde_json::from_value::<RawRequest>(value.clone())
            .unwrap()
            .filesystem,
        Filesystem::Ext4
    );
    for bad in ["ntfs", "auto", "ext4,xfs", "", "EXT4"] {
        value["filesystem"] = bad.into();
        assert!(serde_json::from_value::<RawRequest>(value.clone()).is_err());
    }
}

#[test]
fn nvidia_gpus_get_nvidia_driver_only_when_unfree_is_allowed() {
    use graphics::{Graphics, Integrated};
    let hybrid = |offload| Graphics {
        nvidia: Some("PCI:1:0:0".into()),
        integrated: Some(Integrated::Intel("PCI:0:2:0".into())),
        offload,
    };
    let prime =
        "calamares.nvidia.prime = { nvidiaBusId = \"PCI:1:0:0\"; intelBusId = \"PCI:0:2:0\"; };";
    // Displays on the Intel GPU: PRIME offload.
    let (_dir, settings, mut raw) = fixture();
    raw.allow_unfree = true;
    raw.graphics = hybrid(true);
    let text = render(&raw.parse(&settings).unwrap());
    assert!(text.contains("  calamares.nvidia.enable = true;\n"));
    assert!(text.contains(&format!("\n  {prime}\n")));
    // Panel on the NVIDIA GPU (MUX in discrete mode): NVIDIA is primary.
    let (_dir, settings, mut raw) = fixture();
    raw.allow_unfree = true;
    raw.graphics = hybrid(false);
    let text = render(&raw.parse(&settings).unwrap());
    assert!(text.contains("  calamares.nvidia.enable = true;\n"));
    assert!(!text.contains("calamares.nvidia.prime"));
    // Declining unfree packages keeps the open-source driver.
    let (_dir, settings, mut raw) = fixture();
    raw.graphics = hybrid(true);
    let text = render(&raw.parse(&settings).unwrap());
    assert!(!text.contains("calamares.nvidia"));
    assert!(text.contains("its driver needs unfree packages"));
    // No NVIDIA GPU, nothing written; forged bus IDs are rejected.
    let (_dir, settings, raw) = fixture();
    assert!(!render(&raw.parse(&settings).unwrap()).contains("NVIDIA"));
    let (_dir, settings, mut raw) = fixture();
    raw.allow_unfree = true;
    raw.graphics = Graphics {
        nvidia: Some("PCI:1:0:0\"; boot.kernelParams = [ \"x\" ]; #".into()),
        integrated: None,
        offload: false,
    };
    assert!(raw.parse(&settings).is_err());
}
