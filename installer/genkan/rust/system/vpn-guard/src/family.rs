// SPDX-License-Identifier: GPL-3.0-or-later
use std::net::IpAddr;

use ipnet::IpNet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub const BOTH: [Self; 2] = [Self::V4, Self::V6];

    pub fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(_) => Self::V4,
            IpAddr::V6(_) => Self::V6,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::V4 => Self::V6,
            Self::V6 => Self::V4,
        }
    }

    /// Destinations that are not the public internet: private, shared,
    /// loopback, link-local, multicast and reserved space. For IPv6 that is
    /// everything outside global unicast, 2000::/3.
    pub fn not_public(self) -> impl Iterator<Item = IpNet> {
        let nets: &[&str] = match self {
            Self::V4 => &[
                "0.0.0.0/8",
                "10.0.0.0/8",
                "100.64.0.0/10",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "224.0.0.0/3",
            ],
            Self::V6 => &["::/3", "4000::/2", "8000::/1"],
        };
        nets.iter()
            .map(|net| net.parse().expect("a constant prefix"))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::V4 => "IPv4",
            Self::V6 => "IPv6",
        }
    }
}
