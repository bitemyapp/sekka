// SPDX-License-Identifier: GPL-3.0-or-later
//! NixOS option semantics adapted from calamares-nixos-extensions 0.3.23.
//! No Python/C++ module or global-storage hooks are executed by this installer.
use crate::{
    Firmware, Hostname, InstallPlan, Kernel, Settings, filesystem::Identities, read_trusted,
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

pub fn nix_string(value: &str) -> String {
    // JSON quoting does not escape Nix interpolation. Escape dollars *after*
    // JSON encoding so a literal ${...} cannot become an executable expression.
    serde_json::to_string(value)
        .expect("string serialization")
        .replace("${", "\\${")
}

/// Inputs of the installed flake besides Sekka, which supplies all first-party
/// system modules. Each follows Sekka's own, so updating Sekka moves them only
/// to the revisions its nightly build checked and cached.
const FOLLOWED_INPUTS: [&str; 6] = [
    "nixpkgs",
    "determinate",
    "fh",
    "applications",
    "ai-apps",
    "omp",
];

pub struct Template {
    pub flake: String,
    pub lock: Vec<u8>,
}
impl Template {
    pub fn load(settings: &Settings, hostname: &Hostname) -> Result<Self> {
        let dir = Path::new(&settings.template_dir);
        let template = String::from_utf8(read_trusted(&dir.join("flake.nix.in"))?)?;
        let lock = read_trusted(&dir.join("flake.lock"))?;
        Self::parse(template, lock, hostname)
    }
    pub fn parse(template: String, lock: Vec<u8>, hostname: &Hostname) -> Result<Self> {
        ensure!(
            template.matches("@HOSTNAME@").count() == 1,
            "Template must contain exactly one hostname placeholder"
        );
        let value: serde_json::Value = serde_json::from_slice(&lock)?;
        ensure!(value["version"] == 7, "Unsupported lock version");
        let root = value["root"].as_str().context("Missing lock root")?;
        let inputs = value["nodes"][root]["inputs"]
            .as_object()
            .context("Missing inputs")?;
        let mut expected = BTreeSet::from(FOLLOWED_INPUTS);
        expected.insert("sekka");
        ensure!(
            inputs.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected,
            "Unexpected target flake inputs"
        );
        let locked = |node: &str| {
            value["nodes"][node]["locked"]["narHash"]
                .as_str()
                .is_some_and(|s| s.starts_with("sha256-"))
        };
        let sekka = inputs["sekka"]
            .as_str()
            .context("Expected Sekka to be locked directly")?;
        ensure!(locked(sekka), "Unlocked input: sekka");
        for name in FOLLOWED_INPUTS {
            ensure!(
                inputs[name] == serde_json::json!(["sekka", name]),
                "{name} must follow Sekka's"
            );
            let node = value["nodes"][sekka]["inputs"][name]
                .as_str()
                .context("Expected Sekka's input to be locked directly")?;
            ensure!(locked(node), "Unlocked input: sekka/{name}");
        }
        Ok(Self {
            flake: template.replace("@HOSTNAME@", &nix_string(hostname.as_str())),
            lock,
        })
    }
    /// The system modules come from the locked `calamares` input, not from
    /// files written here, so they can be updated after installation.
    pub fn write(&self, dir: &Path) -> Result<()> {
        fs::write(dir.join("flake.nix"), &self.flake)?;
        fs::write(dir.join("flake.lock"), &self.lock)?;
        Ok(())
    }
}

/// The installed system's configuration.nix. Filesystem identities are chosen
/// before erasure, requested when formatting and verified from each superblock,
/// so the complete system can be built while nothing has been written yet.
/// They are pinned with mkForce: hardware detection run later must not select
/// an obsolete /dev/disk/by-uuid alias.
pub fn configuration(request: &InstallPlan, ids: &Identities) -> Result<String> {
    let settings = request.settings();
    let q = nix_string;
    let by_uuid = |uuid: &str| q(&format!("/dev/disk/by-uuid/{uuid}"));
    crate::filesystem::check_uuid(&ids.root, request.filesystem().name())?;
    ensure!(
        ids.efi.is_some() == (request.firmware() == Firmware::Uefi),
        "EFI filesystem identity does not match the reviewed firmware"
    );
    ensure!(
        ids.swap.is_some() == request.swap(),
        "Swap identity does not match the reviewed choice"
    );
    let boot = match request.firmware() {
        Firmware::Uefi => "boot.loader.systemd-boot.enable = true;\n  boot.loader.efi.canTouchEfiVariables = true;".into(),
        Firmware::Bios => format!("boot.loader.grub.enable = true;\n  boot.loader.grub.device = {};\n  boot.loader.grub.useOSProber = false;", q(&request.disk().path)),
    };
    let kernel = match settings.kernel {
        Kernel::Latest => "  boot.kernelPackages = pkgs.linuxPackages_latest;\n",
        Kernel::Lts => "",
    };
    let mut storage = format!(
        "  fileSystems.\"/\" = {{\n    device = lib.mkForce {};\n    fsType = {};\n    options = [ {} ];\n  }};\n",
        by_uuid(&ids.root),
        q(request.filesystem().name()),
        request
            .filesystem()
            .mount_options()
            .iter()
            .map(|o| q(o))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if let Some(serial) = &ids.efi {
        crate::filesystem::check_uuid(serial, "vfat")?;
        storage.push_str(&format!(
            "  fileSystems.\"/boot\" = {{\n    device = lib.mkForce {};\n    fsType = \"vfat\";\n    options = [ \"fmask=0077\" \"dmask=0077\" ];\n  }};\n",
            by_uuid(serial)
        ));
    }
    match &ids.swap {
        Some(uuid) => {
            crate::filesystem::check_uuid(uuid, "swap")?;
            storage.push_str(&format!(
                "  # Swap partition matched to installed RAM, also used to resume from hibernation.\n  swapDevices = lib.mkForce [ {{ device = {0}; }} ];\n  boot.resumeDevice = {0};\n  calamares.zswap.enable = true;\n",
                by_uuid(uuid)
            ));
        }
        None => storage.push_str("  swapDevices = lib.mkForce [ ];\n"),
    }
    let graphics = match (&request.graphics().nvidia, request.allow_unfree()) {
        (Some(nvidia), true) => {
            let mut text = String::from(
                "  # NVIDIA's driver (latest release, open kernel modules) instead of nouveau.\n  calamares.nvidia.enable = true;\n",
            );
            match (&request.graphics().integrated, request.graphics().offload) {
                (Some(gpu), true) => text.push_str(&format!(
                    "  # The displays are on the integrated GPU; nvidia-offload runs a program on the NVIDIA GPU.\n  calamares.nvidia.prime = {{ nvidiaBusId = {}; {} = {}; }};\n",
                    q(nvidia),
                    gpu.option(),
                    q(gpu.bus_id())
                )),
                (Some(_), false) => text.push_str(
                    "  # The displays are on the NVIDIA GPU, so it is the primary GPU.\n",
                ),
                (None, _) => {}
            }
            text
        }
        (Some(_), false) => {
            "  # An NVIDIA GPU was found, but its driver needs unfree packages, which were declined.\n".into()
        }
        (None, _) => String::new(),
    };
    // The GitHub step: authorized keys, the SSH server and a Git identity.
    let onboarding = request.onboarding();
    let authorized_keys = match onboarding.github() {
        Some(user) if !onboarding.keys().is_empty() => format!(
            "    # Public SSH keys of github.com/{}, fetched during installation.\n    openssh.authorizedKeys.keys = [\n{}    ];\n",
            user.as_str(),
            onboarding
                .keys()
                .iter()
                .map(|key| format!("      {}\n", q(&key.text())))
                .collect::<String>()
        ),
        _ => String::new(),
    };
    let ssh = match (onboarding.ssh_server(), onboarding.password_login()) {
        (false, _) => "",
        (true, false) => {
            "  # The SSH server, for the authorized keys only: passwords are refused.\n  services.openssh = {\n    enable = true;\n    settings.PasswordAuthentication = false;\n    settings.KbdInteractiveAuthentication = false;\n  };\n"
        }
        (true, true) => {
            "  # The SSH server, with password login: no keys were authorized.\n  services.openssh.enable = true;\n"
        }
    };
    let git = onboarding.git().map_or_else(String::new, |identity| {
        format!(
            "  # Git's name and address for commits, for every account (/etc/gitconfig);\n  # `git config --global` sets your own.\n  programs.git = {{\n    enable = true;\n    config.user = {{ name = {}; email = {}; }};\n  }};\n",
            q(identity.name()),
            q(identity.email())
        )
    });
    let diagnostic = if settings.test_diagnostics {
        "  # Disposable QEMU verification only.\n  services.qemuGuest.enable = true;\n  boot.kernelParams = [ \"console=ttyS0,115200n8\" \"console=tty0\" ];\n"
    } else {
        ""
    };
    let list = |items: Vec<String>| items.iter().map(|i| q(i)).collect::<Vec<_>>().join(" ");
    Ok(format!(
        r#"# Generated by the NixOS-focused Rust Calamares installer.
{{ config, lib, pkgs, ... }}: {{
  # The calamares.* options come from the flake's `calamares` input.
  imports = [ ./hardware-configuration.nix ];
  calamares.installUser = {username};
  calamares.applications = [ {applications} ];
  calamares.desktops = [ {desktops} ];
  calamares.defaultDesktop = {default_desktop};
  # CachyOS-inspired kernel, memory, I/O and service defaults (rust/system/tuning.nix in the calamares input).
  calamares.tuning.enable = {tuning};
  {boot}
{kernel}{storage}{graphics}  networking.hostName = {hostname};
  networking.networkmanager.enable = true;
  # Include redistributable device firmware even when additional unfree
  # packages are declined. This is not a strictly free-software-only system.
  hardware.enableRedistributableFirmware = true;
  time.timeZone = {timezone};
  i18n.defaultLocale = {locale};
  services.xserver.xkb.layout = {keyboard};
  console.useXkbConfig = true;
  services.printing.enable = true;
  security.rtkit.enable = true;
  services.pipewire = {{ enable = true; alsa.enable = true; alsa.support32Bit = true; pulse.enable = true; }};
  users.mutableUsers = true;
  users.users.root.initialHashedPassword = "!";
  users.users.{username} = {{
    isNormalUser = true;
    description = {full_name};
    extraGroups = [ "networkmanager" "wheel" ];
    # A root-only file on the target, never a Nix store source path.
    hashedPasswordFile = "/etc/nixos-secrets/user-password.hash";
{authorized_keys}  }};
{ssh}{git}  nixpkgs.config.allowUnfree = {unfree};
  system.stateVersion = {state};
{diagnostic}}}
"#,
        hostname = q(request.hostname().as_str()),
        applications = list(request.applications().ids()),
        desktops = list(
            request
                .desktops()
                .selected()
                .iter()
                .map(|d| d.id().to_owned())
                .collect()
        ),
        default_desktop = q(request.desktops().default().id()),
        tuning = request.tuning(),
        timezone = q(request.timezone().as_str()),
        locale = q(request.locale()),
        keyboard = q(request.keyboard()),
        username = q(request.username().as_str()),
        full_name = q(request.full_name()),
        unfree = request.allow_unfree(),
        state = q(&settings.state_version)
    ))
}

pub fn write_secret(path: &Path, hash: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    writeln!(file, "{hash}")?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_interpolation_and_quotes() {
        assert_eq!(nix_string("${abort \"x\"}"), "\"\\${abort \\\"x\\\"}\"");
        assert_eq!(nix_string("x\\y"), "\"x\\\\y\"");
    }
    #[test]
    fn lock_and_placeholder_are_mandatory() {
        let hostname = Hostname::parse("test").unwrap();
        assert!(Template::parse("no placeholder".into(), b"{}".to_vec(), &hostname).is_err());
        assert!(
            Template::parse(
                "@HOSTNAME@".into(),
                br#"{"version":7,"root":"root","nodes":{"root":{"inputs":{}}}}"#.to_vec(),
                &hostname
            )
            .is_err()
        );
        let following: &str = r#"{"version":7,"root":"root","nodes":{
            "root":{"inputs":{"sekka":"sekka","nixpkgs":["sekka","nixpkgs"],"determinate":["sekka","determinate"],
              "fh":["sekka","fh"],"applications":["sekka","applications"],"ai-apps":["sekka","ai-apps"],"omp":["sekka","omp"]}},
            "sekka":{"locked":{"narHash":"sha256-s"},"inputs":{"nixpkgs":"n","determinate":"d","fh":"f",
              "applications":"a","ai-apps":"i","omp":"o"}},
            "n":{"locked":{"narHash":"sha256-n"}},"d":{"locked":{"narHash":"sha256-d"}},
            "f":{"locked":{"narHash":"sha256-f"}},"a":{"locked":{"narHash":"sha256-a"}},
            "i":{"locked":{"narHash":"sha256-i"}},"o":{"locked":{"narHash":"sha256-o"}}}}"#;
        assert!(
            Template::parse(
                "@HOSTNAME@".into(),
                following.as_bytes().to_vec(),
                &hostname
            )
            .is_ok()
        );
        let own = following.replace(r#""nixpkgs":["sekka","nixpkgs"]"#, r#""nixpkgs":"n""#);
        assert!(Template::parse("@HOSTNAME@".into(), own.into_bytes(), &hostname).is_err());
        let unlocked = following.replace(r#""sha256-i""#, "null");
        assert!(Template::parse("@HOSTNAME@".into(), unlocked.into_bytes(), &hostname).is_err());
    }
    /// The installation media prebuild rust/reference.nix so installations
    /// find their packages already present. Settings that affect which
    /// packages an installed system contains must match between the two.
    fn test_request() -> crate::RawRequest {
        crate::RawRequest {
            disk: crate::disk::Identity {
                path: "/dev/vda".into(),
                major_minor: "252:0".into(),
                bytes: 64 * 1024u64.pow(3),
                serial: "test".into(),
                wwn: "".into(),
                model: "test".into(),
            },
            firmware: Firmware::Uefi,
            filesystem: crate::Filesystem::Ext4,
            hostname: "nixos".into(),
            username: "alice".into(),
            full_name: "".into(),
            password: "long-enough-test-password".into(),
            locale: "en_US.UTF-8".into(),
            timezone: "Etc/UTC".into(),
            keyboard: "us".into(),
            desktops: vec![crate::Desktop::Plasma],
            default_desktop: crate::Desktop::Plasma,
            applications: crate::applications::default_selection(),
            copy_wifi: false,
            wifi_profiles: vec![],
            allow_unfree: true,
            swap: true,
            tuning: true,
            graphics: Default::default(),
            github_user: String::new(),
            ssh_keys: vec![],
            ssh_server: false,
            git_name: String::new(),
            git_email: String::new(),
            confirmation: String::new(),
        }
    }
    fn generate(request: crate::RawRequest) -> String {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Etc")).unwrap();
        std::fs::write(dir.path().join("Etc/UTC"), "TZif").unwrap();
        let settings = Settings {
            template_dir: "/unused".into(),
            zoneinfo: dir.path().to_string_lossy().into_owned(),
            state_version: "26.11".into(),
            kernel: Kernel::Latest,
            test_diagnostics: false,
        };
        let ids = Identities {
            root: "00000000-0000-4000-8000-000000000000".into(),
            efi: Some("0000-0000".into()),
            swap: Some("00000000-0000-4000-8000-000000000001".into()),
        };
        configuration(&request.parse(&settings).unwrap(), &ids).unwrap()
    }
    #[test]
    fn reference_system_shares_closure_relevant_settings() {
        let squash = |text: &str| text.split_whitespace().collect::<String>();
        let reference = squash(include_str!("../reference.nix"));
        let generated = squash(&generate(test_request()));
        for fragment in [
            "calamares.applications=",
            "calamares.desktops=",
            "calamares.tuning.enable=",
            "boot.loader.systemd-boot.enable=true;",
            "boot.loader.efi.canTouchEfiVariables=true;",
            "pkgs.linuxPackages_latest",
            "options=[\"fmask=0077\"\"dmask=0077\"];",
            "calamares.zswap.enable=",
            "networking.networkmanager.enable=true;",
            "hardware.enableRedistributableFirmware=true;",
            "console.useXkbConfig=true;",
            "services.printing.enable=true;",
            "security.rtkit.enable=true;",
            "enable=true;alsa.enable=true;alsa.support32Bit=true;pulse.enable=true;",
            "users.mutableUsers=true;",
            "users.users.root.initialHashedPassword=\"!\";",
            "extraGroups=[\"networkmanager\"\"wheel\"];",
            "hashedPasswordFile=\"/etc/nixos-secrets/user-password.hash\";",
            "nixpkgs.config.allowUnfree=true;",
        ] {
            assert!(generated.contains(fragment), "config.rs lacks {fragment}");
            assert!(
                reference.contains(fragment),
                "reference.nix lacks {fragment}"
            );
        }
    }
    #[test]
    fn github_step_writes_keys_server_and_git_identity() {
        const KEY: &str =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3";
        let plain = generate(test_request());
        for absent in ["openssh", "programs.git", "github.com/"] {
            assert!(!plain.contains(absent), "{absent} without the GitHub step");
        }
        let mut request = test_request();
        request.github_user = "ada-l".into();
        request.ssh_keys = vec![format!("{KEY} ada@laptop")];
        request.ssh_server = true;
        request.git_name = "Ada \"${x}\" Lovelace".into();
        request.git_email = "ada@example.com".into();
        let text = generate(request);
        assert!(text.contains(&format!(
            "    # Public SSH keys of github.com/ada-l, fetched during installation.\n    openssh.authorizedKeys.keys = [\n      \"{KEY}\"\n    ];\n  }};\n"
        )));
        assert!(text.contains("settings.PasswordAuthentication = false;"));
        assert!(text.contains(
            "config.user = { name = \"Ada \\\"\\${x}\\\" Lovelace\"; email = \"ada@example.com\"; };"
        ));
        // Without keys the server accepts passwords: the only way to log in.
        let mut request = test_request();
        request.ssh_server = true;
        let text = generate(request);
        assert!(text.contains("  services.openssh.enable = true;\n"));
        assert!(!text.contains("PasswordAuthentication") && !text.contains("authorizedKeys"));
    }
    #[test]
    fn secrets_are_exclusive_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hash");
        write_secret(&path, "$6$test").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_secret(&path, "overwrite").is_err());
    }
}
