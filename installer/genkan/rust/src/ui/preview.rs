// SPDX-License-Identifier: GPL-3.0-or-later
//! Debug builds only: render a page with sample data for screenshots outside
//! the live ISO. Never compiled into release builds and never touches disks.
//!
//! `CALAMARES_UI_PREVIEW=welcome|disk|account|github|desktop|applications|
//! location|review|ready|prepfail|install|done|failed`
use super::*;

fn sample_disk(path: &str, model: &str, gib: u64, serial: &str, blocked: Option<&str>) -> Disk {
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

fn event(ui: &Rc<Ui>, event: Event) {
    ui.handle(Message::Session(ui.generation.get(), Update::Event(event)));
}

/// `CALAMARES_UI_SIZE=800x600` checks the compact layout. Call before presenting.
pub fn resize(window: &adw::ApplicationWindow) {
    if let Some((width, height)) = std::env::var("CALAMARES_UI_SIZE")
        .ok()
        .and_then(|size| size.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
        .and_then(|(w, h)| Some((w.ok()?, h.ok()?)))
    {
        window.set_default_size(width, height);
    }
}

pub fn apply(ui: &Rc<Ui>) {
    let page = std::env::var("CALAMARES_UI_PREVIEW").unwrap_or_default();
    let gib = 1024u64.pow(3);
    ui.handle(Message::Memory(Ok(MemInfo {
        total: 31 * gib + 400 * 1024 * 1024,
        available: 24 * gib,
        free: 20 * gib,
    })));
    ui.handle(Message::Scanned(Ok((
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
    ))));
    if let Some(check) = ui
        .disk
        .rows
        .borrow()
        .first()
        .and_then(|row| row.activatable_widget())
        .and_then(|widget| widget.downcast::<CheckButton>().ok())
    {
        check.set_active(true);
    }
    ui.account.full_name.set_text("Ada Lovelace");
    ui.account.password.set_text("correct horse battery");
    ui.account.repeat.set_text("correct horse battery");
    ui.account.hostname.set_text("analytical-engine");
    // A GitHub lookup as it answers, without the network. Sample keys only.
    ui.github.username.set_text("adalovelace");
    ui.github_pending.set(Some(ui.github_epoch.get()));
    ui.handle(Message::Github(
        ui.github_epoch.get(),
        Ok(github::Profile {
            user: GithubUser::parse("adalovelace").unwrap(),
            keys: [
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3",
                "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFaKW8Z6YOoa5UaaVHwOgDXPe4PcYk663+2rvgwaCVo0QOYEMFJWQ4VZL6GKzJuSioxTotYTGYsKGOv45zDqz0A=",
            ]
            .iter()
            .map(|key| github::AuthorizedKey::parse(key).unwrap())
            .collect(),
            skipped: 0,
            name: Some("Ada Lovelace".into()),
            email: Some("1815+adalovelace@users.noreply.github.com".into()),
            private_email: true,
        }),
    ));
    ui.github.ssh_server.set_active(true);
    ui.desktop.cards[Desktop::ALL
        .iter()
        .position(|d| *d == Desktop::Tatami)
        .unwrap()]
    .set_active(true);
    if let Some(index) = ui.zones.as_ref().and_then(|z| z.find("America/Chicago")) {
        ui.show_zone(index, &detection_note(Source::Internet("geoip.kde.org")));
    }
    // Sample selections scheduled real warming; show a fixed indicator instead.
    ui.stop_warm();
    ui.activity_spinner.set_visible(true);
    ui.activity
        .set_text("Caching selected software in memory · 2.4 GiB of 6.1 GiB");
    ui.reached.set(Step::Location);
    let step = match page.as_str() {
        "disk" => Step::Disk,
        "account" => Step::Account,
        "github" => Step::GitHub,
        "desktop" => Step::Desktop,
        "applications" => Step::Applications,
        "location" => Step::Location,
        "review" | "ready" | "prepfail" => Step::Review,
        "install" | "done" | "failed" => Step::Install,
        _ => Step::Welcome,
    };
    if step < Step::Review {
        ui.go(step);
        return;
    }
    ui.activity_spinner.set_visible(false);
    ui.activity.set_text("");
    let review = Review {
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
        desktops: vec![Desktop::Plasma, Desktop::Tatami],
        default_desktop: Desktop::Plasma,
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
    };
    ui.show_review(&review);
    ui.go(Step::Review);
    event(
        ui,
        Event::Preparing {
            message: "Caching the installed system in memory".into(),
        },
    );
    event(
        ui,
        Event::Caching {
            read: 2600 * 1024 * 1024,
            total: 6300 * 1024 * 1024,
        },
    );
    if page == "prepfail" {
        event(
            ui,
            Event::Failed {
                message: "The selected configuration could not be evaluated; the target disk has not been erased: nix failed (exit status: 1): error: sample preview failure".into(),
            },
        );
        return;
    }
    if page != "review" {
        event(
            ui,
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
        );
        ui.review.erase.set_text("ERASE /dev/nvme0n1");
        ui.review.consent.set_active(true);
    }
    if step == Step::Review {
        return;
    }
    ui.installing.set(true);
    ui.started
        .set(Instant::now().checked_sub(Duration::from_secs(23)));
    ui.install.elapsed.set_text("0:23");
    ui.go(Step::Install);
    for (step, message) in [
        (
            1,
            "Erasing the selected disk and creating a GPT partition table",
        ),
        (2, "Formatting, verifying and mounting the new filesystems"),
        (3, "Writing the configuration, account and Wi-Fi settings"),
        (4, "Copying the prepared system to the new disk"),
    ] {
        event(
            ui,
            Event::Progress {
                step,
                message: message.into(),
            },
        );
    }
    for line in [
        "copying path '/nix/store/…-linux-7.2.8' from 'auto'",
        "copying path '/nix/store/…-plasma-workspace-6.6.2' from 'auto'",
        "copying path '/nix/store/…-firefox-153.0' from 'auto'",
    ] {
        event(ui, Event::Log { line: line.into() });
    }
    event(
        ui,
        Event::Copying {
            bytes: 3900 * 1024 * 1024,
            total: 6300 * 1024 * 1024,
        },
    );
    match page.as_str() {
        "done" => {
            event(
                ui,
                Event::Timing {
                    stage: "install".into(),
                    seconds: 47.3,
                },
            );
            event(ui, Event::Complete);
        }
        "failed" => event(
            ui,
            Event::Failed {
                message: "nix failed (exit status: 1): error: writing to file: No space left on device (sample preview failure)".into(),
            },
        ),
        _ => {}
    }
}
