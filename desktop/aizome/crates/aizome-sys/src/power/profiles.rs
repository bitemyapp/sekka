//! Power profiles (power-saver, balanced, performance), through
//! power-profiles-daemon's D-Bus API, which tuned's `tuned-ppd` also
//! provides. The current name `org.freedesktop.UPower.PowerProfiles` is
//! tried first, then the original `net.hadess.PowerProfiles`.

use crate::bus::{self, Bus, Event, Opts, Props, PropsExt, signals_from, value_dicts};
use crate::error::Result;
use crate::service::{Model, Stop, Task, Wake, service_handle};
use std::sync::{Arc, Mutex};

const NAMES: [(&str, &str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles", "net.hadess.PowerProfiles"),
];

/// What the power profiles daemon reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfilesState {
    pub available: bool,
    /// `power-saver`, `balanced` or `performance`.
    pub active: String,
    pub profiles: Vec<Profile>,
    /// Why performance is degraded (`lap-detected`, `high-operating-temperature`),
    /// empty when it isn't.
    pub performance_degraded: String,
    /// Applications holding a profile, and why.
    pub holds: Vec<ProfileHold>,
    /// Extra things the daemon does (`trickle_charge`, `amdgpu_panel_power`).
    pub actions: Vec<String>,
}

impl ProfilesState {
    pub fn has(&self, profile: &str) -> bool {
        self.profiles.iter().any(|p| p.name == profile)
    }
}

/// A profile the hardware supports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// What implements it (`platform_profile`, `intel_pstate`, `multiple`).
    pub driver: String,
}

/// An application holding a profile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileHold {
    pub application_id: String,
    pub profile: String,
    pub reason: String,
}

pub(crate) fn build(p: &Props) -> ProfilesState {
    let profiles = p
        .value("Profiles")
        .map(value_dicts)
        .unwrap_or_default()
        .iter()
        .filter_map(|d| Some(Profile { name: d.opt_str("Profile")?, driver: d.str("Driver") }))
        .collect();
    let holds = p
        .value("ActiveProfileHolds")
        .map(value_dicts)
        .unwrap_or_default()
        .iter()
        .map(|d| ProfileHold {
            application_id: d.str("ApplicationId"),
            profile: d.str("Profile"),
            reason: d.str("Reason"),
        })
        .collect();
    ProfilesState {
        available: true,
        active: p.str("ActiveProfile"),
        profiles,
        performance_degraded: p.str("PerformanceDegraded"),
        holds,
        actions: p.strings("Actions"),
    }
}

/// Power profiles.
pub struct PowerProfiles {
    bus: Option<Bus>,
    model: Arc<Model<ProfilesState>>,
    /// Which of [`NAMES`] answered.
    which: Arc<Mutex<usize>>,
    stop: Stop,
}

service_handle!(PowerProfiles, ProfilesState);

impl PowerProfiles {
    pub fn new(wake: Wake) -> PowerProfiles {
        match Bus::system() {
            Ok(bus) => PowerProfiles::with_bus(bus, wake),
            Err(_) => PowerProfiles {
                bus: None,
                model: Arc::new(Model::new(ProfilesState::default(), wake)),
                which: Arc::new(Mutex::new(0)),
                stop: Stop::default(),
            },
        }
    }

    pub fn with_bus(bus: Bus, wake: Wake) -> PowerProfiles {
        let model = Arc::new(Model::new(ProfilesState::default(), wake));
        let stop = Stop::default();
        // The current name, unless only the original is there (or
        // activatable).
        let which = usize::from(
            bus.owner(NAMES[0].0).is_none()
                && !bus.activatable(NAMES[0].0)
                && (bus.owner(NAMES[1].0).is_some() || bus.activatable(NAMES[1].0)),
        );
        let (name, path, iface) = NAMES[which];
        let follower = model.clone();
        let mut props = Props::new();
        let mut present = false;
        bus::follow(bus.clone(), name, vec![signals_from(name)], true, stop.clone(), move |bus, event| match event {
            Event::Appeared => {
                present = true;
                props = bus.get_all(name, path, iface).unwrap_or_default();
            }
            Event::Vanished => {
                present = false;
                follower.set_if_changed(ProfilesState::default());
            }
            Event::Signal(msg) => {
                if bus::member(&msg).1 == "PropertiesChanged"
                    && let Ok((changed_iface, changed, invalidated)) =
                        msg.body().deserialize::<(String, Props, Vec<String>)>()
                    && changed_iface == iface
                {
                    props.extend(changed);
                    if !invalidated.is_empty() {
                        props = bus.get_all(name, path, iface).unwrap_or_default();
                    }
                }
            }
            Event::Idle => {
                if present {
                    follower.set_if_changed(build(&props));
                }
            }
        });
        PowerProfiles { bus: Some(bus), model, which: Arc::new(Mutex::new(which)), stop }
    }

    pub fn available(&self) -> bool {
        self.model.get().available
    }

    fn names(&self) -> (&'static str, &'static str, &'static str) {
        NAMES[*crate::service::lock(&self.which)]
    }

    /// Switch to a profile (`power-saver`, `balanced`, `performance`).
    pub fn set_active(&self, profile: &str) -> Task<()> {
        let (name, path, iface) = self.names();
        let profile = profile.to_owned();
        bus::spawn(&self.bus, "power profiles", self.model.wake(), move |bus| {
            bus.set(name, path, iface, "ActiveProfile", profile)
        })
    }

    /// Hold a profile while something runs (a game holding performance);
    /// the result is the cookie for [`release`](PowerProfiles::release).
    /// The daemon drops the hold when this process leaves the bus.
    pub fn hold(&self, profile: &str, reason: &str, application_id: &str) -> Task<u32> {
        let (name, path, iface) = self.names();
        let args = (profile.to_owned(), reason.to_owned(), application_id.to_owned());
        bus::spawn(&self.bus, "power profiles", self.model.wake(), move |bus| -> Result<u32> {
            bus.call_with(Opts::ACTION, name, path, iface, "HoldProfile", &args)
        })
    }

    pub fn release(&self, cookie: u32) -> Task<()> {
        let (name, path, iface) = self.names();
        bus::spawn(&self.bus, "power profiles", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, name, path, iface, "ReleaseProfile", &(cookie,))
        })
    }
}

impl Drop for PowerProfiles {
    fn drop(&mut self) {
        self.stop.stop();
    }
}
