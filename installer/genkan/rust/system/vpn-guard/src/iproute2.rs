// SPDX-License-Identifier: GPL-3.0-or-later
//! The running system, read and changed through iproute2 and `wg`.
//!
//! The kernel decides where a packet goes, so it is asked (`ip route get`)
//! instead of being imitated, and iproute2's JSON is what is parsed. Only
//! whether a command succeeded is taken from it, never the words of an
//! error: what a failed change left behind is learned by reading the system
//! again (system.rs). The package fixes both programs' paths when it builds
//! this; a build without them finds the programs on PATH.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::net::IpAddr;
use std::process::Command;

use serde::Deserialize;

use crate::family::Family;
use crate::guard::{Change, Device, Guard, PROTOCOL, Path, Route, Rule, TABLE, Tunnel};
use crate::system::System;

const IP: &str = match option_env!("SEKKA_VPN_GUARD_IP") {
    Some(path) => path,
    None => "ip",
};
const WG: &str = match option_env!("SEKKA_VPN_GUARD_WG") {
    Some(path) => path,
    None => "wg",
};

#[derive(Debug)]
pub enum Error {
    /// The program could not be started.
    Start {
        program: &'static str,
        source: io::Error,
    },
    /// The program ran and did not succeed. What it printed is kept for the
    /// journal and is not interpreted.
    Refused { command: String, said: String },
    /// The program's output was not what this guard reads.
    Unreadable { command: String, why: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Start { program, source } => write!(f, "could not run {program}: {source}"),
            Error::Refused { command, said } => write!(f, "`{command}` failed: {said}"),
            Error::Unreadable { command, why } => {
                write!(f, "could not read what `{command}` printed: {why}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// What a program printed, and whether it succeeded.
struct Ran {
    command: String,
    succeeded: bool,
    out: String,
    err: String,
}

impl Ran {
    fn refused(self) -> Error {
        Error::Refused {
            command: self.command,
            said: self.err.trim().to_owned(),
        }
    }

    fn read<T>(&self, parse: impl FnOnce(&str) -> Result<T, String>) -> Result<T, Error> {
        parse(&self.out).map_err(|why| Error::Unreadable {
            command: self.command.clone(),
            why,
        })
    }
}

fn run(program: &'static str, arguments: &[&str]) -> Result<Ran, Error> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|source| Error::Start { program, source })?;
    let name = program.rsplit('/').next().unwrap_or(program);
    Ok(Ran {
        command: format!("{name} {}", arguments.join(" ")),
        succeeded: output.status.success(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// The running system.
pub struct Iproute2;

impl System for Iproute2 {
    type Error = Error;

    fn path(&mut self, family: Family) -> Result<Path, Error> {
        let to = family.internet().to_string();
        let route = run(IP, &["-json", family.flag(), "route", "get", &to])?;
        if !route.succeeded {
            // No route, or an unreachable one such as the guard's own.
            return Ok(Path::Nowhere);
        }
        let Some(device) = route.read(route_device)? else {
            return Ok(Path::Nowhere);
        };
        let link = run(IP, &["-json", "-details", "link", "show", "dev", &device.0])?;
        if !link.succeeded {
            return Err(link.refused());
        }
        let kind = link
            .read(link_kinds)?
            .into_iter()
            .next()
            .and_then(|(_, kind)| kind);
        Ok(classify(device, kind.as_deref()))
    }

    fn tunnels(&mut self) -> Result<Vec<Tunnel>, Error> {
        let links = run(
            IP,
            &["-json", "-details", "link", "show", "type", "wireguard"],
        )?;
        if !links.succeeded {
            return Err(links.refused());
        }
        let mut tunnels = Vec::new();
        for (device, _) in links.read(link_kinds)? {
            let shown = run(WG, &["show", &device.0, "endpoints"])?;
            if !shown.succeeded {
                return Err(shown.refused());
            }
            tunnels.push(Tunnel {
                device,
                endpoints: endpoints(&shown.out),
            });
        }
        Ok(tunnels)
    }

    fn guard(&mut self, family: Family) -> Result<Guard, Error> {
        let listed = run(IP, &["-json", family.flag(), "rule", "show"])?;
        if !listed.succeeded {
            return Err(listed.refused());
        }
        let rules = listed.read(guard_rules)?;
        let table = TABLE.to_string();
        // A table nothing was ever put in does not exist, and listing it
        // fails. If the listing failed for another reason the routes read as
        // none, and the run ends with the guard not as wanted.
        let listed = run(
            IP,
            &["-json", family.flag(), "route", "show", "table", &table],
        )?;
        let routes = if listed.succeeded {
            listed.read(guard_routes)?
        } else {
            BTreeSet::new()
        };
        Ok(Guard::found(rules, routes))
    }

    fn apply(&mut self, family: Family, change: &Change) -> Result<(), Error> {
        let arguments = arguments(family, change);
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let ran = run(IP, &arguments)?;
        if ran.succeeded {
            Ok(())
        } else {
            Err(ran.refused())
        }
    }
}

/// The `ip` arguments that make one change.
fn arguments(family: Family, change: &Change) -> Vec<String> {
    let table = TABLE.to_string();
    let protocol = PROTOCOL.to_string();
    let rule = |verb: &str, rule: Rule| {
        let priority = rule.priority().to_string();
        let mut words = vec![
            family.flag(),
            "rule",
            verb,
            "priority",
            priority.as_str(),
            "lookup",
        ];
        match rule {
            Rule::Local => words.extend(["main", "suppress_prefixlength", "0"]),
            Rule::Table => words.push(table.as_str()),
        }
        words.extend(["protocol", protocol.as_str()]);
        words.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    let route = |route: Route| match route {
        Route::Throw(address) => vec![
            "throw".to_owned(),
            format!("{address}/{}", family.host_prefix()),
        ],
        Route::Unreachable => vec!["unreachable".to_owned(), "default".to_owned()],
    };
    let in_table = |verb: &str, selector: Vec<String>, marked: bool| {
        let mut words = vec![
            family.flag().to_owned(),
            "route".to_owned(),
            verb.to_owned(),
        ];
        words.extend(selector);
        words.extend(["table".to_owned(), table.clone()]);
        if marked {
            words.extend(["proto".to_owned(), protocol.clone()]);
        }
        words
    };
    match *change {
        Change::AddRule(added) => rule("add", added),
        Change::RemoveRule(removed) => rule("del", removed),
        // `replace` creates the route or leaves it as it is: it cannot find
        // it "already there".
        Change::SetRoute(set) => in_table("replace", route(set), true),
        Change::DropRoute(dropped) => in_table("del", route(dropped), false),
        Change::ClearRoutes => in_table("flush", Vec::new(), false),
    }
}

/// Which kind of path a device is, from its link kind.
fn classify(device: Device, kind: Option<&str>) -> Path {
    match kind {
        Some("wireguard") => Path::WireGuard(device),
        Some("tun" | "xfrm" | "vti" | "vti6") => Path::OtherTunnel(device),
        _ => Path::Direct(device),
    }
}

fn json<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

/// The device in `ip -json route get`'s answer.
fn route_device(text: &str) -> Result<Option<Device>, String> {
    #[derive(Deserialize)]
    struct Route {
        dev: Option<String>,
    }
    let routes: Vec<Route> = json(text)?;
    Ok(routes
        .into_iter()
        .next()
        .and_then(|route| route.dev)
        .map(Device))
}

/// Each device in `ip -json -details link show` with its link kind, which
/// ordinary Ethernet and Wi-Fi devices do not have.
fn link_kinds(text: &str) -> Result<Vec<(Device, Option<String>)>, String> {
    #[derive(Deserialize)]
    struct Link {
        ifname: String,
        linkinfo: Option<LinkInfo>,
    }
    #[derive(Deserialize)]
    struct LinkInfo {
        info_kind: Option<String>,
    }
    let links: Vec<Link> = json(text)?;
    Ok(links
        .into_iter()
        .map(|link| {
            (
                Device(link.ifname),
                link.linkinfo.and_then(|info| info.info_kind),
            )
        })
        .collect())
}

/// The guard's own rules among `ip -json rule show`'s: those that carry its
/// protocol number and have one of its two shapes.
fn guard_rules(text: &str) -> Result<BTreeSet<Rule>, String> {
    #[derive(Deserialize)]
    struct Listed {
        priority: Option<u32>,
        dst: Option<String>,
        table: Option<String>,
        suppress_prefixlen: Option<i64>,
        protocol: Option<String>,
    }
    let listed: Vec<Listed> = json(text)?;
    let (ours, table) = (PROTOCOL.to_string(), TABLE.to_string());
    let mut rules = BTreeSet::new();
    for rule in listed {
        if rule.protocol.as_deref() != Some(ours.as_str()) || rule.dst.is_some() {
            continue;
        }
        let recognised = match (rule.table.as_deref(), rule.suppress_prefixlen) {
            (Some("main"), Some(0)) => Rule::Local,
            (Some(to), None) if to == table => Rule::Table,
            _ => continue,
        };
        if rule.priority == Some(recognised.priority()) {
            rules.insert(recognised);
        }
    }
    Ok(rules)
}

/// What `ip -json route show table` lists of the guard's routes: the
/// unreachable default, and a `throw` route for each single address.
fn guard_routes(text: &str) -> Result<BTreeSet<Route>, String> {
    #[derive(Deserialize)]
    struct Listed {
        #[serde(rename = "type")]
        kind: Option<String>,
        dst: Option<String>,
    }
    let listed: Vec<Listed> = json(text)?;
    Ok(listed
        .iter()
        .filter_map(
            |route| match (route.kind.as_deref(), route.dst.as_deref()) {
                (Some("unreachable"), Some("default")) => Some(Route::Unreachable),
                // iproute2 leaves the length out for a single address.
                (Some("throw"), Some(address)) => address.parse().ok().map(Route::Throw),
                _ => None,
            },
        )
        .collect())
}

/// The addresses in `wg show DEVICE endpoints`: a line for each peer, its
/// public key, a tab, then `address:port`, `[address]:port` or `(none)`.
fn endpoints(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(_, end)| end.trim().rsplit_once(':'))
        .filter_map(|(host, _port)| {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            // A link-local address comes with its interface after a %.
            host.split('%').next()?.parse().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Everything quoted here is what iproute2 7.2.0 and wireguard-tools
    // 1.0.20260223 printed in a network namespace made for the purpose.

    fn device(name: &str) -> Device {
        Device(name.to_owned())
    }

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_route_names_its_device() {
        let v4 = r#"[{"dst":"192.0.2.1","gateway":"192.168.1.1","dev":"eth1","prefsrc":"192.168.1.128","flags":[],"uid":0,"cache":[]}]"#;
        let v6 = r#"[{"dst":"2001:db8::1","from":"::","gateway":"2001:db8:1::1","dev":"eth1","prefsrc":"2001:db8:1::128","metric":1024,"flags":[],"pref":"medium"}]"#;
        assert_eq!(route_device(v4), Ok(Some(device("eth1"))));
        assert_eq!(route_device(v6), Ok(Some(device("eth1"))));
        assert_eq!(route_device("[]"), Ok(None));
        assert!(route_device("RTNETLINK answers: No route to host").is_err());
    }

    #[test]
    fn links_are_told_apart_by_kind() {
        let wireguard = r#"[{"ifindex":3,"ifname":"GLADOS","flags":["POINTOPOINT","NOARP"],"mtu":1420,"qdisc":"noop","operstate":"DOWN","link_type":"none","linkinfo":{"info_kind":"wireguard"},"num_tx_queues":1}]"#;
        let ethernet = r#"[{"ifindex":2,"ifname":"enp3s0","flags":["BROADCAST","MULTICAST","UP","LOWER_UP"],"mtu":1500,"link_type":"ether","address":"3a:18:fa:64:cc:86"}]"#;
        let kind = |text| link_kinds(text).unwrap().remove(0);
        let (name, link) = kind(wireguard);
        assert_eq!(
            classify(name, link.as_deref()),
            Path::WireGuard(device("GLADOS"))
        );
        let (name, link) = kind(ethernet);
        assert_eq!(
            classify(name, link.as_deref()),
            Path::Direct(device("enp3s0"))
        );
        assert_eq!(
            classify(device("tun0"), Some("tun")),
            Path::OtherTunnel(device("tun0"))
        );
        assert_eq!(
            classify(device("br0"), Some("bridge")),
            Path::Direct(device("br0"))
        );
    }

    #[test]
    fn only_the_guards_own_rules_are_taken_for_its_own() {
        // NetworkManager's tunnel rule, the guard's two, and the kernel's.
        let v4 = r#"[{"priority":0,"src":"all","table":"local"},{"priority":31713,"not":null,"src":"all","fwmark":"0xcc29","table":"52265"},{"priority":32764,"src":"all","table":"main","suppress_prefixlen":0,"protocol":"83"},{"priority":32765,"src":"all","table":"5458759","protocol":"83"},{"priority":32766,"src":"all","table":"main"},{"priority":32767,"src":"all","table":"default"}]"#;
        assert_eq!(guard_rules(v4), Ok(BTreeSet::from(Rule::BOTH)));
        let v6 = r#"[{"priority":0,"src":"all","table":"local"},{"priority":32764,"src":"all","table":"main","suppress_prefixlen":0,"protocol":"83"},{"priority":32766,"src":"all","table":"main"}]"#;
        assert_eq!(guard_rules(v6), Ok(BTreeSet::from([Rule::Local])));
        // The same shapes without the guard's number are someone else's:
        // wg-quick adds this very rule for the family its tunnel carries.
        let theirs = r#"[{"priority":32764,"src":"all","table":"main","suppress_prefixlen":0},{"priority":32765,"not":null,"src":"all","fwmark":"0xca6c","table":"51820"}]"#;
        assert_eq!(guard_rules(theirs), Ok(BTreeSet::new()));
        // A rule for one address, or at another priority, is not one of the
        // guard's two even with its number.
        let other = r#"[{"priority":32763,"src":"all","dst":"203.0.113.20","table":"main","protocol":"83"},{"priority":100,"src":"all","table":"5458759","protocol":"83"}]"#;
        assert_eq!(guard_rules(other), Ok(BTreeSet::new()));
    }

    #[test]
    fn the_guards_routes_are_read_from_its_table() {
        let v4 = r#"[{"type":"unreachable","dst":"default","protocol":"83","flags":[]},{"type":"throw","dst":"203.0.113.20","protocol":"83","flags":[]}]"#;
        assert_eq!(
            guard_routes(v4),
            Ok(BTreeSet::from([
                Route::Throw(address("203.0.113.20")),
                Route::Unreachable
            ]))
        );
        let v6 = r#"[{"type":"throw","dst":"2001:db8:ffff::20","dev":"lo","protocol":"83","metric":1024,"flags":[],"pref":"medium"},{"type":"unreachable","dst":"default","dev":"lo","protocol":"83","metric":1024,"flags":[],"pref":"medium"}]"#;
        assert_eq!(
            guard_routes(v6),
            Ok(BTreeSet::from([
                Route::Throw(address("2001:db8:ffff::20")),
                Route::Unreachable
            ]))
        );
        assert_eq!(guard_routes("[]"), Ok(BTreeSet::new()));
        // A route to a tunnel, or a throw for a whole network, is neither.
        let other = r#"[{"dst":"default","dev":"GLADOS","flags":[]},{"type":"throw","dst":"203.0.113.0/24","flags":[]}]"#;
        assert_eq!(guard_routes(other), Ok(BTreeSet::new()));
    }

    #[test]
    fn tunnel_ends_are_read_in_every_form_wg_prints() {
        let shown = "sCrJRXeNvlClXkSSrBQsmarcdcWGadEITCsZpjpwFC0=\t[2001:db8:ffff::20]:51821\n\
                     qzTOzNaqz9B5fwdcgAfRmeBxeG9Mwn3MGPLqGx9V1iE=\t(none)\n\
                     xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\t203.0.113.20:51820\n\
                     HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=\t[fe80::1%wlo1]:51820\n";
        assert_eq!(
            endpoints(shown),
            [
                address("2001:db8:ffff::20"),
                address("203.0.113.20"),
                address("fe80::1")
            ]
        );
        assert_eq!(endpoints(""), Vec::<IpAddr>::new());
    }

    #[test]
    fn each_change_is_one_ip_command() {
        let words = |family, change: Change| arguments(family, &change).join(" ");
        assert_eq!(
            words(Family::V6, Change::AddRule(Rule::Local)),
            "-6 rule add priority 32764 lookup main suppress_prefixlength 0 protocol 83"
        );
        assert_eq!(
            words(Family::V4, Change::AddRule(Rule::Table)),
            "-4 rule add priority 32765 lookup 5458759 protocol 83"
        );
        assert_eq!(
            words(Family::V4, Change::RemoveRule(Rule::Table)),
            "-4 rule del priority 32765 lookup 5458759 protocol 83"
        );
        assert_eq!(
            words(
                Family::V4,
                Change::SetRoute(Route::Throw(address("203.0.113.20")))
            ),
            "-4 route replace throw 203.0.113.20/32 table 5458759 proto 83"
        );
        assert_eq!(
            words(
                Family::V6,
                Change::SetRoute(Route::Throw(address("2001:db8:ffff::20")))
            ),
            "-6 route replace throw 2001:db8:ffff::20/128 table 5458759 proto 83"
        );
        assert_eq!(
            words(Family::V6, Change::SetRoute(Route::Unreachable)),
            "-6 route replace unreachable default table 5458759 proto 83"
        );
        assert_eq!(
            words(
                Family::V4,
                Change::DropRoute(Route::Throw(address("203.0.113.20")))
            ),
            "-4 route del throw 203.0.113.20/32 table 5458759"
        );
        assert_eq!(
            words(Family::V4, Change::DropRoute(Route::Unreachable)),
            "-4 route del unreachable default table 5458759"
        );
        assert_eq!(
            words(Family::V6, Change::ClearRoutes),
            "-6 route flush table 5458759"
        );
    }
}
