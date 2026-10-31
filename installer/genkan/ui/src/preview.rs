// SPDX-License-Identifier: GPL-3.0-or-later
//! Pages with sample data, for screenshots outside the live ISO and for the
//! tests. Never touches disks: the sample's review has no helper behind it,
//! and confirming it only shows the Install page.
//!
//! In debug builds, `CALAMARES_UI_PREVIEW=welcome|disk|account|github|
//! desktop|applications|location|keyboard|review|ready|prepfail|install|done|failed`
//! opens the window on that page; `CALAMARES_UI_SIZE=800x600` sizes it.
//! Release builds never read either.
use crate::app::{Genkan, Msg, Review, Step};
use crate::zone::detection_note;
use aizome_ipc::{KeyboardBus, KeyboardInfo};
use aizome_kit::prelude::*;
use calamares_nixos::{
    Desktop, Filesystem, Firmware, SuperKey,
    disk::{self, Disk},
    github::{self, GithubUser},
    install::{Event, Summary},
    memory::MemInfo,
    timezone::Source,
};

/// The page `CALAMARES_UI_PREVIEW` asks for, in debug builds.
pub fn requested() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("CALAMARES_UI_PREVIEW").ok()
    } else {
        None
    }
}

pub fn sample_disk(path: &str, model: &str, gib: u64, serial: &str, blocked: Option<&str>) -> Disk {
    Disk {
        identity: disk::Identity {
            path: path.into(),
            major_minor: "259:0".into(),
            bytes: gib * 1024u64.pow(3),
            serial: serial.into(),
            wwn: String::new(),
            model: model.into(),
        },
        blocked: blocked.map(str::to_owned),
    }
}

/// `page` with the sample, at `CALAMARES_UI_SIZE` if it's set.
pub fn apply(app: &mut Genkan, cx: &mut Cx<Msg>, page: &str) {
    if let (Some(window), Some((width, height))) = (
        app.window,
        std::env::var("CALAMARES_UI_SIZE")
            .ok()
            .and_then(|size| size.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
            .and_then(|(w, h)| Some((w.ok()?, h.ok()?))),
    ) {
        cx.request_size(window, Size::new(width, height));
    }
    show(app, cx, page);
}

/// Fill the program with the sample and go to `page`.
pub fn show(app: &mut Genkan, cx: &mut Cx<Msg>, page: &str) {
    let gib = 1024u64.pow(3);
    app.update(
        Msg::Memory(Ok(MemInfo {
            total: 31 * gib + 400 * 1024 * 1024,
            available: 24 * gib,
            free: 20 * gib,
        })),
        cx,
    );
    app.update(
        Msg::Scanned(Ok((
            vec![
                sample_disk(
                    "/dev/nvme0n1",
                    "Samsung SSD 990 PRO 1TB",
                    931,
                    "S7DNNJ0X123456",
                    None,
                ),
                sample_disk(
                    "/dev/nvme1n1",
                    "WD Blue SN580 500GB",
                    465,
                    "23401A800123",
                    None,
                ),
                sample_disk(
                    "/dev/sda",
                    "SanDisk 3.2Gen1",
                    114,
                    "0401be1c",
                    Some("Contains a mounted filesystem or active swap (including live media)"),
                ),
            ],
            Firmware::Uefi,
        ))),
        cx,
    );
    app.update(Msg::SelectDisk(0), cx);
    app.update(Msg::FullName("Ada Lovelace".into()), cx);
    app.update(Msg::Password("correct horse battery".into()), cx);
    app.update(Msg::Repeat("correct horse battery".into()), cx);
    app.update(Msg::Hostname("analytical-engine".into()), cx);
    // A GitHub lookup as it answers, without the network. Sample keys only.
    app.update(Msg::GithubUser("adalovelace".into()), cx);
    app.github_pending = Some(0);
    let profile = github::Profile {
        user: GithubUser::parse("adalovelace").expect("a valid username"),
        keys: [
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3",
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFaKW8Z6YOoa5UaaVHwOgDXPe4PcYk663+2rvgwaCVo0QOYEMFJWQ4VZL6GKzJuSioxTotYTGYsKGOv45zDqz0A=",
        ]
        .iter()
        .map(|key| github::AuthorizedKey::parse(key).expect("a sample key"))
        .collect(),
        skipped: 0,
        name: Some("Ada Lovelace".into()),
        email: Some("1815+adalovelace@users.noreply.github.com".into()),
        private_email: true,
    };
    app.update(Msg::Github(0, Ok(profile)), cx);
    app.update(Msg::SshServer(true), cx);
    if let Some(index) = app.zones.as_ref().and_then(|z| z.find("America/Chicago")) {
        app.show_zone(index, &detection_note(Source::Internet("geoip.kde.org")));
    }
    // Sample selections would warm the cache; show a fixed indicator instead.
    app.update(
        Msg::Warm {
            generation: 0,
            read: 2400 * 1024 * 1024,
            total: 6100 * 1024 * 1024,
            done: false,
        },
        cx,
    );
    // The key beside a ThinkPad's space bar, as the keyboard step heard it.
    app.caught(
        56,
        Some(KeyboardInfo {
            name: "AT Translated Set 2 keyboard".into(),
            id: "0001:0001".into(),
            bus: KeyboardBus::BuiltIn,
            last: true,
        }),
    );
    app.reached = Step::Keyboard;
    let step = match page {
        "disk" => Step::Disk,
        "account" => Step::Account,
        "github" => Step::GitHub,
        "desktop" => Step::Desktop,
        "applications" => Step::Applications,
        "location" => Step::Location,
        "keyboard" => Step::Keyboard,
        "review" | "ready" | "prepfail" => Step::Review,
        "install" | "done" | "failed" => Step::Install,
        _ => Step::Welcome,
    };
    if step < Step::Review {
        app.go(step, cx);
        return;
    }
    app.activity = None;
    app.show_review(Review {
        encrypt: false,
        disk: sample_disk(
            "/dev/nvme0n1",
            "Samsung SSD 990 PRO 1TB",
            931,
            "S7DNNJ0X123456",
            None,
        )
        .identity,
        firmware: Firmware::Uefi,
        filesystem: Filesystem::Btrfs,
        hostname: "analytical-engine".into(),
        username: "ada".into(),
        full_name: "Ada Lovelace".into(),
        locale: "en_US.UTF-8".into(),
        timezone: "America/Chicago".into(),
        keyboard: "us".into(),
        super_key: SuperKey::SwapAlt,
        desktops: vec![Desktop::Aizome, Desktop::Tatami],
        default_desktop: Desktop::Aizome,
        applications: "Firefox, Ghostty, Development build tools, Rustup".into(),
        wifi: true,
        wifi_profiles: 2,
        unfree: true,
        swap: true,
        tuning: true,
        graphics: calamares_nixos::graphics::Graphics {
            nvidia: Some("PCI:1:0:0".into()),
            integrated: Some(calamares_nixos::graphics::Integrated::Intel(
                "PCI:0:2:0".into(),
            )),
            offload: false,
        }
        .describe(true),
        ssh_keys: "2 keys from github.com/adalovelace".into(),
        ssh_server: "OpenSSH, key-only login".into(),
        git: "Ada Lovelace <1815+adalovelace@users.noreply.github.com>".into(),
    });
    app.go(Step::Review, cx);
    app.helper_event(
        Event::Preparing {
            message: "Caching the installed system in memory".into(),
        },
        cx,
    );
    app.helper_event(
        Event::Caching {
            read: 2600 * 1024 * 1024,
            total: 6300 * 1024 * 1024,
        },
        cx,
    );
    if page == "prepfail" {
        app.helper_event(
            Event::Failed {
                message: "The selected configuration could not be evaluated; the target disk has not been erased: nix failed (exit status: 1): error: sample preview failure".into(),
            },
            cx,
        );
        return;
    }
    if page != "review" {
        app.helper_event(
            Event::Prepared {
                summary: Summary {
                    closure_bytes: 6300 * 1024 * 1024,
                    cached_bytes: 6300 * 1024 * 1024,
                    cache_limited: false,
                    downloaded_bytes: 0,
                    deferred: false,
                    root_bytes: 898 * gib,
                    swap_bytes: 32 * gib,
                    seconds: 52.4,
                },
            },
            cx,
        );
        let phrase = app.phrase.clone();
        app.update(Msg::Erase(phrase), cx);
        app.update(Msg::Consent(true), cx);
    }
    if step == Step::Review {
        return;
    }
    app.update(Msg::Next, cx);
    app.elapsed = 23;
    for (step, message) in [
        (
            1,
            "Erasing the selected disk and creating a GPT partition table",
        ),
        (2, "Formatting, verifying and mounting the new filesystems"),
        (3, "Writing the configuration, account and Wi-Fi settings"),
        (4, "Copying the prepared system to the new disk"),
    ] {
        app.helper_event(
            Event::Progress {
                step,
                message: message.into(),
            },
            cx,
        );
    }
    for line in [
        "copying path '/nix/store/…-linux-7.2.8' from 'auto'",
        "copying path '/nix/store/…-aizome-0.1.0' from 'auto'",
        "copying path '/nix/store/…-firefox-153.0' from 'auto'",
    ] {
        app.helper_event(Event::Log { line: line.into() }, cx);
    }
    app.helper_event(
        Event::Copying {
            bytes: 3900 * 1024 * 1024,
            total: 6300 * 1024 * 1024,
        },
        cx,
    );
    match page {
        "done" => {
            app.helper_event(
                Event::Timing {
                    stage: "install".into(),
                    seconds: 47.3,
                },
                cx,
            );
            app.helper_event(Event::Complete, cx);
        }
        "failed" => app.helper_event(
            Event::Failed {
                message: "nix failed (exit status: 1): error: writing to file: No space left on device (sample preview failure)".into(),
            },
            cx,
        ),
        _ => {}
    }
}
