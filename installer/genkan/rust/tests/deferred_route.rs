// SPDX-License-Identifier: GPL-3.0-or-later
//! GEN-07: the deferred route (downloads larger than live memory) must check
//! that the root filesystem has room, as the in-memory route does after
//! building. Without the check, `install::prepare` accepted a selection whose
//! downloads alone exceed the target's root filesystem, and execution then
//! erased the disk and failed with "No space left" while building on it.
//!
//! Drives the real `install::prepare` with fakes only. The library is built
//! with `CALAMARES_TOOL_PATH` naming a scratch directory of fake tools
//! (written here; set `GENKAN_FAKE_TOOLS` to the same directory): `findmnt`
//! says root is tmpfs and /iso is iso9660, `lsblk` lists one disk, the
//! formatter and modprobe checks succeed, `nixos-generate-config` prints an
//! empty module and `nix build --dry-run` reports a 300 GiB unpacked
//! download. There is no wipefs, parted, mkfs or nix-copy stand-in at all,
//! so nothing destructive could run even if reached.
//!
//! `prepare` insists on euid 0, a root-owned settings path, `/run` and a
//! live-looking root, so the test re-runs itself in an unprivileged
//! user+mount namespace (`unshare -Urm`) and chroots into a tmpfs root with
//! `/proc`, `/sys`, `/dev`, `/nix`, `/bin`, `/usr` and the tools
//! bind-mounted, a private `/run`, a two-line `/etc`, and a fake
//! `/sys/dev/block/7:0` for the listed disk `/dev/loop0` (only `stat`ed; this
//! user cannot open it).
//!
//! ```sh
//! export CALAMARES_TOOL_PATH=$scratch/tools GENKAN_FAKE_TOOLS=$scratch/tools
//! cargo test --test deferred_route -- --ignored
//! ```
// The test re-runs itself in a namespace; it is no part of the installer.
#![allow(clippy::disallowed_methods)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

const TOOLS: Option<&str> = option_env!("CALAMARES_TOOL_PATH");
const INNER: &str = "GENKAN_FAKE_TOOLS_INNER";
const MARK: &str = ".genkan-fake-tools";
const GIB: u64 = 1024 * 1024 * 1024;

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

const DRV: &str = "/nix/store/00000000000000000000000000000000-nixos-system-fake.drv";
const OUT: &str = "/nix/store/11111111111111111111111111111111-nixos-system-fake";

fn write_tools() {
    // Builtins only: the helper gives its tools PATH=CALAMARES_TOOL_PATH.
    script(
        "lsblk",
        r#"while IFS= read -r l || [ -n "$l" ]; do printf '%s\n' "$l"; done < /run/fake-lsblk.json"#,
    );
    script(
        "findmnt",
        r#"for a in "$@"; do last=$a; done
case "$last" in
  /) echo tmpfs ;;
  /iso) echo iso9660 ;;
  *) exit 1 ;;
esac"#,
    );
    for ok in [
        "mkfs.ext4",
        "mkfs.btrfs",
        "mkfs.xfs",
        "mkfs.fat",
        "modprobe",
        "mkswap",
    ] {
        script(ok, "exit 0");
    }
    script("nixos-generate-config", "echo '{ ... }: { }'");
    // Paths of the build the live store already has: `nix path-info` says
    // they hold 20 GiB.
    script(
        "nix-store",
        "echo /nix/store/22222222222222222222222222222222-live-a\necho /nix/store/33333333333333333333333333333333-live-b",
    );
    // The live store already has paths of the build: 20 GiB of them.
    script(
        "nix-store",
        "printf '%s\\n' /nix/store/22222222222222222222222222222222-live-a /nix/store/33333333333333333333333333333333-live-b",
    );
    script(
        "nix",
        &format!(
            r#"case "$1 $2" in
  "build --dry-run")
    printf '%s\n' '[{{"drvPath":"{DRV}","outputs":{{"out":"{OUT}"}}}}]'
    echo 'these 4 derivations will be built:' >&2
    echo 'these 1500 paths will be fetched (60.00 GiB download, 300.00 GiB unpacked):' >&2
    ;;
  "path-info --json") printf '%s\n' '[{{"narSize":21474836480}}]' ;;
  *) echo "fake nix: unexpected $*" >&2; exit 1 ;;
esac"#
        ),
    );
}

fn c(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap()
}

fn mount(source: &str, target: &str, fstype: Option<&str>, flags: libc::c_ulong, data: &str) {
    let (s, t, d) = (c(source), c(target), c(data));
    let f = fstype.map(c);
    let rc = unsafe {
        libc::mount(
            s.as_ptr(),
            t.as_ptr(),
            f.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
            flags,
            d.as_ptr().cast(),
        )
    };
    assert_eq!(rc, 0, "mount {target}: {}", std::io::Error::last_os_error());
}

/// Where the namespace's root is mounted, which the outer test names;
/// empty directories once the namespace is gone.
const ROOT: &str = "GENKAN_FAKE_ROOT";

fn root_dir() -> std::path::PathBuf {
    std::env::var_os(ROOT).map_or_else(
        || std::env::temp_dir().join(format!("genkan-fake-root-{}", std::process::id())),
        Into::into,
    )
}

/// A root-owned (namespace root) tmpfs root, so `read_trusted` sees every
/// ancestor of the settings paths owned by uid 0.
fn enter_fake_root() {
    let root = root_dir().to_str().unwrap().to_owned();
    fs::create_dir_all(&root).unwrap();
    mount("none", &root, Some("tmpfs"), 0, "mode=755");
    // Only the fake tool directory from the scratch space: the tools'
    // interpreter is /bin/sh, which lives in /nix/store.
    for dir in ["/proc", "/sys", "/dev", "/nix", "/bin", "/usr", tools()] {
        let target = format!("{root}{dir}");
        fs::create_dir_all(&target).unwrap();
        mount(dir, &target, None, libc::MS_BIND | libc::MS_REC, "");
    }
    fs::create_dir(format!("{root}/run")).unwrap();
    mount("none", &format!("{root}/run"), Some("tmpfs"), 0, "mode=755");
    mount(
        "none",
        &format!("{root}/sys/dev/block"),
        Some("tmpfs"),
        0,
        "mode=755",
    );
    fs::create_dir(format!("{root}/etc")).unwrap();
    fs::write(format!("{root}/etc/os-release"), "NAME=NixOS\nID=nixos\n").unwrap();
    fs::write(format!("{root}/etc/passwd"), "root:x:0:0::/root:/bin/sh\n").unwrap();
    let r = c(&root);
    assert_eq!(
        unsafe { libc::chroot(r.as_ptr()) },
        0,
        "chroot: {}",
        std::io::Error::last_os_error()
    );
    std::env::set_current_dir("/").unwrap();
}

const LOCK: &str = r#"{"version":7,"root":"root","nodes":{
  "root":{"inputs":{"sekka":"sekka","nixpkgs":["sekka","nixpkgs"],"determinate":["sekka","determinate"],
    "fh":["sekka","fh"],"applications":["sekka","applications"],"ai-apps":["sekka","ai-apps"],"omp":["sekka","omp"]}},
  "sekka":{"locked":{"narHash":"sha256-s"},"inputs":{"nixpkgs":"n","determinate":"d","fh":"f",
    "applications":"a","ai-apps":"i","omp":"o"}},
  "n":{"locked":{"narHash":"sha256-n"}},"d":{"locked":{"narHash":"sha256-d"}},
  "f":{"locked":{"narHash":"sha256-f"}},"a":{"locked":{"narHash":"sha256-a"}},
  "i":{"locked":{"narHash":"sha256-i"}},"o":{"locked":{"narHash":"sha256-o"}}}}"#;

/// Prepares an installation to a `disk_gib` disk, as the helper does.
fn prepare_on(disk_gib: u64) -> anyhow::Result<calamares_nixos::install::Summary> {
    use calamares_nixos::*;
    let disk_bytes = disk_gib * GIB;
    fs::write(
        "/run/fake-lsblk.json",
        format!(
            r#"{{"blockdevices":[{{"path":"/dev/loop0","type":"disk","size":{disk_bytes},"ro":false,"model":"Fake Disk","serial":"FAKE","wwn":null,"maj:min":"7:0","mountpoints":[null]}}]}}"#
        ),
    )
    .unwrap();
    let settings = Settings {
        template_dir: "/run/genkan-template".into(),
        zoneinfo: "/run/zoneinfo".into(),
        state_version: "26.05".into(),
        kernel: Kernel::Lts,
        test_diagnostics: false,
    };
    let request = RawRequest {
        disk: disk::Identity {
            path: "/dev/loop0".into(),
            major_minor: "7:0".into(),
            bytes: disk_bytes,
            serial: "FAKE".into(),
            wwn: String::new(),
            model: "Fake Disk".into(),
        },
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
    };
    let plan = request.parse(&settings).expect("plan");
    install::prepare(&plan).map(|prepared| prepared.summary().clone())
}

/// Runs only inside the namespace the outer test makes.
#[test]
#[ignore = "run by deferred_route_still_checks_root_space inside a user+mount namespace"]
fn inner() {
    if std::env::var_os(INNER).is_none() {
        return;
    }
    assert_eq!(unsafe { libc::geteuid() }, 0, "expected namespace root");
    unsafe { libc::umask(0o022) };
    enter_fake_root();

    // The listed disk's sysfs entry: non-virtual, 512-byte sectors, no holders.
    let fake = "/run/fakesys/devices/pci0000:00/0000:00:1f.2/fake";
    fs::create_dir_all(format!("{fake}/queue")).unwrap();
    fs::create_dir_all(format!("{fake}/holders")).unwrap();
    fs::write(format!("{fake}/queue/logical_block_size"), "512\n").unwrap();
    std::os::unix::fs::symlink(fake, "/sys/dev/block/7:0").unwrap();
    // Root-owned settings: the flake template and tzdata.
    fs::create_dir_all("/run/genkan-template").unwrap();
    fs::write(
        "/run/genkan-template/flake.nix.in",
        "{ hostname = @HOSTNAME@; }\n",
    )
    .unwrap();
    fs::write("/run/genkan-template/flake.lock", LOCK).unwrap();
    fs::create_dir_all("/run/zoneinfo/Etc").unwrap();
    fs::write("/run/zoneinfo/Etc/UTC", "TZif").unwrap();

    let ram = calamares_nixos::memory::read().unwrap();
    println!(
        "live RAM {:.1} GiB total, {:.1} GiB available; downloads 300 GiB unpacked",
        ram.total as f64 / GIB as f64,
        ram.available as f64 / GIB as f64,
    );
    let small = prepare_on(40).map_err(|e| format!("{e:#}"));
    println!("40 GiB disk: {small:?}");
    // Room for the downloads and the reserve (308 GiB), but not for the
    // 20 GiB of the build the live store has, which the build copies too.
    let borderline = prepare_on(320).map_err(|e| format!("{e:#}"));
    println!("320 GiB disk: {borderline:?}");
    // Control: a disk with room for the downloads is prepared on the
    // deferred route.
    let large = prepare_on(400).map_err(|e| format!("{e:#}"));
    println!("400 GiB disk: {large:?}");
    let calls = fs::read_to_string("/run/fake-calls.log").unwrap_or_default();
    println!("tools run:\n{calls}");
    assert!(
        large.as_ref().is_ok_and(|s| s.deferred),
        "harness: expected the 400 GiB disk to be prepared on the deferred route, got {large:?}"
    );
    assert!(
        large.as_ref().is_ok_and(|s| s.closure_bytes > 320 * GIB),
        "the estimate counts the downloads and the live store's paths: {large:?}"
    );
    assert!(
        small
            .as_ref()
            .is_err_and(|e| e.contains("on the root filesystem")),
        "preparation accepted a selection with 300 GiB of unpacked downloads for a 39 GiB root filesystem (deferred route); expected a refusal before erasure, got {small:?}"
    );
    assert!(
        borderline
            .as_ref()
            .is_err_and(|e| e.contains("on the root filesystem")),
        "preparation ignored the live store's paths that building on the target copies: {borderline:?}"
    );
}

#[test]
#[ignore = "needs the library built with CALAMARES_TOOL_PATH naming a scratch directory of fakes; see the module docs"]
fn deferred_route_still_checks_root_space() {
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
        .env(ROOT, root_dir())
        .output()
        .expect("unshare");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stdout}\n{stderr}");
    // The namespace's mounts are gone with it; remove the empty mount points.
    let _ = remove_empty_tree(&root_dir());
    assert!(stdout.contains("running 1 test"), "inner test did not run");
    assert!(out.status.success(), "inner test failed (see above)");
}

/// Removes directories only, never files: the tree holds empty mount points.
fn remove_empty_tree(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)?.flatten() {
        if entry.file_type()?.is_dir() {
            remove_empty_tree(&entry.path())?;
        }
    }
    fs::remove_dir(dir)
}
