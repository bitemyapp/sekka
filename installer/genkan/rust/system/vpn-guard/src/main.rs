// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

mod family;
mod firewall;
mod nm;

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::fs::{self, DirBuilder, File, Permissions};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::process::ExitCode;

use family::Family;
use ipnet::IpNet;
use nm::Tunnel;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// What the firewall should hold.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// The family refused outside a tunnel: the one no full tunnel carries
    /// while a full tunnel carries the other.
    pub blocked: Option<Family>,
    /// The profiles that carry a whole family, for the status display.
    pub carriers: BTreeSet<String>,
    /// Marks on tunnels' own encrypted packets.
    pub marks: BTreeSet<NonZeroU32>,
    /// Servers of unmarked tunnels that are reached over the blocked family.
    pub endpoints: BTreeSet<SocketAddr>,
    /// Destinations the configuration allows outside the tunnel.
    pub trusted: BTreeSet<IpNet>,
}

impl Policy {
    fn for_tunnels(tunnels: &[Tunnel]) -> Self {
        let carried = |family| tunnels.iter().any(|t| t.carries.contains(&family));
        let mut policy = Policy {
            blocked: Family::BOTH
                .into_iter()
                .find(|&family| !carried(family) && carried(family.other())),
            carriers: tunnels
                .iter()
                .filter(|t| !t.carries.is_empty())
                .map(|t| t.name.clone())
                .collect(),
            ..Policy::default()
        };
        let Some(blocked) = policy.blocked else {
            return policy;
        };
        // Every tunnel's own packets stay out of the block, split tunnels'
        // too: by mark where the device sets one, otherwise by the servers
        // the profile gives by address.
        for tunnel in tunnels {
            match tunnel.mark {
                Some(mark) => {
                    policy.marks.insert(mark);
                }
                None => policy.endpoints.extend(
                    tunnel
                        .servers
                        .iter()
                        .filter(|server| Family::of(server.ip()) == blocked),
                ),
            }
        }
        policy
    }
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
        _ => {
            return Err(
                "usage: sekka-vpn-guard {start|stop|reconcile|clear|status|validate-config}".into(),
            );
        }
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

fn verified_status(blocked: Option<Family>, carriers: &BTreeSet<String>) -> String {
    let mut text = String::new();
    for family in Family::BOTH {
        let state = if blocked == Some(family) {
            "blocked"
        } else {
            "open"
        };
        text.push_str(&format!("{}: {state}\n", family.name()));
    }
    if !carriers.is_empty() {
        let names: Vec<_> = carriers.iter().map(String::as_str).collect();
        text.push_str(&format!("Full tunnel: {}\n", names.join(", ")));
    }
    text
}

fn trusted_prefixes(json: &str) -> Result<BTreeSet<IpNet>> {
    let mut trusted = BTreeSet::new();
    for prefix in serde_json::from_str::<Vec<String>>(json)? {
        let net: IpNet = prefix
            .parse()
            .map_err(|_| format!("invalid trusted LAN prefix: {prefix}"))?;
        if net.network() != net.addr() {
            return Err(format!("trusted LAN prefix has host bits set: {prefix}").into());
        }
        trusted.insert(net);
    }
    Ok(trusted)
}

fn reconcile() -> Result<()> {
    let config = fs::read_to_string(CONFIG).map_err(|e| format!("{CONFIG}: {e}"))?;
    let Some(tunnels) = nm::tunnels()? else {
        // Without NetworkManager nothing is bringing a tunnel up or taking
        // one down. Whatever is installed stays until it is back, as at boot,
        // when the guard starts first, and across a NetworkManager restart.
        save_status(&verified_status(
            firewall::blocked_family()?,
            &BTreeSet::new(),
        ))?;
        return Ok(());
    };
    let mut policy = Policy::for_tunnels(&tunnels);
    policy.trusted = trusted_prefixes(&config)?;
    firewall::apply(&policy)?;
    save_status(&verified_status(policy.blocked, &policy.carriers))?;
    Ok(())
}

fn run(action: Request) -> Result<()> {
    match action {
        Request::Ignore => return Ok(()),
        Request::ValidateConfig => {
            return trusted_prefixes(&io::read_to_string(io::stdin())?).map(drop);
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
            save_status(&verified_status(None, &BTreeSet::new()))?;
            return Ok(());
        }
        Request::Start => {
            File::create(ARMED)?;
        }
        Request::Reconcile if !fs::exists(ARMED)? || !fs::exists(CONFIG)? => return Ok(()),
        _ => {}
    }
    if let Err(error) = reconcile() {
        // A read NetworkManager did not answer changes nothing: an installed
        // block stays. Its next event runs the guard again.
        let observed = match firewall::blocked_family() {
            Ok(Some(family)) => format!("{} egress block installed", family.name()),
            Ok(None) => "no egress block installed".to_owned(),
            Err(_) => "installed policy unknown".to_owned(),
        };
        save_status(&format!(
            "Degraded: {observed}. The next network event retries.\n"
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

    fn tunnel(name: &str, carries: &[Family], mark: u32, servers: &[&str]) -> Tunnel {
        Tunnel {
            name: name.to_owned(),
            carries: carries.iter().copied().collect(),
            mark: NonZeroU32::new(mark),
            servers: servers.iter().map(|s| s.parse().unwrap()).collect(),
        }
    }

    #[test]
    fn trusted_exceptions_are_canonical_destination_prefixes() {
        let trusted = trusted_prefixes(r#"["2001:db8::a/128","10.0.0.0/8"]"#).unwrap();
        assert!(trusted.contains(&"2001:db8::a/128".parse::<IpNet>().unwrap()));
        assert!(trusted.contains(&"10.0.0.0/8".parse::<IpNet>().unwrap()));
        for json in [r#"["10.1.2.3/8"]"#, r#"["1.2.3.4/33"]"#, r#"["eth0"]"#] {
            assert!(trusted_prefixes(json).is_err());
        }
    }

    #[test]
    fn the_family_only_one_full_tunnel_leaves_out_is_blocked() {
        let v4 = tunnel("GLADOS", &[Family::V4], 7, &["203.0.113.20:51820"]);
        let v6 = tunnel("WHEATLEY", &[Family::V6], 8, &["203.0.113.20:51821"]);
        let split = tunnel("SPLIT", &[], 0, &["203.0.113.20:51820"]);

        let policy = Policy::for_tunnels(std::slice::from_ref(&v4));
        assert_eq!(policy.blocked, Some(Family::V6));
        assert_eq!(policy.carriers, BTreeSet::from(["GLADOS".to_owned()]));
        assert_eq!(
            Policy::for_tunnels(std::slice::from_ref(&v6)).blocked,
            Some(Family::V4)
        );

        // One tunnel per family leaves nothing to block, and neither does a
        // split tunnel or no tunnel at all.
        let both = Policy::for_tunnels(&[v4, v6]);
        assert_eq!(both.blocked, None);
        assert_eq!(both.carriers.len(), 2);
        assert!(both.marks.is_empty());
        assert_eq!(Policy::for_tunnels(&[split]), Policy::default());
        assert_eq!(Policy::for_tunnels(&[]), Policy::default());
    }

    #[test]
    fn tunnels_keep_reaching_their_servers_in_the_blocked_family() {
        let full = tunnel("HALVES", &[Family::V4], 0, &["[2001:db8::20]:51820"]);
        let marked = tunnel("WORK", &[], 9, &["[2001:db8::30]:51820"]);
        let split = tunnel(
            "HOME",
            &[],
            0,
            &["[2001:db8::40]:51820", "198.51.100.7:51820"],
        );
        let policy = Policy::for_tunnels(&[full, marked, split]);
        assert_eq!(policy.blocked, Some(Family::V6));
        assert_eq!(policy.marks, BTreeSet::from([NonZeroU32::new(9).unwrap()]));
        // A server in the family that stays open needs no exception.
        let servers: [SocketAddr; 2] = [
            "[2001:db8::20]:51820".parse().unwrap(),
            "[2001:db8::40]:51820".parse().unwrap(),
        ];
        assert_eq!(policy.endpoints, BTreeSet::from(servers));
    }
}
