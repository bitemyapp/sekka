// SPDX-License-Identifier: GPL-3.0-or-later
//! What the guard is made of, when a family needs it, and the steps from one
//! state of it to another.
//!
//! A full-tunnel WireGuard connection sends one family's internet traffic
//! into the tunnel by policy routing: a rule that looks in the main table for
//! everything but a default route, then a rule that sends the rest to a table
//! whose default route is the tunnel. NetworkManager and wg-quick both do
//! this, and only for the families the tunnel's AllowedIPs cover.
//!
//! The guard gives each family the same two rules over a table of its own.
//! They stay in place and change nothing while that table is empty. To close
//! a family the guard puts an unreachable default route in the table, and
//! beside it a `throw` route for each tunnel's far end, which sends that one
//! address on to the main table. So everything that changes while the system
//! runs is a route, set with `ip route replace` and cleared with a flush:
//! both can be repeated, and neither has an "already there" to be told apart
//! from a failure.

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;

use crate::family::{Family, PerFamily};

/// The table that holds the guard's routes ("SKG").
pub const TABLE: u32 = 0x53_4B_47;

/// The routing protocol number the guard's rules and routes carry, so that it
/// recognises its own rules and never takes another program's for them ("S").
pub const PROTOCOL: u8 = 83;

/// A network device, by the name the kernel gives it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Device(pub String);

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where one family's internet traffic goes when an ordinary program sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Path {
    /// Into a WireGuard tunnel.
    WireGuard(Device),
    /// Into another kind of tunnel, which the guard neither counts as cover
    /// nor blocks: it cannot tell where that tunnel's own far end is.
    OtherTunnel(Device),
    /// Out by an ordinary interface.
    Direct(Device),
    /// Nowhere: the family has no route, or the guard already blocks it.
    Nowhere,
}

/// A WireGuard device and the addresses of its peers' far ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub device: Device,
    pub endpoints: Vec<IpAddr>,
}

/// One of the guard's two policy rules, in the order the kernel reads them.
/// Both stay in place whether or not the family is closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// The main table without its default route: the home network, link-local
    /// addresses and any route more specific than a default stay in force.
    Local,
    /// Everything left is looked up in the guard's table. While that table
    /// is empty the lookup finds nothing and the main table answers as usual.
    Table,
}

impl Rule {
    pub const BOTH: [Rule; 2] = [Rule::Local, Rule::Table];

    /// Where the rule sits among the family's rules. NetworkManager's and
    /// wg-quick's tunnel rules come earlier, so a tunnel that starts carrying
    /// a closed family wins over the block at once; the main table's own
    /// rule (32766) comes later.
    pub fn priority(self) -> u32 {
        match self {
            Rule::Local => 32764,
            Rule::Table => 32765,
        }
    }
}

/// A route in the guard's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    /// A tunnel's far end is looked up in the main table after all, as it
    /// was before the guard. Without this the tunnel's own packets, and the
    /// replies the reverse-path filter checks, would meet the block.
    Throw(IpAddr),
    /// Every other address is unreachable.
    Unreachable,
}

/// The guard in one family: which of its rules are in place and what its
/// table holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    rules: BTreeSet<Rule>,
    routes: BTreeSet<Route>,
}

impl Guard {
    /// Nothing of the guard at all, as before its first run and after it is
    /// cleared.
    pub fn absent() -> Guard {
        Guard::default()
    }

    /// The two rules over an empty table: the family is as the rest of the
    /// system left it.
    pub fn open() -> Guard {
        Guard {
            rules: BTreeSet::from(Rule::BOTH),
            routes: BTreeSet::new(),
        }
    }

    /// The family turned away, with the given tunnel ends still reachable.
    pub fn closed(endpoints: impl IntoIterator<Item = IpAddr>) -> Guard {
        let mut routes: BTreeSet<Route> = endpoints.into_iter().map(Route::Throw).collect();
        routes.insert(Route::Unreachable);
        Guard {
            rules: BTreeSet::from(Rule::BOTH),
            routes,
        }
    }

    /// Whatever was found installed, which may be part of a guard.
    pub fn found(rules: BTreeSet<Rule>, routes: BTreeSet<Route>) -> Guard {
        Guard { rules, routes }
    }

    /// Whether internet traffic of the family is turned away.
    pub fn blocks(&self) -> bool {
        self.rules.contains(&Rule::Table) && self.routes.contains(&Route::Unreachable)
    }

    /// Whether what a step is for is in place. A step is judged by this and
    /// not by what the command that made it said.
    pub fn has(&self, change: &Change) -> bool {
        match change {
            Change::AddRule(rule) => self.rules.contains(rule),
            Change::RemoveRule(rule) => !self.rules.contains(rule),
            Change::SetRoute(route) => self.routes.contains(route),
            Change::DropRoute(route) => !self.routes.contains(route),
            Change::ClearRoutes => self.routes.is_empty(),
        }
    }

    /// The guard as one step leaves it.
    #[cfg(test)]
    pub fn after(&self, change: &Change) -> Guard {
        let mut next = self.clone();
        match change {
            Change::AddRule(rule) => drop(next.rules.insert(*rule)),
            Change::RemoveRule(rule) => drop(next.rules.remove(rule)),
            Change::SetRoute(route) => drop(next.routes.insert(*route)),
            Change::DropRoute(route) => drop(next.routes.remove(route)),
            Change::ClearRoutes => next.routes.clear(),
        }
        next
    }
}

impl fmt::Display for Guard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rules: Vec<String> = self
            .rules
            .iter()
            .map(|rule| match rule {
                Rule::Local => "main without its default".to_owned(),
                Rule::Table => "the guard's table".to_owned(),
            })
            .collect();
        let routes: Vec<String> = self
            .routes
            .iter()
            .map(|route| match route {
                Route::Throw(address) => format!("throw {address}"),
                Route::Unreachable => "unreachable default".to_owned(),
            })
            .collect();
        let list = |items: &[String]| {
            if items.is_empty() {
                "none".to_owned()
            } else {
                items.join(", ")
            }
        };
        write!(f, "rules: {}; routes: {}", list(&rules), list(&routes))
    }
}

/// What a family needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Nothing: no WireGuard tunnel carries the other family alone.
    Open,
    /// The guard: this tunnel carries the other family and not this one, so
    /// this one would leave outside it.
    Close { carried_by: &'a Device },
}

/// Decides what `family` needs from where both families' traffic goes.
pub fn verdict(paths: &PerFamily<Path>, family: Family) -> Verdict<'_> {
    let Path::WireGuard(carried_by) = paths.get(family.other()) else {
        return Verdict::Open;
    };
    match paths.get(family) {
        Path::Direct(_) | Path::Nowhere => Verdict::Close { carried_by },
        Path::WireGuard(_) | Path::OtherTunnel(_) => Verdict::Open,
    }
}

/// The guard `family` should have.
pub fn wanted(paths: &PerFamily<Path>, tunnels: &[Tunnel], family: Family) -> Guard {
    match verdict(paths, family) {
        Verdict::Open => Guard::open(),
        Verdict::Close { .. } => Guard::closed(
            tunnels
                .iter()
                .flat_map(|tunnel| tunnel.endpoints.iter().copied())
                .filter(|endpoint| Family::of(*endpoint) == family),
        ),
    }
}

/// One step that changes the guard in a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Puts a rule in. Only asked for when the rule was not found.
    AddRule(Rule),
    /// Takes a rule out, when the guard is cleared.
    RemoveRule(Rule),
    /// Puts a route in the guard's table, or leaves it if it is there.
    SetRoute(Route),
    /// Takes one route out of the table while others stay.
    DropRoute(Route),
    /// Empties the table.
    ClearRoutes,
}

/// The steps from one guard to another. Whatever the two are, no step puts
/// the block in force without the rule that keeps the home network reachable
/// and the routes that keep the tunnels' far ends reachable: those go in
/// before it and come out after it.
pub fn changes(from: &Guard, to: &Guard) -> Vec<Change> {
    let throw = |route: &&Route| matches!(route, Route::Throw(_));
    let adds = |rule: Rule| to.rules.contains(&rule) && !from.rules.contains(&rule);
    let mut steps = Vec::new();

    if adds(Rule::Local) {
        steps.push(Change::AddRule(Rule::Local));
    }
    steps.extend(
        to.routes
            .difference(&from.routes)
            .filter(throw)
            .copied()
            .map(Change::SetRoute),
    );
    if adds(Rule::Table) {
        steps.push(Change::AddRule(Rule::Table));
    }
    if to.routes.contains(&Route::Unreachable) && !from.routes.contains(&Route::Unreachable) {
        steps.push(Change::SetRoute(Route::Unreachable));
    }

    if to.routes.is_empty() {
        if !from.routes.is_empty() {
            steps.push(Change::ClearRoutes);
        }
    } else {
        if from.routes.contains(&Route::Unreachable) && !to.routes.contains(&Route::Unreachable) {
            steps.push(Change::DropRoute(Route::Unreachable));
        }
        steps.extend(
            from.routes
                .difference(&to.routes)
                .filter(throw)
                .copied()
                .map(Change::DropRoute),
        );
    }
    for rule in [Rule::Table, Rule::Local] {
        if from.rules.contains(&rule) && !to.rules.contains(&rule) {
            steps.push(Change::RemoveRule(rule));
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str) -> Device {
        Device(name.to_owned())
    }

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn paths(v4: Path, v6: Path) -> PerFamily<Path> {
        PerFamily { v4, v6 }
    }

    /// The block in force with the home network's rule missing.
    fn cut_off(guard: &Guard) -> bool {
        guard.blocks() && !guard.rules.contains(&Rule::Local)
    }

    /// The block in force with a wanted tunnel end not let through.
    fn tunnel_cut(guard: &Guard, to: &Guard) -> bool {
        let throws = |route: &&Route| matches!(route, Route::Throw(_));
        guard.blocks()
            && to.blocks()
            && to
                .routes
                .iter()
                .filter(throws)
                .any(|route| !guard.routes.contains(route))
    }

    /// Applies the steps one at a time and checks that none of them cuts off
    /// the home network or a wanted tunnel end that was reachable before it.
    fn walk(from: &Guard, to: &Guard) -> Guard {
        let mut now = from.clone();
        for step in changes(from, to) {
            let next = now.after(&step);
            assert_ne!(next, now, "{step:?} changed nothing from {now:?}");
            assert!(next.has(&step));
            assert!(
                !cut_off(&next) || cut_off(&now),
                "{step:?} cut off the home network: {next:?}"
            );
            assert!(
                !tunnel_cut(&next, to) || tunnel_cut(&now, to),
                "{step:?} cut off a tunnel end: {next:?}"
            );
            now = next;
        }
        now
    }

    #[test]
    fn nothing_is_guarded_without_a_tunnel() {
        let seen = paths(Path::Direct(device("wlo1")), Path::Direct(device("wlo1")));
        for family in Family::BOTH {
            assert_eq!(verdict(&seen, family), Verdict::Open);
            assert_eq!(wanted(&seen, &[], family), Guard::open());
        }
    }

    #[test]
    fn an_ipv4_only_tunnel_closes_ipv6() {
        let tunnel = device("GLADOS");
        let seen = paths(
            Path::WireGuard(tunnel.clone()),
            Path::Direct(device("wlo1")),
        );
        assert_eq!(verdict(&seen, Family::V4), Verdict::Open);
        assert_eq!(
            verdict(&seen, Family::V6),
            Verdict::Close {
                carried_by: &tunnel
            }
        );
    }

    #[test]
    fn an_ipv6_only_tunnel_closes_ipv4() {
        let tunnel = device("WHEATLEY");
        let seen = paths(
            Path::Direct(device("wlo1")),
            Path::WireGuard(tunnel.clone()),
        );
        assert_eq!(
            verdict(&seen, Family::V4),
            Verdict::Close {
                carried_by: &tunnel
            }
        );
        assert_eq!(verdict(&seen, Family::V6), Verdict::Open);
    }

    #[test]
    fn a_closed_family_stays_closed_while_the_tunnel_is_up() {
        // With the guard in force the family's own traffic goes nowhere.
        let tunnel = device("GLADOS");
        let seen = paths(Path::WireGuard(tunnel.clone()), Path::Nowhere);
        assert_eq!(
            verdict(&seen, Family::V6),
            Verdict::Close {
                carried_by: &tunnel
            }
        );
    }

    #[test]
    fn a_tunnel_that_carries_both_families_needs_no_guard() {
        let seen = paths(
            Path::WireGuard(device("wg0")),
            Path::WireGuard(device("wg0")),
        );
        for family in Family::BOTH {
            assert_eq!(verdict(&seen, family), Verdict::Open);
        }
    }

    #[test]
    fn a_family_in_another_kind_of_tunnel_is_left_alone() {
        let seen = paths(
            Path::WireGuard(device("wg0")),
            Path::OtherTunnel(device("tun0")),
        );
        assert_eq!(verdict(&seen, Family::V6), Verdict::Open);
        // And another kind of tunnel is not taken as cover for the other family.
        let seen = paths(
            Path::OtherTunnel(device("tun0")),
            Path::Direct(device("wlo1")),
        );
        assert_eq!(verdict(&seen, Family::V6), Verdict::Open);
    }

    #[test]
    fn a_split_tunnel_closes_nothing() {
        // Its routes are more specific than a default, so the internet at
        // large still leaves directly in both families.
        let seen = paths(Path::Direct(device("wlo1")), Path::Direct(device("wlo1")));
        assert_eq!(verdict(&seen, Family::V6), Verdict::Open);
    }

    #[test]
    fn the_guard_keeps_every_tunnel_end_of_its_own_family_reachable() {
        let seen = paths(
            Path::Direct(device("wlo1")),
            Path::WireGuard(device("WHEATLEY")),
        );
        let tunnels = [
            Tunnel {
                device: device("WHEATLEY"),
                endpoints: vec![address("203.0.113.20")],
            },
            Tunnel {
                device: device("office"),
                endpoints: vec![address("198.51.100.7"), address("2001:db8:5::1")],
            },
        ];
        assert_eq!(
            wanted(&seen, &tunnels, Family::V4),
            Guard::closed([address("203.0.113.20"), address("198.51.100.7")])
        );
        assert_eq!(wanted(&seen, &tunnels, Family::V6), Guard::open());
    }

    #[test]
    fn the_rules_change_nothing_while_the_table_is_empty() {
        assert!(!Guard::absent().blocks());
        assert!(!Guard::open().blocks());
        assert!(Guard::closed([]).blocks());
        // The route without the rule that leads to it turns nothing away.
        assert!(
            !Guard::found(
                BTreeSet::from([Rule::Local]),
                BTreeSet::from([Route::Unreachable])
            )
            .blocks()
        );
    }

    #[test]
    fn the_first_run_puts_the_rules_in_and_no_route() {
        assert_eq!(
            changes(&Guard::absent(), &Guard::open()),
            [Change::AddRule(Rule::Local), Change::AddRule(Rule::Table)]
        );
    }

    #[test]
    fn closing_is_route_steps_alone_with_the_block_last() {
        let end = address("203.0.113.20");
        assert_eq!(
            changes(&Guard::open(), &Guard::closed([end])),
            [
                Change::SetRoute(Route::Throw(end)),
                Change::SetRoute(Route::Unreachable)
            ]
        );
        assert_eq!(
            changes(&Guard::open(), &Guard::closed([])),
            [Change::SetRoute(Route::Unreachable)]
        );
    }

    #[test]
    fn opening_empties_the_table_and_leaves_the_rules() {
        assert_eq!(
            changes(&Guard::closed([address("203.0.113.20")]), &Guard::open()),
            [Change::ClearRoutes]
        );
    }

    #[test]
    fn a_tunnel_end_that_moves_is_let_through_before_the_old_one_is_dropped() {
        let (old, new) = (address("203.0.113.20"), address("203.0.113.21"));
        assert_eq!(
            changes(&Guard::closed([old]), &Guard::closed([new])),
            [
                Change::SetRoute(Route::Throw(new)),
                Change::DropRoute(Route::Throw(old))
            ]
        );
    }

    #[test]
    fn clearing_takes_the_routes_out_before_the_rules() {
        assert_eq!(
            changes(&Guard::closed([address("203.0.113.20")]), &Guard::absent()),
            [
                Change::ClearRoutes,
                Change::RemoveRule(Rule::Table),
                Change::RemoveRule(Rule::Local)
            ]
        );
        assert_eq!(
            changes(&Guard::open(), &Guard::absent()),
            [
                Change::RemoveRule(Rule::Table),
                Change::RemoveRule(Rule::Local)
            ]
        );
    }

    #[test]
    fn nothing_changes_when_the_guard_is_already_as_wanted() {
        for guard in [
            Guard::absent(),
            Guard::open(),
            Guard::closed([]),
            Guard::closed([address("2001:db8::7")]),
        ] {
            assert_eq!(changes(&guard, &guard), []);
        }
    }

    #[test]
    fn every_pair_of_states_is_reached_without_cutting_anything_off() {
        let (a, b) = (address("203.0.113.20"), address("203.0.113.21"));
        let rules = |rules: &[Rule]| rules.iter().copied().collect::<BTreeSet<_>>();
        let routes = |routes: &[Route]| routes.iter().copied().collect::<BTreeSet<_>>();
        // Whole guards, no guard, and the parts a failed run or another
        // program could leave, some of them already cutting something off.
        let states = [
            Guard::absent(),
            Guard::open(),
            Guard::closed([]),
            Guard::closed([a]),
            Guard::closed([a, b]),
            Guard::found(rules(&[Rule::Local]), routes(&[])),
            Guard::found(rules(&[Rule::Table]), routes(&[])),
            Guard::found(rules(&[]), routes(&[Route::Unreachable])),
            Guard::found(
                rules(&[Rule::Local]),
                routes(&[Route::Throw(b), Route::Unreachable]),
            ),
            Guard::found(rules(&[Rule::Table]), routes(&[Route::Unreachable])),
            Guard::found(
                rules(&[Rule::Local, Rule::Table]),
                routes(&[Route::Throw(a)]),
            ),
        ];
        for from in &states {
            for to in [
                Guard::absent(),
                Guard::open(),
                Guard::closed([]),
                Guard::closed([a]),
                Guard::closed([b]),
            ] {
                assert_eq!(walk(from, &to), to, "from {from:?}");
            }
        }
    }

    #[test]
    fn a_step_is_in_place_when_the_guard_shows_what_it_was_for() {
        let end = address("203.0.113.20");
        let closed = Guard::closed([end]);
        assert!(closed.has(&Change::AddRule(Rule::Table)));
        assert!(closed.has(&Change::SetRoute(Route::Throw(end))));
        assert!(!closed.has(&Change::DropRoute(Route::Unreachable)));
        assert!(!closed.has(&Change::ClearRoutes));
        assert!(Guard::open().has(&Change::ClearRoutes));
        assert!(Guard::absent().has(&Change::RemoveRule(Rule::Local)));
        assert!(!Guard::absent().has(&Change::AddRule(Rule::Local)));
    }

    #[test]
    fn a_guard_reads_as_a_line_for_the_journal() {
        assert_eq!(Guard::absent().to_string(), "rules: none; routes: none");
        assert_eq!(
            Guard::closed([address("203.0.113.20")]).to_string(),
            "rules: main without its default, the guard's table; routes: throw 203.0.113.20, unreachable default"
        );
    }
}
