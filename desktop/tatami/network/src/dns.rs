// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keep DNS policy when IPv6 is switched off. NetworkManager rejects IPv6
//! DNS addresses with method=disabled, so suspend them in the same profile.
use crate::model::Settings;
use std::net::{Ipv4Addr, Ipv6Addr};

pub const BACKUP_KEY: &str = "org.sekka.tatami.ipv6-dns";
type Changes = Vec<(&'static str, String)>;

fn flag(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

impl Settings {
    pub fn ipv6_off(&self) -> bool {
        matches!(self.ipv6_method.as_str(), "disabled" | "ignore")
    }

    /// Stale data must not override an external edit to the current policy.
    /// Unknown, malformed or stale data is an error, never restored as policy.
    pub fn suspended_dns(&self) -> Result<Option<Vec<String>>, String> {
        let Some(raw) = &self.suspended_dns else {
            return Ok(None);
        };
        let fields: Vec<_> = raw.split('|').collect();
        let invalid = || {
            "Saved IPv6 DNS could not be read. Select Automatic DNS or apply Custom servers before turning IPv6 on.".to_owned()
        };
        if fields.len() != 5
            || fields[0] != "v1"
            || !["yes", "no"].contains(&fields[1])
            || !["yes", "no"].contains(&fields[3])
        {
            return Err(invalid());
        }
        let v4: Vec<_> = fields[2].split_whitespace().collect();
        let v6: Vec<_> = fields[4].split_whitespace().collect();
        if v4.iter().any(|s| s.parse::<Ipv4Addr>().is_err())
            || v6.iter().any(|s| s.parse::<Ipv6Addr>().is_err())
        {
            return Err(invalid());
        }
        if v4 != self.ipv4_dns
            || fields[1] != flag(self.ipv4_ignore_auto_dns)
            || fields[3] != flag(self.ipv6_ignore_auto_dns)
        {
            return Err("DNS changed outside Network. Select Automatic DNS or apply Custom servers before turning IPv6 on.".into());
        }
        Ok(Some(v6.into_iter().map(str::to_owned).collect()))
    }

    fn backup(v4: &[String], manual4: bool, v6: &[String], manual6: bool) -> String {
        format!(
            "{BACKUP_KEY}=v1|{}|{}|{}|{}",
            flag(manual4),
            v4.join(" "),
            flag(manual6),
            v6.join(" ")
        )
    }

    /// One profile modification includes policy and its suspended addresses.
    /// Dictionary additions/removals leave other applications' keys intact.
    pub fn dns_changes(&self, v4: Vec<String>, v6: Vec<String>) -> Changes {
        let manual = !v4.is_empty() || !v6.is_empty();
        let mut changes = vec![
            ("ipv4.dns", v4.join(",")),
            ("ipv4.ignore-auto-dns", flag(manual).into()),
            (
                "ipv6.dns",
                if self.ipv6_off() {
                    String::new()
                } else {
                    v6.join(",")
                },
            ),
            ("ipv6.ignore-auto-dns", flag(manual).into()),
        ];
        if self.ipv6_off() && manual {
            changes.push(("+user.data", Self::backup(&v4, manual, &v6, manual)));
        } else if self.suspended_dns.is_some() {
            changes.push(("-user.data", BACKUP_KEY.into()));
        }
        changes
    }

    pub fn ipv6_changes(&self, method: &str) -> Result<Changes, String> {
        let mut changes = vec![("ipv6.method", method.to_owned())];
        if method == "disabled" && self.ipv6_method != "disabled" {
            // External editors can use NM DNS formats beyond this window's
            // plain IP addresses. Refuse before clearing an unrestorable value.
            if self.ipv4_dns.iter().any(|s| s.parse::<Ipv4Addr>().is_err())
                || self.ipv6_dns.iter().any(|s| s.parse::<Ipv6Addr>().is_err())
            {
                return Err("These DNS settings cannot be saved while IPv6 is off. Select Automatic DNS or apply Custom IP addresses first.".into());
            }
            changes.push((
                "+user.data",
                Self::backup(
                    &self.ipv4_dns,
                    self.ipv4_ignore_auto_dns,
                    &self.ipv6_dns,
                    self.ipv6_ignore_auto_dns,
                ),
            ));
            changes.push(("ipv6.dns", String::new()));
        } else if method == "auto" && self.ipv6_off() {
            let saved = self.suspended_dns()?;
            // Legacy Off profiles have no backup. Keep current explicit
            // servers and the current v4 policy;
            // do not silently add network DNS or invent missing custom servers.
            let manual = if saved.is_some() {
                self.ipv6_ignore_auto_dns
            } else {
                self.ipv4_ignore_auto_dns || self.ipv6_ignore_auto_dns
            };
            changes.push((
                "ipv6.dns",
                saved.unwrap_or_else(|| self.ipv6_dns.clone()).join(","),
            ));
            changes.push(("ipv6.ignore-auto-dns", flag(manual).into()));
            if self.suspended_dns.is_some() {
                changes.push(("-user.data", BACKUP_KEY.into()));
            }
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }

    fn suspended(v4: &[&str], v6: &[&str]) -> Settings {
        let v4 = words(v4);
        let v6 = words(v6);
        let manual = !v4.is_empty() || !v6.is_empty();
        Settings {
            ipv6_method: "disabled".into(),
            suspended_dns: Some(
                Settings::backup(&v4, manual, &v6, manual)
                    .split_once('=')
                    .unwrap()
                    .1
                    .into(),
            ),
            ipv4_dns: v4,
            ipv4_ignore_auto_dns: manual,
            ipv6_ignore_auto_dns: manual,
            ..Settings::default()
        }
    }

    #[test]
    fn off_auto_preserves_exact_dns_choices() {
        for (v4, v6) in [
            (vec![], vec![]),
            (
                vec!["1.1.1.1", "1.0.0.1"],
                vec!["2606:4700:4700::1111", "2606:4700:4700::1001"],
            ),
            (
                vec!["8.8.8.8", "8.8.4.4"],
                vec!["2001:4860:4860::8888", "2001:4860:4860::8844"],
            ),
            (vec!["9.9.9.9"], vec!["2620:fe::fe"]),
            (vec!["9.9.9.9"], vec![]),
            (vec![], vec!["2620:fe::fe"]),
        ] {
            let s = suspended(&v4, &v6);
            let changes = s.ipv6_changes("auto").unwrap();
            assert!(changes.contains(&("ipv6.dns", v6.join(","))));
            assert!(changes.contains(&(
                "ipv6.ignore-auto-dns",
                flag(!v4.is_empty() || !v6.is_empty()).into()
            )));
            assert!(!changes.iter().any(|(key, _)| key.starts_with("ipv4.")));
            assert!(changes.contains(&("-user.data", BACKUP_KEY.into())));
        }
    }

    #[test]
    fn dns_edit_while_off_replaces_or_clears_suspended_choices() {
        let s = suspended(&["1.1.1.1"], &["2606:4700:4700::1111"]);
        let changes = s.dns_changes(words(&["9.9.9.9"]), words(&["2620:fe::fe"]));
        assert!(changes.contains(&("ipv6.dns", String::new())));
        assert!(changes.contains(&(
            "+user.data",
            "org.sekka.tatami.ipv6-dns=v1|yes|9.9.9.9|yes|2620:fe::fe".into()
        )));
        let reset = s.dns_changes(vec![], vec![]);
        assert!(reset.contains(&("-user.data", BACKUP_KEY.into())));
        assert!(reset.contains(&("ipv6.ignore-auto-dns", "no".into())));
    }

    #[test]
    fn malformed_and_stale_backups_cannot_override_current_policy() {
        for value in [
            "bad",
            "v2|yes|9.9.9.9|yes|2620:fe::fe",
            "v1|yes|9.9.9.9|yes|not-an-ip",
            "v1|yes|2620:fe::fe|yes|9.9.9.9",
        ] {
            let mut s = suspended(&["9.9.9.9"], &["2620:fe::fe"]);
            s.suspended_dns = Some(value.into());
            assert!(s.ipv6_changes("auto").is_err());
        }
        let mut s = suspended(&["9.9.9.9"], &["2620:fe::fe"]);
        s.ipv4_dns = words(&["8.8.8.8"]);
        assert!(s.ipv6_changes("auto").is_err());
        let mut s = suspended(&["9.9.9.9"], &["2620:fe::fe"]);
        s.ipv6_ignore_auto_dns = false;
        assert!(s.ipv6_changes("auto").is_err());
    }

    #[test]
    fn legacy_off_policy_does_not_add_network_dns() {
        let s = Settings {
            ipv6_method: "disabled".into(),
            ipv4_ignore_auto_dns: true,
            ipv4_dns: words(&["9.9.9.9"]),
            ..Settings::default()
        };
        let changes = s.ipv6_changes("auto").unwrap();
        assert!(changes.contains(&("ipv6.dns", String::new())));
        assert!(changes.contains(&("ipv6.ignore-auto-dns", "yes".into())));
        let s = Settings {
            ipv6_method: "disabled".into(),
            ipv6_ignore_auto_dns: true,
            ..Settings::default()
        };
        assert!(
            s.ipv6_changes("auto")
                .unwrap()
                .contains(&("ipv6.ignore-auto-dns", "yes".into()))
        );
    }

    #[test]
    fn off_refuses_unrestorable_external_dns_before_clearing_it() {
        let s = Settings {
            ipv6_method: "auto".into(),
            ipv6_dns: words(&["dns+tls://[2620:fe::fe]:853"]),
            ..Settings::default()
        };
        assert!(s.ipv6_changes("disabled").is_err());
    }
}
