// SPDX-License-Identifier: GPL-3.0-or-later
//! GEN-03: checking the selected disk again must not fail because an
//! *unrelated* disk vanished or was renumbered between `lsblk` and the
//! per-disk checks. The check after `wipefs`/`parted` and after formatting
//! (`Confirmed::recheck`) runs on a half-written disk, where failing abandons
//! the installation.
//!
//! No real disk is listed, opened or written. The library is built with
//! `CALAMARES_TOOL_PATH` naming a scratch directory of fakes (this test
//! writes them; set `GENKAN_FAKE_TOOLS` to the same directory), so `lsblk`
//! is a script printing a fixed inventory whatever it's asked. The test
//! re-runs itself in an unprivileged user+mount namespace (`unshare -Urm`)
//! where `/run` and `/sys/dev/block` are private tmpfs mounts: the "target"
//! is the node `/dev/loop0` (only `stat`ed; this user cannot open it) and its
//! sysfs entry is a fake directory, so discovery accepts it as a 40 GiB disk.
//!
//! ```sh
//! export CALAMARES_TOOL_PATH=$scratch/tools GENKAN_FAKE_TOOLS=$scratch/tools
//! cargo test --test revalidation -- --ignored
//! ```
// The test re-runs itself in a namespace; it is no part of the installer.
#![allow(clippy::disallowed_methods)]
use calamares_nixos::disk::{self, Identity, Reviewed};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

const TOOLS: Option<&str> = option_env!("CALAMARES_TOOL_PATH");
const INNER: &str = "GENKAN_FAKE_TOOLS_INNER";
const MARK: &str = ".genkan-fake-tools";

/// The fake tool directory, refusing any directory this test didn't make:
/// the helper's tools would otherwise be replaced.
fn tools() -> &'static str {
    let dir = TOOLS.expect("build with CALAMARES_TOOL_PATH=<an empty scratch directory>");
    assert_eq!(
        std::env::var("GENKAN_FAKE_TOOLS").as_deref(),
        Ok(dir),
        "set GENKAN_FAKE_TOOLS to the CALAMARES_TOOL_PATH the library was built with"
    );
    let path = Path::new(dir);
    if !path.join(MARK).exists() {
        assert!(
            !path.exists() || fs::read_dir(path).unwrap().next().is_none(),
            "refusing to write fake tools into {dir}, which holds something else"
        );
        fs::create_dir_all(path).unwrap();
        fs::write(path.join(MARK), "").unwrap();
    }
    dir
}

fn script(name: &str, body: &str) {
    let path = Path::new(tools()).join(name);
    let new = path.with_extension("new");
    fs::write(
        &new,
        format!("#!/bin/sh\necho \"{name} $*\" >> /run/fake-calls.log\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&new, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(new, path).unwrap();
}

fn write_tools() {
    // Builtins only: the helper gives its tools PATH=CALAMARES_TOOL_PATH.
    script(
        "lsblk",
        r#"while IFS= read -r l || [ -n "$l" ]; do printf '%s\n' "$l"; done < /run/fake-lsblk.json"#,
    );
}

fn mount(source: &str, target: &str, fstype: &str, flags: libc::c_ulong, data: &str) {
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (s, t, f, d) = (c(source), c(target), c(fstype), c(data));
    let rc = unsafe { libc::mount(s.as_ptr(), t.as_ptr(), f.as_ptr(), flags, d.as_ptr().cast()) };
    assert_eq!(rc, 0, "mount {target}: {}", std::io::Error::last_os_error());
}

fn disk_json(path: &str, majmin: &str) -> String {
    format!(
        r#"{{"path":"{path}","type":"disk","size":42949672960,"ro":false,"model":"Fake Disk","serial":"FAKE-{majmin}","wwn":null,"maj:min":"{majmin}","mountpoints":[null]}}"#
    )
}

fn inventory(disks: &[String]) {
    fs::write(
        "/run/fake-lsblk.json",
        format!(r#"{{"blockdevices":[{}]}}"#, disks.join(",")),
    )
    .unwrap();
}

/// A reviewed plan for `disk`: a review comes only from one.
fn plan_for(disk: Identity) -> calamares_nixos::InstallPlan {
    use calamares_nixos::*;
    fs::create_dir_all("/run/zoneinfo/Etc").unwrap();
    fs::write("/run/zoneinfo/Etc/UTC", "TZif").unwrap();
    let settings = Settings {
        template_dir: "/run/genkan-template".into(),
        zoneinfo: "/run/zoneinfo".into(),
        state_version: "26.05".into(),
        kernel: Kernel::Lts,
        test_diagnostics: false,
    };
    RawRequest {
        disk,
        firmware: Firmware::current(),
        filesystem: Filesystem::Ext4,
        encrypt: false,
        encryption_password: String::new(),
        hostname: "fake".into(),
        username: "fakeuser".into(),
        full_name: "Fake User".into(),
        password: "long-enough-test-password".into(),
        locale: "en_US.UTF-8".into(),
        timezone: "Etc/UTC".into(),
        keyboard: "us".into(),
        super_key: Default::default(),
        desktops: vec![Desktop::Plasma],
        default_desktop: Desktop::Plasma,
        applications: applications::default_selection(),
        copy_wifi: false,
        wifi_profiles: vec![],
        allow_unfree: false,
        swap: false,
        tuning: true,
        graphics: Default::default(),
        github_user: String::new(),
        ssh_keys: vec![],
        ssh_server: false,
        git_name: String::new(),
        git_email: String::new(),
        confirmation: String::new(),
    }
    .parse(&settings)
    .expect("plan")
}

/// Runs only inside the namespace the outer test makes.
#[test]
#[ignore = "run by unrelated_disks_do_not_abort_revalidation inside a user+mount namespace"]
fn inner() {
    if std::env::var_os(INNER).is_none() {
        return;
    }
    assert_eq!(unsafe { libc::geteuid() }, 0, "expected namespace root");
    mount("none", "/run", "tmpfs", 0, "mode=755");
    mount("none", "/sys/dev/block", "tmpfs", 0, "mode=755");
    // The target's sysfs entry: a non-virtual device with 512-byte sectors
    // and no holders.
    let fake = "/run/fakesys/devices/pci0000:00/0000:00:1f.2/fake";
    fs::create_dir_all(format!("{fake}/queue")).unwrap();
    fs::create_dir_all(format!("{fake}/holders")).unwrap();
    fs::write(format!("{fake}/queue/logical_block_size"), "512\n").unwrap();
    std::os::unix::fs::symlink(fake, "/sys/dev/block/7:0").unwrap();

    let target = Identity {
        path: "/dev/loop0".into(),
        major_minor: "7:0".into(),
        bytes: 40 * 1024u64.pow(3),
        serial: "FAKE-7:0".into(),
        wwn: String::new(),
        model: "Fake Disk".into(),
    };
    let plan = plan_for(target);
    let reviewed = Reviewed::of(&plan);

    // Control: the target alone confirms, before the first write.
    inventory(&[disk_json("/dev/loop0", "7:0")]);
    let mut confirmed = reviewed.revalidate().expect("harness broken");
    println!("control (target only): confirmed {}", confirmed.path());

    let mut failures = Vec::new();
    let unrelated = [
        // A: a USB disk lsblk listed was unplugged before it was stat()ed.
        ("unplugged /dev/sdz", disk_json("/dev/sdz", "65:144")),
        // B: an unrelated disk was renumbered: lsblk's numbers are stale.
        (
            "renumbered /dev/loop1, lsblk said 8:16",
            disk_json("/dev/loop1", "8:16"),
        ),
    ];
    for (case, other) in unrelated {
        inventory(&[disk_json("/dev/loop0", "7:0"), other]);
        // Before the first write, and after it, as the storage step does.
        let before = reviewed.revalidate().map(|_| ());
        println!("{case}: before the first write: {before:?}");
        if let Err(e) = before {
            failures.push(format!("{case}, before the first write: {e:#}"));
        }
        match confirmed.recheck(&[]) {
            Ok(again) => {
                println!("{case}: after the first write: Ok({})", again.path());
                confirmed = again;
            }
            Err(e) => {
                println!("{case}: after the first write: Err({e:#})");
                failures.push(format!("{case}, after the first write: {e:#}"));
                inventory(&[disk_json("/dev/loop0", "7:0")]);
                confirmed = reviewed.revalidate().unwrap();
            }
        }
    }
    // The listing for choosing a disk lists the unplugged one as unavailable
    // and still lists the target.
    inventory(&[
        disk_json("/dev/loop0", "7:0"),
        disk_json("/dev/sdz", "65:144"),
    ]);
    let listed = disk::discover().expect("the listing survives an unplugged disk");
    println!(
        "listing: {:?}",
        listed
            .iter()
            .map(|d| (&d.identity.path, &d.blocked))
            .collect::<Vec<_>>()
    );
    assert!(listed[0].blocked.is_none() && listed[1].blocked.is_some());
    // The target itself changing is still refused.
    inventory(&[disk_json("/dev/loop0", "7:0").replace("FAKE-7:0", "OTHER")]);
    let changed = confirmed.recheck(&[]);
    println!(
        "target's serial changed: {:?}",
        changed.as_ref().map(|c| c.path())
    );
    assert!(changed.is_err(), "a changed target was accepted");

    let calls = fs::read_to_string("/run/fake-calls.log").unwrap_or_default();
    println!("tools run:\n{calls}");
    assert!(
        failures.is_empty(),
        "checking the unchanged selected disk failed because of another disk: {failures:#?}"
    );
}

#[test]
#[ignore = "needs the library built with CALAMARES_TOOL_PATH naming a scratch directory of fakes; see the module docs"]
fn unrelated_disks_do_not_abort_revalidation() {
    write_tools();
    let out = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
            "--",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inner",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(INNER, "1")
        .output()
        .expect("unshare");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stdout}\n{stderr}");
    assert!(stdout.contains("running 1 test"), "inner test did not run");
    assert!(out.status.success(), "inner test failed (see above)");
}
