// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

mod discovery;
mod family;
mod firewall;
mod nm;

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::fs::{self, DirBuilder, File, Permissions};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::process::ExitCode;

use family::Family;
use ipnet::IpNet;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub blocked: Option<Family>,
    pub carriers: BTreeSet<String>,
    pub tunnels: BTreeSet<String>,
    pub marks: BTreeSet<u32>,
    pub endpoints: BTreeSet<SocketAddr>,
    pub trusted_v4: BTreeSet<String>,
    pub trusted_v6: BTreeSet<String>,
}

const CONFIG: &str = "/etc/sekka/vpn-guard.json";
const RUN: &str = "/run/sekka-vpn-guard";
const LOCK: &str = "/run/sekka-vpn-guard/lock";
const ARMED: &str = "/run/sekka-vpn-guard/armed";
const STATUS: &str = "/run/sekka-vpn-guard/status";
const NEXT_STATUS: &str = "/run/sekka-vpn-guard/status.new";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    Start,
    Stop,
    Reconcile,
    Clear,
    Status,
    ValidateConfig,
    Ignore,
}

fn request(args: &[String]) -> Result<Request> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(match args.as_slice() {
        ["status"] => Request::Status,
        ["reconcile"] => Request::Reconcile,
        ["clear"] => Request::Clear,
        ["start"] => Request::Start,
        ["stop"] => Request::Stop,
        ["validate-config"] => Request::ValidateConfig,
        [
            _,
            "pre-up"
            | "vpn-pre-up"
            | "up"
            | "down"
            | "vpn-up"
            | "vpn-down"
            | "reapply"
            | "dhcp4-change"
            | "dhcp6-change"
            | "connectivity-change",
        ] => Request::Reconcile,
        [_, _] => Request::Ignore,
        _ => return Err("usage: sekka-vpn-guard {status|reconcile|clear}".into()),
    })
}

// Only root changes policy. Readers see the most recent verified status via
// atomic rename and have no access to the lock used by the dispatcher.
fn lock() -> io::Result<File> {
    match DirBuilder::new().mode(0o755).create(RUN) {
        Ok(()) => fs::set_permissions(RUN, Permissions::from_mode(0o755))?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(LOCK)?;
    file.lock()?;
    Ok(file)
}

fn save_status(message: &str) -> io::Result<()> {
    let mut file = File::create(NEXT_STATUS)?;
    file.write_all(message.as_bytes())?;
    file.set_permissions(Permissions::from_mode(0o644))?;
    fs::rename(NEXT_STATUS, STATUS)
}

fn verified_status(policy: &Policy) -> String {
    let mut text = String::new();
    for family in Family::BOTH {
        let state = if policy.blocked == Some(family) {
            "blocked"
        } else {
            "open"
        };
        text.push_str(&format!("{}: {state}\n", family.name()));
    }
    if !policy.carriers.is_empty() {
        let names: Vec<_> = policy.carriers.iter().map(String::as_str).collect();
        text.push_str(&format!("Full tunnel: {}\n", names.join(", ")));
    }
    text
}

fn add_trusted_prefixes(policy: &mut Policy, json: &str) -> Result<()> {
    for prefix in serde_json::from_str::<Vec<String>>(json)? {
        let net: IpNet = prefix
            .parse()
            .map_err(|_| format!("invalid trusted LAN prefix: {prefix}"))?;
        if net.network() != net.addr() {
            return Err(format!("trusted LAN prefix has host bits set: {prefix}").into());
        }
        let prefixes = if net.addr().is_ipv4() {
            &mut policy.trusted_v4
        } else {
            &mut policy.trusted_v6
        };
        prefixes.insert(net.to_string());
    }
    Ok(())
}

fn reconcile() -> Result<()> {
    let mut policy = match discovery::discover() {
        Ok(policy) => policy,
        Err(discovery_error) => {
            // An interrupted read must never lift an existing block. On a first
            // activation, even without a complete kernel snapshot, NM may know
            // a single-family full tunnel is being activated. Protect that family
            // conservatively until the retry can restore its transport exceptions.
            if firewall::blocked_family()?.is_none()
                && let Some(family) = nm::active_intent().map_err(|e| {
                    format!("{discovery_error}; could not read NetworkManager profile intent: {e}")
                })?
            {
                firewall::apply(&Policy {
                    blocked: Some(family),
                    ..Default::default()
                })?;
            }
            return Err(discovery_error.into());
        }
    };
    add_trusted_prefixes(&mut policy, &fs::read_to_string(CONFIG)?)?;
    firewall::apply(&policy)?;
    save_status(&verified_status(&policy))?;
    Ok(())
}

fn run(action: Request) -> Result<()> {
    match action {
        Request::Ignore => return Ok(()),
        Request::ValidateConfig => {
            return add_trusted_prefixes(&mut Policy::default(), &io::read_to_string(io::stdin())?);
        }
        Request::Status => {
            let text = fs::read_to_string(STATUS)
                .map_err(|e| format!("no verified VPN guard status is available: {e}"))?;
            io::stdout().lock().write_all(text.as_bytes())?;
            if text.starts_with("Degraded:") {
                return Err("VPN guard needs reconciliation; see the service journal".into());
            }
            return Ok(());
        }
        _ => {}
    }

    let _lock = lock()?;
    match action {
        Request::Stop | Request::Clear => {
            if action == Request::Stop && fs::exists(ARMED)? {
                fs::remove_file(ARMED)?;
            }
            firewall::apply(&Policy::default())?;
            save_status(&verified_status(&Policy::default()))?;
            return Ok(());
        }
        Request::Start => {
            File::create(ARMED)?;
        }
        Request::Reconcile if !fs::exists(ARMED)? || !fs::exists(CONFIG)? => return Ok(()),
        _ => {}
    }
    if let Err(error) = reconcile() {
        let observed = match firewall::blocked_family() {
            Ok(Some(family)) => format!("{} egress block installed", family.name()),
            Ok(None) => "no egress block installed".to_owned(),
            Err(_) => "installed policy unknown".to_owned(),
        };
        save_status(&format!(
            "Degraded: {observed}. Reconciliation will retry.\n"
        ))?;
        if action == Request::Start {
            // Keep lifecycle ownership even when the first observation fails.
            // This lets a later generation stop the dispatcher and remove policy.
            eprintln!("sekka-vpn-guard: {error}");
        } else {
            return Err(error);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match request(&env::args().skip(1).collect::<Vec<_>>()).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sekka-vpn-guard: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_exceptions_are_canonical_destination_prefixes() {
        let mut policy = Policy::default();
        add_trusted_prefixes(&mut policy, r#"["2001:db8::a/128","10.0.0.0/8"]"#).unwrap();
        assert!(policy.trusted_v6.contains("2001:db8::a/128"));
        assert!(policy.trusted_v4.contains("10.0.0.0/8"));
        for json in [r#"["10.1.2.3/8"]"#, r#"["1.2.3.4/33"]"#, r#"["eth0"]"#] {
            assert!(add_trusted_prefixes(&mut policy, json).is_err());
        }
    }
}
