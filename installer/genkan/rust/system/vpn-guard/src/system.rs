// SPDX-License-Identifier: GPL-3.0-or-later
//! Looking at the system, deciding and changing it, apart from how the
//! system is reached.
//!
//! Whether a change took is never read from what the command that made it
//! said. A step that reports failure is checked against the system: if what
//! it was for is in place, something else got there first and the run goes
//! on. When the steps are done the guard is read once more, and the run has
//! worked only if that is the guard that was wanted.

use std::fmt;

use crate::family::{Family, PerFamily};
use crate::guard::{self, Change, Device, Guard, Path, Tunnel, Verdict};

/// The parts of the system the guard reads and changes.
pub trait System {
    type Error;

    /// Where the family's internet traffic goes.
    fn path(&mut self, family: Family) -> Result<Path, Self::Error>;

    /// Every WireGuard device with its peers' far ends. Reading those takes
    /// privilege, so this is asked only when a family is to be closed.
    fn tunnels(&mut self) -> Result<Vec<Tunnel>, Self::Error>;

    /// What is installed of the guard in the family.
    fn guard(&mut self, family: Family) -> Result<Guard, Self::Error>;

    /// Makes one change. An error says only that the attempt did not report
    /// success: the caller looks at the system to learn what is true.
    fn apply(&mut self, family: Family, change: &Change) -> Result<(), Self::Error>;
}

/// Why a family's guard could not be brought to what was wanted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure<E> {
    /// The guard could not be read.
    Unread(E),
    /// A step failed, and what it was for is not in place.
    Step { change: Change, error: E },
    /// Every step ran, and the guard found afterwards is not the one wanted.
    NotAsWanted { wanted: Guard, found: Guard },
}

impl<E: fmt::Display> fmt::Display for Failure<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Unread(error) => write!(f, "could not read the guard: {error}"),
            Failure::Step { change, error } => write!(f, "{change:?} did not take: {error}"),
            Failure::NotAsWanted { wanted, found } => {
                write!(
                    f,
                    "the guard is not as wanted. Wanted {wanted}. Found {found}."
                )
            }
        }
    }
}

/// What became of one family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub family: Family,
    pub before: Guard,
    pub after: Guard,
    /// The tunnel that carries the other family alone, for a closed family.
    pub carried_by: Option<Device>,
}

impl Outcome {
    /// A line for the journal when the family was closed or opened, and
    /// nothing when it stayed as it was.
    pub fn news(&self) -> Option<String> {
        let name = self.family.name();
        let other = self.family.other().name();
        match (self.before.blocks(), self.after.blocks(), &self.carried_by) {
            (false, true, Some(tunnel)) => Some(format!(
                "{name} is blocked while {tunnel} is connected: {tunnel} carries only {other}, so {name} would leave outside it."
            )),
            (false, true, None) => Some(format!("{name} is blocked.")),
            (true, false, _) => Some(format!("{name} is no longer blocked.")),
            (true, true, _) | (false, false, _) => None,
        }
    }
}

/// Each family's outcome, or why its guard is not as wanted.
pub type Outcomes<E> = PerFamily<Result<Outcome, Failure<E>>>;

/// Brings the guard in both families to what the system needs now. A failure
/// to look at where traffic goes stops everything; a failure in one family
/// does not keep the other from being changed.
pub fn reconcile<S: System>(system: &mut S) -> Result<Outcomes<S::Error>, S::Error> {
    let paths = PerFamily::try_from_fn(|family| system.path(family))?;
    let closing = Family::BOTH
        .into_iter()
        .any(|family| matches!(guard::verdict(&paths, family), Verdict::Close { .. }));
    let tunnels = if closing {
        system.tunnels()?
    } else {
        Vec::new()
    };
    Ok(both(|family| {
        let carried_by = match guard::verdict(&paths, family) {
            Verdict::Close { carried_by } => Some(carried_by.clone()),
            Verdict::Open => None,
        };
        settle(
            system,
            family,
            guard::wanted(&paths, &tunnels, family),
            carried_by,
        )
    }))
}

/// Removes the guard from both families, rules and all.
pub fn clear<S: System>(system: &mut S) -> Outcomes<S::Error> {
    both(|family| settle(system, family, Guard::absent(), None))
}

fn both<T>(mut each: impl FnMut(Family) -> T) -> PerFamily<T> {
    PerFamily {
        v4: each(Family::V4),
        v6: each(Family::V6),
    }
}

fn settle<S: System>(
    system: &mut S,
    family: Family,
    wanted: Guard,
    carried_by: Option<Device>,
) -> Result<Outcome, Failure<S::Error>> {
    let before = system.guard(family).map_err(Failure::Unread)?;
    for change in guard::changes(&before, &wanted) {
        if let Err(error) = system.apply(family, &change) {
            let now = system.guard(family).map_err(Failure::Unread)?;
            if !now.has(&change) {
                return Err(Failure::Step { change, error });
            }
        }
    }
    let found = system.guard(family).map_err(Failure::Unread)?;
    if found != wanted {
        return Err(Failure::NotAsWanted { wanted, found });
    }
    Ok(Outcome {
        family,
        before,
        after: found,
        carried_by,
    })
}

/// A sentence for each family on where its traffic goes and why. Changes
/// nothing and needs no privilege.
pub fn status<S: System>(system: &mut S) -> Result<Vec<String>, S::Error> {
    const CATCH_UP: &str =
        "at the next network change, or when root runs `sekka-vpn-guard reconcile`";
    let paths = PerFamily::try_from_fn(|family| system.path(family))?;
    let mut lines = Vec::new();
    for family in Family::BOTH {
        let name = family.name();
        let other = family.other().name();
        let blocked = system.guard(family)?.blocks();
        lines.push(match (paths.get(family), guard::verdict(&paths, family), blocked) {
            (_, Verdict::Close { carried_by }, true) => format!(
                "{name} is blocked: {carried_by} carries only {other}, so {name} would leave outside it."
            ),
            (_, Verdict::Close { carried_by }, false) => format!(
                "{name} is not blocked yet, though {carried_by} carries only {other}. The guard catches up {CATCH_UP}."
            ),
            (Path::WireGuard(device) | Path::OtherTunnel(device), Verdict::Open, _) => {
                format!("{name} goes through {device}.")
            }
            (Path::Direct(device), Verdict::Open, _) => format!("{name} leaves by {device}."),
            (Path::Nowhere, Verdict::Open, true) => {
                format!("{name} is blocked by a guard that is no longer needed. It is lifted {CATCH_UP}.")
            }
            (Path::Nowhere, Verdict::Open, false) => format!("{name} has no route to the internet."),
        });
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::guard::{Route, Rule};

    /// What a fake system does with one particular step.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quirk {
        /// Makes the change and reports failure, as a command does when
        /// something else made the same change first.
        DoneButSaysNo,
        /// Reports failure and changes nothing.
        Refuses,
        /// Reports success and changes nothing.
        SaysYesAndDoesNothing,
    }

    /// A system in memory. As in the kernel, a family the guard blocks goes
    /// nowhere unless a tunnel carries it, because tunnels' rules come first.
    struct Fake {
        without_guard: PerFamily<Path>,
        tunnels: Vec<Tunnel>,
        guards: PerFamily<Guard>,
        applied: Vec<(Family, Change)>,
        asked_for_tunnels: usize,
        quirk: Option<(Family, Change, Quirk)>,
    }

    impl Fake {
        fn new(v4: Path, v6: Path, tunnels: Vec<Tunnel>) -> Fake {
            Fake {
                without_guard: PerFamily { v4, v6 },
                tunnels,
                guards: PerFamily::default(),
                applied: Vec::new(),
                asked_for_tunnels: 0,
                quirk: None,
            }
        }

        /// The system after a first run has put the rules in.
        fn settled(v4: Path, v6: Path, tunnels: Vec<Tunnel>) -> Fake {
            let mut fake = Fake::new(v4, v6, tunnels);
            fake.guards = PerFamily {
                v4: Guard::open(),
                v6: Guard::open(),
            };
            fake
        }

        fn steps(&self, family: Family) -> Vec<Change> {
            self.applied
                .iter()
                .filter(|(of, _)| *of == family)
                .map(|(_, change)| *change)
                .collect()
        }
    }

    impl System for Fake {
        type Error = String;

        fn path(&mut self, family: Family) -> Result<Path, String> {
            let path = self.without_guard.get(family).clone();
            Ok(match path {
                Path::Direct(_) if self.guards.get(family).blocks() => Path::Nowhere,
                other => other,
            })
        }

        fn tunnels(&mut self) -> Result<Vec<Tunnel>, String> {
            self.asked_for_tunnels += 1;
            Ok(self.tunnels.clone())
        }

        fn guard(&mut self, family: Family) -> Result<Guard, String> {
            Ok(self.guards.get(family).clone())
        }

        fn apply(&mut self, family: Family, change: &Change) -> Result<(), String> {
            let quirk = match self.quirk {
                Some((of, step, quirk)) if of == family && step == *change => Some(quirk),
                _ => None,
            };
            if quirk != Some(Quirk::Refuses) && quirk != Some(Quirk::SaysYesAndDoesNothing) {
                let guard = match family {
                    Family::V4 => &mut self.guards.v4,
                    Family::V6 => &mut self.guards.v6,
                };
                *guard = guard.after(change);
            }
            self.applied.push((family, *change));
            match quirk {
                Some(Quirk::DoneButSaysNo) => Err("File exists".to_owned()),
                Some(Quirk::Refuses) => Err("Operation not permitted".to_owned()),
                Some(Quirk::SaysYesAndDoesNothing) | None => Ok(()),
            }
        }
    }

    fn device(name: &str) -> Device {
        Device(name.to_owned())
    }

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn wlo1() -> Path {
        Path::Direct(device("wlo1"))
    }

    fn glados() -> Fake {
        Fake::settled(
            Path::WireGuard(device("GLADOS")),
            wlo1(),
            vec![Tunnel {
                device: device("GLADOS"),
                endpoints: vec![address("203.0.113.20")],
            }],
        )
    }

    fn news(outcomes: &Outcomes<String>) -> Vec<String> {
        Family::BOTH
            .into_iter()
            .filter_map(|family| outcomes.get(family).as_ref().unwrap().news())
            .collect()
    }

    #[test]
    fn the_first_run_puts_the_rules_in_and_blocks_nothing() {
        let mut system = Fake::new(wlo1(), wlo1(), Vec::new());
        let outcomes = reconcile(&mut system).unwrap();
        assert_eq!(news(&outcomes), [] as [String; 0]);
        for family in Family::BOTH {
            assert_eq!(
                system.steps(family),
                [Change::AddRule(Rule::Local), Change::AddRule(Rule::Table)]
            );
            assert_eq!(*system.guards.get(family), Guard::open());
        }
        assert_eq!(
            system.asked_for_tunnels, 0,
            "tunnel ends were read with nothing to close"
        );
        assert_eq!(system.path(Family::V6), Ok(wlo1()));

        let steps = system.applied.len();
        reconcile(&mut system).unwrap();
        assert_eq!(
            system.applied.len(),
            steps,
            "a second run changed something"
        );
    }

    #[test]
    fn an_ipv4_only_tunnel_gets_ipv6_closed_by_one_route() {
        let mut system = glados();
        let first = reconcile(&mut system).unwrap();
        assert_eq!(
            news(&first),
            [
                "IPv6 is blocked while GLADOS is connected: GLADOS carries only IPv4, so IPv6 would leave outside it."
            ]
        );
        assert_eq!(system.steps(Family::V4), []);
        // The tunnel's far end is an IPv4 address, so IPv6 needs no way round for it.
        assert_eq!(
            system.steps(Family::V6),
            [Change::SetRoute(Route::Unreachable)]
        );
        assert_eq!(system.guards.v6, Guard::closed([]));

        let second = reconcile(&mut system).unwrap();
        assert_eq!(news(&second), [] as [String; 0]);
        assert_eq!(system.applied.len(), 1, "a second run changed something");
    }

    #[test]
    fn the_guard_is_lifted_when_the_tunnel_goes() {
        let mut system = glados();
        reconcile(&mut system).unwrap();
        system.without_guard.v4 = wlo1();
        let outcomes = reconcile(&mut system).unwrap();
        assert_eq!(news(&outcomes), ["IPv6 is no longer blocked."]);
        assert_eq!(system.steps(Family::V6).last(), Some(&Change::ClearRoutes));
        assert_eq!(system.guards.v6, Guard::open());
    }

    #[test]
    fn the_guard_is_lifted_when_a_tunnel_starts_carrying_the_family() {
        let mut system = glados();
        reconcile(&mut system).unwrap();
        system.without_guard.v6 = Path::WireGuard(device("GLADOS"));
        reconcile(&mut system).unwrap();
        assert_eq!(system.guards.v6, Guard::open());
    }

    #[test]
    fn an_ipv6_only_tunnel_keeps_its_ipv4_far_end_reachable() {
        let end = address("203.0.113.20");
        let mut system = Fake::settled(
            wlo1(),
            Path::WireGuard(device("WHEATLEY")),
            vec![Tunnel {
                device: device("WHEATLEY"),
                endpoints: vec![end],
            }],
        );
        reconcile(&mut system).unwrap();
        assert_eq!(
            system.steps(Family::V4),
            [
                Change::SetRoute(Route::Throw(end)),
                Change::SetRoute(Route::Unreachable)
            ]
        );
        assert_eq!(system.guards.v4, Guard::closed([end]));
    }

    #[test]
    fn a_step_that_says_no_is_judged_by_what_is_in_place() {
        // Another run put the rule in between this run's look and its step.
        let mut system = Fake::new(Path::WireGuard(device("GLADOS")), wlo1(), Vec::new());
        system.quirk = Some((
            Family::V6,
            Change::AddRule(Rule::Local),
            Quirk::DoneButSaysNo,
        ));
        let outcomes = reconcile(&mut system).unwrap();
        assert!(outcomes.v6.is_ok(), "{:?}", outcomes.v6);
        assert_eq!(system.guards.v6, Guard::closed([]));
    }

    #[test]
    fn a_step_that_fails_stops_its_family_and_not_the_other() {
        // A guard left in IPv4 by an earlier run, to be lifted, and IPv6 to be closed.
        let mut system = glados();
        system.guards.v4 = Guard::closed([]);
        system.quirk = Some((Family::V4, Change::ClearRoutes, Quirk::Refuses));
        let outcomes = reconcile(&mut system).unwrap();
        assert_eq!(
            outcomes.v4,
            Err(Failure::Step {
                change: Change::ClearRoutes,
                error: "Operation not permitted".to_owned()
            })
        );
        assert!(outcomes.v6.is_ok());
        assert_eq!(system.guards.v6, Guard::closed([]));
    }

    #[test]
    fn a_step_that_fails_stops_before_the_block_goes_in() {
        // The way round for the tunnel's far end could not be set, so the
        // unreachable default must not follow it.
        let end = address("203.0.113.20");
        let mut system = Fake::settled(
            wlo1(),
            Path::WireGuard(device("WHEATLEY")),
            vec![Tunnel {
                device: device("WHEATLEY"),
                endpoints: vec![end],
            }],
        );
        system.quirk = Some((
            Family::V4,
            Change::SetRoute(Route::Throw(end)),
            Quirk::Refuses,
        ));
        let outcomes = reconcile(&mut system).unwrap();
        assert!(
            matches!(outcomes.v4, Err(Failure::Step { .. })),
            "{:?}",
            outcomes.v4
        );
        assert_eq!(
            system.steps(Family::V4),
            [Change::SetRoute(Route::Throw(end))]
        );
        assert!(!system.guards.v4.blocks());
    }

    #[test]
    fn a_run_whose_steps_all_say_yes_still_fails_when_the_guard_is_not_as_wanted() {
        let mut system = glados();
        system.quirk = Some((
            Family::V6,
            Change::SetRoute(Route::Unreachable),
            Quirk::SaysYesAndDoesNothing,
        ));
        let outcomes = reconcile(&mut system).unwrap();
        assert_eq!(
            outcomes.v6,
            Err(Failure::NotAsWanted {
                wanted: Guard::closed([]),
                found: Guard::open()
            })
        );
        assert_eq!(
            outcomes.v6.unwrap_err().to_string(),
            "the guard is not as wanted. Wanted rules: main without its default, the guard's table; routes: unreachable default. \
             Found rules: main without its default, the guard's table; routes: none."
        );
    }

    #[test]
    fn clearing_removes_the_routes_and_the_rules() {
        let mut system = glados();
        reconcile(&mut system).unwrap();
        let outcomes = clear(&mut system);
        assert_eq!(news(&outcomes), ["IPv6 is no longer blocked."]);
        for family in Family::BOTH {
            assert_eq!(*system.guards.get(family), Guard::absent());
        }
    }

    #[test]
    fn status_says_where_each_family_goes() {
        let mut system = glados();
        assert_eq!(
            status(&mut system).unwrap(),
            [
                "IPv4 goes through GLADOS.",
                "IPv6 is not blocked yet, though GLADOS carries only IPv4. The guard catches up at the next network change, or when root runs `sekka-vpn-guard reconcile`.",
            ]
        );
        reconcile(&mut system).unwrap();
        assert_eq!(
            status(&mut system).unwrap(),
            [
                "IPv4 goes through GLADOS.",
                "IPv6 is blocked: GLADOS carries only IPv4, so IPv6 would leave outside it.",
            ]
        );
        assert_eq!(
            system.asked_for_tunnels, 1,
            "status read the tunnels' far ends"
        );

        let mut plain = Fake::new(wlo1(), Path::Nowhere, Vec::new());
        assert_eq!(
            status(&mut plain).unwrap(),
            ["IPv4 leaves by wlo1.", "IPv6 has no route to the internet."]
        );
    }
}
