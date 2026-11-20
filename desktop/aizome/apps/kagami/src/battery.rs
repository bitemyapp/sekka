//! Unprivileged adapters for the opt-in hardware test battery.
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

/// One connection attempt, restricted to the explicitly selected AWDL receiver.
pub struct Autotest {
    target: String,
    device_id: String,
    attempted: bool,
}

impl Autotest {
    pub fn from_env() -> Option<Self> {
        if std::env::var("SEKKA_AIRPLAY_AUTOTEST").ok().as_deref() != Some("1")
            || std::env::var_os("SEKKA_AIRPLAY_RADIO_RELAY").is_none()
        {
            return None;
        }
        let media = std::env::var("SEKKA_AIRPLAY_TEST_MEDIA_PROFILE").ok()?;
        if !matches!(media.as_str(), "synthetic-low" | "synthetic-video" | "synthetic-av") {
            return None;
        }
        let target = std::env::var("SEKKA_AIRPLAY_TEST_TARGET").ok()?;
        let device_id = std::env::var("SEKKA_AIRPLAY_TEST_DEVICE_ID").ok()?;
        if target.is_empty() || device_id.is_empty() {
            return None;
        }
        Some(Self { target, device_id, attempted: false })
    }

    pub fn next(&mut self, devices: &[crate::backend::Device], ready: bool) -> Option<crate::backend::Device> {
        if !ready || self.attempted {
            return None;
        }
        let matches: Vec<_> = devices.iter().filter(|d| d.name == self.target).collect();
        if matches.len() != 1 {
            return None;
        }
        let device = matches[0];
        if !device.device_id.eq_ignore_ascii_case(&self.device_id) || device.port == 0 {
            return None;
        }
        let host = device.ip.strip_suffix("%awdl0")?.parse::<std::net::Ipv6Addr>().ok()?;
        if !host.is_unicast_link_local() {
            return None;
        }
        self.attempted = true;
        Some(device.clone())
    }
}

pub fn relay(path: &std::path::Path) -> std::io::Result<()> {
    let mut socket = UnixStream::connect(path)?;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: valid socket, correctly sized writable ucred and length.
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // This is a user-owned relay, never a replacement root service.
    if credentials.uid != unsafe { libc::geteuid() } {
        return Err(std::io::Error::other("The test relay belongs to another user"));
    }
    let control = socket.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::stdin().read(&mut [0]);
        let _ = control.shutdown(std::net::Shutdown::Both);
    });
    std::io::copy(&mut socket, &mut std::io::stdout().lock())?;
    std::io::stdout().flush()
}

pub fn media_args(profile: &str) -> Result<&'static [&'static str], String> {
    match profile {
        "screen-auto" => Ok(&[]),
        "screen-low" => Ok(&["-fps", "15", "-bitrate", "500"]),
        "screen-noaudio" => Ok(&["-no-audio"]),
        "screen-software" => Ok(&["-hwaccel", "none"]),
        "screen-latency" => Ok(&["-target-latency-ms", "1000"]),
        "screen-hevc" => Ok(&["-video-codec", "hevc"]),
        "synthetic-low" => Ok(&["-test", "-no-audio", "-fps", "15", "-bitrate", "500"]),
        "synthetic-video" => Ok(&["-test", "-no-audio", "-bitrate", "1843"]),
        "synthetic-av" => Ok(&["-test", "-bitrate", "1843"]),
        _ => Err(format!("Unknown media test profile: {profile}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_media_options_are_accepted() {
        assert!(media_args("screen-auto").unwrap().is_empty());
        assert!(media_args("synthetic-low").unwrap().contains(&"-test"));
        assert!(media_args("-no-encrypt").is_err());
        assert!(media_args("../command").is_err());
    }

    #[test]
    fn autonomous_selection_requires_unique_name_identity_scope_and_readiness() {
        let mut test = Autotest { target: "Test TV".into(), device_id: "aa:bb".into(), attempted: false };
        let tv = crate::backend::Device {
            name: "Test TV".into(),
            device_id: "AA:BB".into(),
            ip: "fe80::123%awdl0".into(),
            port: 7000,
            ..Default::default()
        };
        assert!(test.next(&[tv.clone()], false).is_none());
        assert!(test.next(&[tv.clone(), tv.clone()], true).is_none());
        for wrong in [
            crate::backend::Device { device_id: "cc:dd".into(), ..tv.clone() },
            crate::backend::Device { ip: "fe80::123%wlan0".into(), ..tv.clone() },
            crate::backend::Device { ip: "::1%awdl0".into(), ..tv.clone() },
        ] {
            assert!(test.next(&[wrong], true).is_none());
        }
        assert_eq!(test.next(&[tv.clone()], true), Some(tv.clone()));
        assert!(test.next(&[tv], true).is_none()); // never retry a failed PIN automatically
    }
}
