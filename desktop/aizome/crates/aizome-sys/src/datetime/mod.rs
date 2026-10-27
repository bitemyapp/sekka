//! Date, time, locale and the machine's names, through systemd's
//! timedated, localed and hostnamed, plus the time zone list ([`zones()`]).
//!
//! - [`TimeDate`]: the time zone (set it), network time on or off, the
//!   clock (set it), and whether the hardware clock keeps local time.
//! - [`Locale`]: the system locale (`LANG=…`) and the X11 keyboard
//!   (layout, model, variant, options), as display managers use them.
//! - [`Hostname`]: the hostname, the pretty name people see, the chassis
//!   and icon, the OS and hardware names.
//!
//! # Settings NixOS pins
//!
//! On NixOS a setting given in the system configuration is read-only:
//! nixpkgs' systemd reads and writes these files at paths it is given in
//! the daemons' environment (`SYSTEMD_ETC_LOCALTIME`,
//! `SYSTEMD_ETC_LOCALE_CONF`, `SYSTEMD_ETC_HOSTNAME`,
//! `SYSTEMD_ETC_MACHINE_INFO`…), and those under `/etc/static` live in the
//! Nix store. Each snapshot says which settings are pinned
//! ([`TimeDateState::timezone_managed`] and the like), read from the
//! daemons' unit environment through systemd, so the UI can show "managed
//! by the system configuration" instead of offering a change that would
//! fail; a change that fails anyway for the same reason returns
//! [`crate::Error::ManagedBySystem`].
//!
//! For Settings to change them, the NixOS configuration leaves them unset:
//! `time.timeZone = null` (the time zone), no `i18n.defaultLocale` override
//! beyond the default and `console.keyMap` for the keyboard, and the
//! pretty hostname needs `/etc/machine-info` to be writable (point
//! `SYSTEMD_ETC_MACHINE_INFO` of `systemd-hostnamed` at `/etc/machine-info`).
//! The static hostname is `networking.hostName` and stays pinned.

pub mod zones;

pub use zones::{Zone, by_region, nearest, zones, zones_in};

use crate::bus::{self, Bus, Opts, Props, PropsExt};
use crate::error::Error;
use crate::service::{Model, Stop, Task, Wake, service_handle};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zbus::zvariant::OwnedObjectPath;

const TIMEDATE: &str = "org.freedesktop.timedate1";
const TIMEDATE_PATH: &str = "/org/freedesktop/timedate1";
const LOCALE: &str = "org.freedesktop.locale1";
const LOCALE_PATH: &str = "/org/freedesktop/locale1";
const HOSTNAME: &str = "org.freedesktop.hostname1";
const HOSTNAME_PATH: &str = "/org/freedesktop/hostname1";

/// A unit's environment (`Environment=`), read from systemd.
pub(crate) fn unit_environment(bus: &Bus, unit: &str) -> HashMap<String, String> {
    let Ok(path) = bus.call::<_, OwnedObjectPath>(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "LoadUnit",
        &(unit,),
    ) else {
        return HashMap::new();
    };
    bus.get::<Vec<String>>("org.freedesktop.systemd1", path.as_str(), "org.freedesktop.systemd1.Service", "Environment")
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pair| pair.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

/// Whether the file a daemon writes (named by `var` in its environment,
/// else `default`) is in the Nix store and so pinned by the configuration.
pub(crate) fn pinned(env: &HashMap<String, String>, var: &str, default: &str) -> bool {
    let path = env.get(var).map(String::as_str).unwrap_or(default);
    in_store(path)
}

fn in_store(path: &str) -> bool {
    if path.starts_with("/etc/static/") || path.starts_with("/nix/store/") {
        return true;
    }
    std::fs::read_link(path).is_ok_and(|target| pinned_link(&target))
}

/// A NixOS-looking link, used as a conservative fallback when the module
/// doesn't publish its policy. Both NixOS and timedated can write an
/// absolute /etc/zoneinfo link, so its spelling alone cannot distinguish
/// a configured zone from a mutable one.
fn pinned_link(target: &std::path::Path) -> bool {
    target.starts_with("/etc/static") || target.starts_with("/nix/store") || target.starts_with("/etc/zoneinfo/")
}

/// The module publishes the effective time.timeZone policy. A mutable
/// localtime link may resolve into the store without being managed by Nix.
fn timezone_managed(env: &HashMap<String, String>) -> bool {
    match env.get("AIZOME_TIMEZONE_MANAGED").map(String::as_str) {
        Some("0") => false,
        Some("1") => true,
        _ => pinned(env, "SYSTEMD_ETC_LOCALTIME", "/etc/localtime"),
    }
}

// --- timedated -------------------------------------------------------------

/// What timedated reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeDateState {
    pub available: bool,
    /// The IANA time zone (`America/Chicago`).
    pub timezone: String,
    /// The system configuration pins the time zone (`time.timeZone`).
    pub timezone_managed: bool,
    /// The hardware clock keeps local time (rather than UTC).
    pub local_rtc: bool,
    /// Network time can be switched (a time sync service is installed).
    pub can_ntp: bool,
    /// Network time is on.
    pub ntp: bool,
    /// The clock is synchronized.
    pub ntp_synchronized: bool,
}

/// systemd-timedated.
pub struct TimeDate {
    bus: Option<Bus>,
    model: Arc<Model<TimeDateState>>,
    stop: Stop,
}

service_handle!(TimeDate, TimeDateState);

impl TimeDate {
    pub fn new(wake: Wake) -> TimeDate {
        TimeDate::with_bus(Bus::system().ok(), wake)
    }

    pub fn with_bus(bus: Option<Bus>, wake: Wake) -> TimeDate {
        let model = Arc::new(Model::new(TimeDateState::default(), wake));
        let stop = Stop::default();
        if let Some(bus) = bus.clone() {
            let follower = model.clone();
            bus::follow_props(bus, TIMEDATE, TIMEDATE_PATH, TIMEDATE, true, stop.clone(), move |bus, props| {
                let env = unit_environment(bus, "systemd-timedated.service");
                follower.set_if_changed(match props {
                    None => TimeDateState::default(),
                    Some(p) => TimeDateState {
                        available: true,
                        timezone: p.str("Timezone"),
                        timezone_managed: timezone_managed(&env),
                        local_rtc: p.bool("LocalRTC"),
                        can_ntp: p.bool("CanNTP"),
                        ntp: p.bool("NTP"),
                        ntp_synchronized: p.bool("NTPSynchronized"),
                    },
                });
            });
        }
        TimeDate { bus, model, stop }
    }

    pub fn available(&self) -> bool {
        self.model.get().available
    }

    fn call<B>(&self, method: &'static str, args: B) -> Task<()>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType + Send + 'static,
    {
        bus::spawn(&self.bus, "timedated", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, TIMEDATE, TIMEDATE_PATH, TIMEDATE, method, &args)
        })
    }

    /// Set the time zone (an IANA name, such as one of [`zones()`]).
    pub fn set_timezone(&self, timezone: &str) -> Task<()> {
        if self.model.get().timezone_managed {
            return Task::ready(Err(Error::ManagedBySystem("time.timeZone".into())));
        }
        if timezone.is_empty() || timezone.contains("..") || timezone.starts_with('/') {
            return Task::ready(Err(Error::InvalidArgument(format!("Not a time zone: {timezone}"))));
        }
        self.call("SetTimezone", (timezone.to_owned(), true))
    }

    /// Network time on or off.
    pub fn set_ntp(&self, on: bool) -> Task<()> {
        self.call("SetNTP", (on, true))
    }

    /// Set the clock (network time must be off).
    pub fn set_time(&self, time: SystemTime) -> Task<()> {
        let Ok(since) = time.duration_since(UNIX_EPOCH) else {
            return Task::ready(Err(Error::InvalidArgument("A time before 1970".into())));
        };
        let micros = i64::try_from(since.as_micros()).unwrap_or(i64::MAX);
        self.call("SetTime", (micros, false, true))
    }

    /// Whether the hardware clock keeps local time (for dual boot with
    /// Windows); `fix_system` sets the system clock from it.
    pub fn set_local_rtc(&self, local: bool, fix_system: bool) -> Task<()> {
        self.call("SetLocalRTC", (local, fix_system, true))
    }
}

impl Drop for TimeDate {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

// --- localed -------------------------------------------------------------------

/// What localed reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocaleState {
    pub available: bool,
    /// `LANG=en_US.UTF-8` and any `LC_*=…`.
    pub locale: Vec<String>,
    /// The system configuration pins the locale (`i18n.defaultLocale`).
    pub locale_managed: bool,
    pub x11_layout: String,
    pub x11_model: String,
    pub x11_variant: String,
    pub x11_options: String,
    pub vconsole_keymap: String,
    /// The system configuration pins the keyboard (`console.keyMap`,
    /// `services.xserver.xkb`).
    pub keyboard_managed: bool,
}

impl LocaleState {
    /// The value of `LANG`.
    pub fn lang(&self) -> Option<&str> {
        self.locale.iter().find_map(|entry| entry.strip_prefix("LANG="))
    }
}

/// systemd-localed.
pub struct Locale {
    bus: Option<Bus>,
    model: Arc<Model<LocaleState>>,
    stop: Stop,
}

service_handle!(Locale, LocaleState);

impl Locale {
    pub fn new(wake: Wake) -> Locale {
        Locale::with_bus(Bus::system().ok(), wake)
    }

    pub fn with_bus(bus: Option<Bus>, wake: Wake) -> Locale {
        let model = Arc::new(Model::new(LocaleState::default(), wake));
        let stop = Stop::default();
        if let Some(bus) = bus.clone() {
            let follower = model.clone();
            let mut env: Option<HashMap<String, String>> = None;
            bus::follow_props(bus, LOCALE, LOCALE_PATH, LOCALE, true, stop.clone(), move |bus, props| {
                let env = env.get_or_insert_with(|| unit_environment(bus, "systemd-localed.service"));
                follower.set_if_changed(match props {
                    None => LocaleState::default(),
                    Some(p) => LocaleState {
                        available: true,
                        locale: p.strings("Locale"),
                        locale_managed: pinned(env, "SYSTEMD_ETC_LOCALE_CONF", "/etc/locale.conf"),
                        x11_layout: p.str("X11Layout"),
                        x11_model: p.str("X11Model"),
                        x11_variant: p.str("X11Variant"),
                        x11_options: p.str("X11Options"),
                        vconsole_keymap: p.str("VConsoleKeymap"),
                        keyboard_managed: pinned(env, "SYSTEMD_ETC_VCONSOLE_CONF", "/etc/vconsole.conf"),
                    },
                });
            });
        }
        Locale { bus, model, stop }
    }

    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// Set the system locale: `["LANG=de_DE.UTF-8"]`, optionally with
    /// `LC_*=` entries.
    pub fn set_locale(&self, locale: &[String]) -> Task<()> {
        if self.model.get().locale_managed {
            return Task::ready(Err(Error::ManagedBySystem("i18n.defaultLocale".into())));
        }
        if locale.iter().any(|entry| !entry.contains('=')) {
            return Task::ready(Err(Error::InvalidArgument("Locale entries look like LANG=de_DE.UTF-8".into())));
        }
        let locale = locale.to_vec();
        bus::spawn(&self.bus, "localed", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, LOCALE, LOCALE_PATH, LOCALE, "SetLocale", &(locale, true))
        })
    }

    /// Set the X11 keyboard (and the console's, converted).
    pub fn set_x11_keyboard(&self, layout: &str, model: &str, variant: &str, options: &str) -> Task<()> {
        if self.model.get().keyboard_managed {
            return Task::ready(Err(Error::ManagedBySystem("console.keyMap / services.xserver.xkb".into())));
        }
        let args = (layout.to_owned(), model.to_owned(), variant.to_owned(), options.to_owned(), true, true);
        bus::spawn(&self.bus, "localed", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, LOCALE, LOCALE_PATH, LOCALE, "SetX11Keyboard", &args)
        })
    }
}

impl Drop for Locale {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

// --- hostnamed -----------------------------------------------------------------

/// What hostnamed reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostnameState {
    pub available: bool,
    /// The hostname in use.
    pub hostname: String,
    pub static_hostname: String,
    /// The system configuration pins it (`networking.hostName`).
    pub static_hostname_managed: bool,
    /// The name people see ("Chris's ThinkPad").
    pub pretty_hostname: String,
    /// `/etc/machine-info` is pinned, so the pretty name can't be set.
    pub pretty_hostname_managed: bool,
    /// `laptop`, `desktop`, `tablet`, `server`, `vm`…
    pub chassis: String,
    /// `computer-laptop`.
    pub icon_name: String,
    pub deployment: String,
    pub location: String,
    pub os_pretty_name: String,
    pub kernel_name: String,
    pub kernel_release: String,
    pub hardware_vendor: String,
    pub hardware_model: String,
    pub firmware_version: String,
}

impl HostnameState {
    /// The name to show: the pretty one, else the hostname.
    pub fn display_name(&self) -> &str {
        if self.pretty_hostname.is_empty() { &self.hostname } else { &self.pretty_hostname }
    }
}

/// systemd-hostnamed.
pub struct Hostname {
    bus: Option<Bus>,
    model: Arc<Model<HostnameState>>,
    stop: Stop,
}

service_handle!(Hostname, HostnameState);

pub(crate) fn hostname_state(p: &Props, env: &HashMap<String, String>) -> HostnameState {
    HostnameState {
        available: true,
        hostname: p.str("Hostname"),
        static_hostname: p.str("StaticHostname"),
        static_hostname_managed: pinned(env, "SYSTEMD_ETC_HOSTNAME", "/etc/hostname"),
        pretty_hostname: p.str("PrettyHostname"),
        pretty_hostname_managed: pinned(env, "SYSTEMD_ETC_MACHINE_INFO", "/etc/machine-info"),
        chassis: p.str("Chassis"),
        icon_name: p.str("IconName"),
        deployment: p.str("Deployment"),
        location: p.str("Location"),
        os_pretty_name: p.str("OperatingSystemPrettyName"),
        kernel_name: p.str("KernelName"),
        kernel_release: p.str("KernelRelease"),
        hardware_vendor: p.str("HardwareVendor"),
        hardware_model: p.str("HardwareModel"),
        firmware_version: p.str("FirmwareVersion"),
    }
}

impl Hostname {
    pub fn new(wake: Wake) -> Hostname {
        Hostname::with_bus(Bus::system().ok(), wake)
    }

    pub fn with_bus(bus: Option<Bus>, wake: Wake) -> Hostname {
        let model = Arc::new(Model::new(HostnameState::default(), wake));
        let stop = Stop::default();
        if let Some(bus) = bus.clone() {
            let follower = model.clone();
            let mut env: Option<HashMap<String, String>> = None;
            bus::follow_props(bus, HOSTNAME, HOSTNAME_PATH, HOSTNAME, true, stop.clone(), move |bus, props| {
                let env = env.get_or_insert_with(|| unit_environment(bus, "systemd-hostnamed.service"));
                follower.set_if_changed(props.map(|p| hostname_state(p, env)).unwrap_or_default());
            });
        }
        Hostname { bus, model, stop }
    }

    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// Set the pretty name ("Chris's ThinkPad"); empty clears it.
    pub fn set_pretty_hostname(&self, name: &str) -> Task<()> {
        if self.model.get().pretty_hostname_managed {
            return Task::ready(Err(Error::ManagedBySystem("/etc/machine-info is read-only".into())));
        }
        if name.contains('\n') {
            return Task::ready(Err(Error::InvalidArgument("A name is one line".into())));
        }
        let name = name.to_owned();
        bus::spawn(&self.bus, "hostnamed", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, HOSTNAME, HOSTNAME_PATH, HOSTNAME, "SetPrettyHostname", &(name, true))
        })
    }

    /// Set the static hostname (pinned on NixOS by `networking.hostName`).
    pub fn set_static_hostname(&self, name: &str) -> Task<()> {
        if self.model.get().static_hostname_managed {
            return Task::ready(Err(Error::ManagedBySystem("networking.hostName".into())));
        }
        if !valid_hostname(name) {
            return Task::ready(Err(Error::InvalidArgument(format!("Not a hostname: {name}"))));
        }
        let name = name.to_owned();
        bus::spawn(&self.bus, "hostnamed", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, HOSTNAME, HOSTNAME_PATH, HOSTNAME, "SetStaticHostname", &(name, true))
        })
    }
}

impl Drop for Hostname {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// A valid hostname: labels of letters, digits and inner dashes, separated
/// by dots, 1–63 bytes each and at most 253 in all.
pub fn valid_hostname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_paths_are_pinned() {
        let env = HashMap::from([
            ("SYSTEMD_ETC_LOCALTIME".to_owned(), "/etc/static/localtime".to_owned()),
            ("SYSTEMD_ETC_MACHINE_INFO".to_owned(), "/var/lib/machine-info".to_owned()),
        ]);
        assert!(pinned(&env, "SYSTEMD_ETC_LOCALTIME", "/etc/localtime"));
        assert!(!pinned(&env, "SYSTEMD_ETC_MACHINE_INFO", "/etc/machine-info"));
        assert!(!pinned(&HashMap::new(), "SYSTEMD_ETC_HOSTNAME", "/nonexistent/hostname"));
    }

    #[test]
    fn nixos_time_zone_link_is_pinned() {
        let dir = std::env::temp_dir().join(format!("aizome-sys-localtime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = |name: &str, target: &str| {
            let path = dir.join(name);
            let _ = std::fs::remove_file(&path);
            std::os::unix::fs::symlink(target, &path).unwrap();
            path.to_string_lossy().into_owned()
        };
        // `time.timeZone = "…"`: NixOS links /etc/localtime straight to
        // /etc/zoneinfo (itself a link into /etc/static), one hop that
        // isn't into /etc/static or the store.
        let nixos = link("nixos", "/etc/zoneinfo/Europe/Lisbon");
        // A relative link is another possible spelling; the module
        // publishes the actual policy for either spelling.
        let timedated = link("timedated", "../etc/zoneinfo/Europe/Lisbon");
        let none = HashMap::new();
        assert!(pinned(&none, "SYSTEMD_ETC_LOCALTIME", &nixos));
        assert!(!pinned(&none, "SYSTEMD_ETC_LOCALTIME", &timedated));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostnames() {
        assert!(valid_hostname("ganymede"));
        assert!(valid_hostname("io.jupiter"));
        assert!(!valid_hostname("-bad"));
        assert!(!valid_hostname("has space"));
        assert!(!valid_hostname(""));
        assert!(!valid_hostname(&"a".repeat(64)));
    }
}
