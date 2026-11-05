// SPDX-License-Identifier: GPL-3.0-or-later
//! `sekka-network-store open|create|stop|skeleton|export`: NetworkManager's
//! saved connections, secrets and all, kept in a small LUKS2 image whose only
//! key is sealed to the TPM, bound over NetworkManager's keyfile directory
//! before it starts. NetworkManager itself is unchanged: secrets stay owned
//! by the system, so saved networks connect at boot, before anyone logs in,
//! in every desktop. DEVELOPERS.md ("Saved network connections") has the
//! design and its limits.
//!
//! - `open` (before NetworkManager) opens the store, adopts any profiles
//!   left in the plaintext directory, merges back stores set aside earlier
//!   that open again, and binds it. If it can't, NetworkManager reads the
//!   plaintext directory as before, with the secret-less copies put back.
//! - `create` (beside NetworkManager) makes a store once every system
//!   generation can open one, and sets aside one locked to a TPM that was
//!   cleared or replaced. A new store is opened at the next boot.
//! - `stop` closes it, but never under a running NetworkManager.
//! - `skeleton` refreshes the secret-less copies.
//! - `export` (when turned off) moves the profiles back out and removes it.

mod cmd;
mod files;
mod merge;
mod rotation;
mod skeleton;
mod store;
mod sys;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use merge::Source;
use rotation::{Decision, Srk};
use store::{Absent, Bound, Context, Failure, Sealed};

/// Stops the program at `name` when a test asks (SEKKA_NETWORK_STORE_CRASH_AT),
/// as a power loss there would, to check the next run recovers.
pub fn crash_point(name: &str) {
    if std::env::var("SEKKA_NETWORK_STORE_CRASH_AT").is_ok_and(|at| at == name) {
        eprintln!("sekka-network-store: stopping at {name}, as the test asked");
        std::process::abort();
    }
}

fn log(message: &str) {
    eprintln!("sekka-network-store: {message}");
}

/// What the store is doing, for the journal and for desktop UIs.
#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// Open: NetworkManager reads and writes the sealed store.
    Sealed,
    /// Made this boot; opened from the next.
    Created,
    /// A store made to replace one locked to a cleared or replaced TPM.
    Rotated,
    /// Made only once no bootable generation predates the store.
    WaitingForGenerations,
    /// The filesystem is encrypted already.
    AlreadyEncrypted,
    NoTpm,
    /// NetworkManager was running, so nothing it reads was touched.
    NetworkManagerRunning,
    /// The TPM didn't unlock the store: saved networks ask for passwords.
    Unsealable,
    Damaged,
    Exported,
    Failed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Sealed => "sealed",
            State::Created => "created",
            State::Rotated => "rotated",
            State::WaitingForGenerations => "waiting-for-generations",
            State::AlreadyEncrypted => "already-encrypted",
            State::NoTpm => "no-tpm",
            State::NetworkManagerRunning => "network-manager-running",
            State::Unsealable => "unsealable",
            State::Damaged => "damaged",
            State::Exported => "exported",
            State::Failed => "failed",
        }
    }
}

fn json(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn report(cx: &Context, state: State, detail: &str) {
    log(&format!("{}: {detail}", state.name()));
    let text = format!(
        "{{\"state\":{},\"detail\":{}}}\n",
        json(state.name()),
        json(detail)
    );
    if let Err(e) = files::ensure_dir(&cx.run, 0o755)
        .and_then(|()| files::write_atomic(&cx.status(), text.as_bytes(), 0o644))
    {
        log(&format!("{}: {e}", cx.status().display()));
    }
}

/// NetworkManager reads the plaintext directory, as before the store: put
/// back the secret-less copies so saved networks stay listed.
fn fall_back(cx: &Context, state: State, why: &str) {
    match skeleton::restore(&cx.skeleton(), &cx.keyfiles) {
        Ok(0) => {}
        Ok(n) => log(&format!(
            "put back {n} saved networks without their passwords"
        )),
        Err(e) => log(&format!("putting back saved networks: {e}")),
    }
    report(cx, state, why);
}

/// Merge stores set aside earlier that open again (the TPM they were sealed
/// to is back) into `live`, within `budget`.
fn merge_rotated(cx: &Context, live: &Path, started: Instant, budget: Duration) {
    for old in Sealed::rotated(cx) {
        if started.elapsed() > budget {
            log("leaving the stores set aside for the next boot: out of time");
            return;
        }
        let image = old.image().to_path_buf();
        let mounted = match old.attach().and_then(|c| c.mount(true)) {
            Ok(m) => m,
            Err(e) => {
                log(&format!("{} stays set aside: {e}", image.display()));
                continue;
            }
        };
        let merged = merge::merge(Source::Rotated, &mounted.connections(), live, false);
        let closed = mounted.close();
        match (merged, closed) {
            (Ok(r), Ok(())) => {
                log(&format!(
                    "merged {} saved networks back from {}",
                    r.copied,
                    image.display()
                ));
                if let Some(old) = Sealed::rotated(cx).into_iter().find(|s| s.image() == image) {
                    old.forget();
                }
            }
            (Err(e), _) => log(&format!("merging {}: {e}", image.display())),
            (_, Err(e)) => log(&format!("closing {}: {e}", image.display())),
        }
    }
}

/// Store the TPM's SRK again (systemd-tpm2-setup); whether one is stored now.
fn recreate_srk(cx: &Context) -> bool {
    let setup = cmd::tool(&cx.tpm2_setup.to_string_lossy())
        .limit(Duration::from_secs(20))
        .run();
    if !setup.success() {
        log(&format!("storing the TPM's SRK again: {}", setup.stderr));
    }
    matches!(store::read_srk(), Srk::Known(_))
}

fn open(cx: &Context) {
    let started = Instant::now();
    if Bound::current(cx).is_some() {
        return;
    }
    if sys::network_manager_running() {
        return report(
            cx,
            State::NetworkManagerRunning,
            "the store opens at the next boot",
        );
    }
    let sealed = match Sealed::find(cx) {
        Ok(Some(sealed)) => sealed,
        Ok(None) => return,
        Err(e) => return fall_back(cx, State::Failed, &e.to_string()),
    };
    if !sys::tpm_present() {
        return fall_back(
            cx,
            State::Unsealable,
            "there's no TPM to unlock it (turned off in the firmware?)",
        );
    }
    let attached = match sealed.attach() {
        // systemd unseals against the TPM's stored SRK and doesn't make it
        // again. One evicted is made again from the TPM's seed (the same
        // key), so store it and try once more.
        Err(Failure::Unseal(e)) if store::read_srk() == Srk::Missing && recreate_srk(cx) => {
            log(&format!("stored the TPM's SRK again, after: {e}"));
            match Sealed::find(cx) {
                Ok(Some(sealed)) => sealed.attach(),
                _ => Err(Failure::Unseal(e)),
            }
        }
        attached => attached,
    };
    let checked = match attached {
        Ok(checked) => checked,
        Err(e @ Failure::Unseal(_)) => {
            if let Err(error) =
                files::ensure_dir(&cx.run, 0o755).and_then(|()| fs::write(cx.unseal_failed(), ""))
            {
                log(&format!("{}: {error}", cx.unseal_failed().display()));
            }
            return fall_back(cx, State::Unsealable, &e.to_string());
        }
        Err(e @ Failure::Damaged(_)) => return fall_back(cx, State::Damaged, &e.to_string()),
        Err(e) => return fall_back(cx, State::Failed, &e.to_string()),
    };
    let mounted = match checked.mount(false) {
        Ok(mounted) => mounted,
        Err(e) => return fall_back(cx, State::Failed, &e.to_string()),
    };
    crash_point("mounted");
    // Profiles NetworkManager saved where it reads when the store wasn't
    // open: Genkan's at installation, ones from before the store, and ones
    // saved at a boot it couldn't open.
    let adopted = match merge::merge(
        Source::Plaintext,
        &cx.keyfiles,
        &mounted.connections(),
        true,
    ) {
        Ok(r) => r,
        Err(e) => {
            log(&format!("adopting saved networks: {e}"));
            Default::default()
        }
    };
    if adopted.copied > 0 || adopted.conflicts > 0 {
        log(&format!(
            "adopted {} saved networks ({} kept aside in conflicts)",
            adopted.copied, adopted.conflicts
        ));
    }
    merge_rotated(cx, &mounted.connections(), started, Duration::from_secs(12));
    if let Err(e) = mounted.bind() {
        return fall_back(cx, State::Failed, &e);
    }
    if let Err(e) = skeleton::refresh(&cx.keyfiles, &cx.skeleton()) {
        log(&format!("refreshing the secret-less copies: {e}"));
    }
    sys::write_count(&cx.unseal_failures(), 0);
    let _ = fs::remove_file(cx.unseal_failed());
    if adopted.copied > 0 {
        // Trim the plaintext's freed blocks now rather than at the weekly run.
        let _ = cmd::tool("systemctl")
            .args(["start", "--no-block", "fstrim.service"])
            .run();
    }
    report(
        cx,
        State::Sealed,
        "saved networks are sealed to this computer's TPM",
    );
}

fn create(cx: &Context) {
    let mut state = State::Created;
    if let Err(e) = files::ensure_dir(&cx.state, 0o700) {
        return report(cx, State::Failed, &e.to_string());
    }
    if cx.image().exists() {
        if !cx.unseal_failed().exists() {
            return;
        }
        let sealed = match Sealed::find(cx) {
            Ok(Some(sealed)) => sealed,
            _ => return,
        };
        let recorded = fs::read(cx.srk()).ok();
        let failures = sys::read_count(&cx.unseal_failures());
        let mut decision = rotation::decide(&store::read_srk(), recorded.as_deref(), failures);
        if decision == Decision::RecreateSrk {
            recreate_srk(cx);
            decision = match rotation::decide(&store::read_srk(), recorded.as_deref(), failures) {
                Decision::RecreateSrk => Decision::Keep,
                d => d,
            };
        }
        match decision {
            Decision::Keep | Decision::RecreateSrk => {
                return log(
                    "the TPM didn't answer: keeping the store, to try again at the next boot",
                );
            }
            Decision::Count => {
                sys::write_count(&cx.unseal_failures(), failures + 1);
                return log(&format!(
                    "the store didn't unlock with the same TPM ({} boots so far): keeping it",
                    failures + 1
                ));
            }
            Decision::Rotate => {
                if let Err(e) = sealed.set_aside() {
                    return report(cx, State::Failed, &format!("setting the store aside: {e}"));
                }
                sys::write_count(&cx.unseal_failures(), 0);
                log(
                    "set aside a store locked to a TPM that was cleared or replaced; it's retried at every boot",
                );
                state = State::Rotated;
            }
        }
    }
    let unmarked = sys::unmarked(&sys::generations(&cx.profiles));
    if !unmarked.is_empty() {
        let list: Vec<String> = unmarked.iter().map(|p| p.display().to_string()).collect();
        return report(
            cx,
            State::WaitingForGenerations,
            &format!(
                "made once these generations, which couldn't open it, are gone: {}",
                list.join(", ")
            ),
        );
    }
    if sys::encrypted(&cx.keyfiles) {
        return report(
            cx,
            State::AlreadyEncrypted,
            "the filesystem is encrypted already",
        );
    }
    if !sys::tpm_present() {
        return report(cx, State::NoTpm, "there's no TPM");
    }
    let failures = sys::read_count(&cx.create_failures());
    if failures >= 3 && !failures.is_multiple_of(8) {
        sys::write_count(&cx.create_failures(), failures + 1);
        return log("not trying to make the store this boot, after failing before");
    }
    let Some(absent) = Absent::find(cx) else {
        return;
    };
    let made = absent.build().and_then(|built| {
        let srk = match store::read_srk() {
            Srk::Known(srk) => Some(srk),
            _ => None,
        };
        built.commit(srk.as_deref())
    });
    match made {
        Ok(_) => {
            sys::write_count(&cx.create_failures(), 0);
            let detail = match state {
                State::Rotated => {
                    "made a new store for a TPM that was cleared or replaced; saved networks ask for their passwords once"
                }
                _ => "made the store; saved networks are sealed from the next boot",
            };
            report(cx, state, detail);
        }
        Err(e) => {
            sys::write_count(&cx.create_failures(), failures + 1);
            report(cx, State::Failed, &format!("making the store: {e}"));
        }
    }
}

fn stop(cx: &Context) {
    if sys::network_manager_running() {
        return log("NetworkManager is running: the store stays open");
    }
    match Bound::current(cx) {
        Some(bound) => {
            if let Err(e) = bound.close() {
                log(&format!("closing the store: {e}"));
            }
        }
        // Shutdown may have unmounted it already.
        None => store::detach(store::MAPPER),
    }
}

fn refresh_skeleton(cx: &Context) {
    if Bound::current(cx).is_some()
        && let Err(e) = skeleton::refresh(&cx.keyfiles, &cx.skeleton())
    {
        log(&format!("refreshing the secret-less copies: {e}"));
    }
}

fn export(cx: &Context) {
    if sys::network_manager_running() {
        return report(
            cx,
            State::NetworkManagerRunning,
            "the store is exported at the next boot",
        );
    }
    let sealed = match Sealed::find(cx) {
        Ok(Some(sealed)) => sealed,
        Ok(None) => return,
        Err(e) => return report(cx, State::Failed, &e.to_string()),
    };
    let mounted = match sealed.attach().and_then(|c| c.mount(true)) {
        Ok(mounted) => mounted,
        Err(e) => return fall_back(cx, State::Failed, &format!("exporting: {e}")),
    };
    let exported = merge::merge(Source::Store, &mounted.connections(), &cx.keyfiles, false);
    let closed = mounted.close();
    match (exported, closed) {
        (Ok(r), Ok(())) => {
            for path in [cx.image(), cx.srk()] {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir_all(cx.skeleton());
            let _ = files::sync_dir(&cx.state);
            report(
                cx,
                State::Exported,
                &format!("moved {} saved networks back out of the store", r.copied),
            );
        }
        (Err(e), _) => report(cx, State::Failed, &format!("exporting: {e}")),
        (_, Err(e)) => report(cx, State::Failed, &format!("closing the store: {e}")),
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: sekka-network-store open|create|stop|skeleton|export \
         [--state DIR] [--keyfiles DIR] [--run DIR] [--profiles DIR] [--tpm2-setup PATH]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    let mut cx = Context {
        state: PathBuf::from("/var/lib/sekka/network-store"),
        keyfiles: PathBuf::from("/etc/NetworkManager/system-connections"),
        run: PathBuf::from("/run/sekka"),
        profiles: PathBuf::from("/nix/var/nix/profiles"),
        tpm2_setup: PathBuf::from("systemd-tpm2-setup"),
    };
    while let Some(option) = args.next() {
        let Some(value) = args.next() else {
            return usage();
        };
        match option.as_str() {
            "--state" => cx.state = value.into(),
            "--keyfiles" => cx.keyfiles = value.into(),
            "--run" => cx.run = value.into(),
            "--profiles" => cx.profiles = value.into(),
            "--tpm2-setup" => cx.tpm2_setup = value.into(),
            _ => return usage(),
        }
    }
    if let Err(e) = files::ensure_dir(&cx.state, 0o700) {
        log(&format!("{}: {e}", cx.state.display()));
        return ExitCode::FAILURE;
    }
    // One at a time: `open` before NetworkManager, `create` beside it, and
    // the secret-less copies on every change.
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cx.lock())
        .and_then(|f| {
            f.lock()?;
            Ok(f)
        });
    let _lock = match lock {
        Ok(lock) => lock,
        Err(e) => {
            log(&format!("{}: {e}", cx.lock().display()));
            return ExitCode::FAILURE;
        }
    };
    match command.as_str() {
        "open" => open(&cx),
        "create" => create(&cx),
        "stop" => stop(&cx),
        "skeleton" => refresh_skeleton(&cx),
        "export" => export(&cx),
        _ => return usage(),
    }
    ExitCode::SUCCESS
}
