// SPDX-License-Identifier: GPL-3.0-or-later
//! The two address families, and a pair of values with one for each.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One of the two address families a tunnel can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub const BOTH: [Family; 2] = [Family::V4, Family::V6];

    pub fn of(address: IpAddr) -> Family {
        match address {
            IpAddr::V4(_) => Family::V4,
            IpAddr::V6(_) => Family::V6,
        }
    }

    pub fn other(self) -> Family {
        match self {
            Family::V4 => Family::V6,
            Family::V6 => Family::V4,
        }
    }

    /// The name people know it by.
    pub fn name(self) -> &'static str {
        match self {
            Family::V4 => "IPv4",
            Family::V6 => "IPv6",
        }
    }

    /// iproute2's flag for it.
    pub fn flag(self) -> &'static str {
        match self {
            Family::V4 => "-4",
            Family::V6 => "-6",
        }
    }

    /// An address that stands for the internet at large. These are
    /// documentation addresses (RFC 5737, RFC 3849): no network is reached
    /// at them and no configuration routes them on purpose, so only a
    /// default route, or the two halves that stand in for one, carries a
    /// packet sent to them.
    pub fn internet(self) -> IpAddr {
        match self {
            Family::V4 => IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            Family::V6 => IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
        }
    }

    /// The prefix length of a single address.
    pub fn host_prefix(self) -> u8 {
        match self {
            Family::V4 => 32,
            Family::V6 => 128,
        }
    }
}

/// One value for each family.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PerFamily<T> {
    pub v4: T,
    pub v6: T,
}

impl<T> PerFamily<T> {
    pub fn get(&self, family: Family) -> &T {
        match family {
            Family::V4 => &self.v4,
            Family::V6 => &self.v6,
        }
    }

    /// Builds both values, IPv4's first, stopping at the first failure.
    pub fn try_from_fn<E>(mut make: impl FnMut(Family) -> Result<T, E>) -> Result<Self, E> {
        Ok(PerFamily {
            v4: make(Family::V4)?,
            v6: make(Family::V6)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_family_has_the_other_and_its_own_stand_in() {
        for family in Family::BOTH {
            assert_ne!(family, family.other());
            assert_eq!(family, family.other().other());
            assert_eq!(family, Family::of(family.internet()));
        }
    }

    #[test]
    fn a_pair_answers_for_the_family_asked() {
        let pair = PerFamily::try_from_fn(|family| Ok::<_, ()>(family.name())).unwrap();
        assert_eq!(*pair.get(Family::V4), "IPv4");
        assert_eq!(*pair.get(Family::V6), "IPv6");
    }
}
