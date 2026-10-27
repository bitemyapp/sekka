//! Session-scoped NetworkManager SecretAgent. The daemon owns registration,
//! cancellation and keyring access; short-lived native dialogs own only UI.
pub mod keyring;
pub mod model;
pub mod process;
pub mod prompt;
pub mod vpn;

use keyring::Store;
use model::{AGENT_OWNED, ALLOW_INTERACTION, NOT_REQUIRED, NOT_SAVED, Profile, Prompt, REQUEST_NEW, Settings};
use process::{Cancel, GET_SECRETS_TIMEOUT, SAVE_SECRETS_TIMEOUT};
use prompt::Prompter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use zbus::{message::Header, zvariant::OwnedObjectPath};

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
pub const IDENTIFIER: &str = "org.aizome.NetworkAgent";
pub type Result<T> = std::result::Result<T, AgentError>;

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.NetworkManager.SecretAgent")]
pub enum AgentError {
    Failed(String),
    InvalidConnection(String),
    UserCanceled(String),
    AgentCanceled(String),
    NoSecrets(String),
    NotAuthorized(String),
}

pub struct Engine {
    pub store: Arc<dyn Store>,
    pub ui: Arc<dyn Prompter>,
    pub plugins: vpn::Plugins,
}
impl Engine {
    pub fn get(&self, p: Profile, setting: &str, hints: &[String], flags: u32, cancel: &Cancel) -> Result<Settings> {
        cancel.check()?;
        let interactive = flags & ALLOW_INTERACTION != 0;
        let mut values = p.supplied(setting);
        let fields = if setting == "vpn" { Vec::new() } else { p.fields(setting, hints)? };
        let needs_keyring = if setting == "vpn" {
            model::dict(&p.settings, "vpn", "data")
                .iter()
                .any(|(key, value)| key.ends_with("-flags") && value.parse::<u32>().ok() == Some(AGENT_OWNED))
        } else {
            fields.iter().any(|f| p.flags(setting, &f.key) == AGENT_OWNED)
        };
        let saved = if !needs_keyring || flags & REQUEST_NEW != 0 {
            HashMap::new()
        } else {
            match self.store.lookup(&p.uuid, setting, interactive, cancel) {
                Ok(values) => values,
                Err(_) if interactive => {
                    cancel.check()?;
                    HashMap::new()
                }
                Err(_) => return Err(AgentError::NoSecrets("Stored credentials are unavailable".into())),
            }
        };
        for (key, value) in saved {
            if p.flags(setting, &key) & (AGENT_OWNED | NOT_SAVED | NOT_REQUIRED) == AGENT_OWNED {
                values.entry(key).or_insert(value);
            }
        }
        let response = if setting == "vpn" {
            self.plugins.secrets(&p, hints, flags, values, self.ui.as_ref(), cancel)?
        } else {
            let mut fields = fields;
            let mut answer = HashMap::new();
            fields.retain(|f| {
                if flags & REQUEST_NEW == 0
                    && let Some(value) = values.get(&f.key).filter(|v| !v.is_empty())
                {
                    answer.insert(f.key.clone(), value.clone());
                    false
                } else {
                    true
                }
            });
            if !fields.is_empty() {
                if !interactive {
                    return Err(AgentError::NoSecrets("Network authentication needs user interaction".into()));
                }
                let requested: Vec<_> = fields.iter().map(|f| f.key.clone()).collect();
                let entered = self.ui.ask(
                    Prompt {
                        title: "Network Authentication".into(),
                        message: format!("Enter credentials for “{}”.", p.name),
                        fields,
                    },
                    cancel,
                )?;
                for key in requested {
                    let value = entered
                        .get(&key)
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| AgentError::NoSecrets("Required credential was not supplied".into()))?;
                    answer.insert(key, value.clone());
                }
            }
            answer
        };
        cancel.check()?;
        // Preserve each existing profile's storage policy. System-owned
        // values go back to NM; one-time/ask-every-time values are never saved.
        for (key, value) in &response {
            if interactive
                && p.flags(setting, key) == AGENT_OWNED
                && self.store.store(&p.uuid, setting, key, value, &format!("{} — {setting}", p.name), cancel).is_err()
            {
                cancel.check()?;
                // Failure to remember a password must not prevent using it.
                log::warn!("The keyring could not save network credentials");
            }
        }
        Ok(p.reply(setting, response))
    }
    pub fn save(&self, p: Profile, cancel: &Cancel) -> Result<()> {
        // NetworkManager sends SaveSecrets after every profile update, with
        // the agent-owned secrets the update carried or, failing that, those
        // it holds in memory: none for a connection not activated since it
        // started. So replace only the secrets supplied, and forget a saved
        // one only once the profile stops keeping it in the keyring (an
        // editor changing it to "ask every time", say).
        for (setting, key) in self.store.saved(&p.uuid, cancel)? {
            if !p.saves(&setting, &key) {
                self.store.forget(&p.uuid, &setting, &key, cancel)?;
            }
        }
        for setting in p.settings.keys() {
            for (key, value) in p.supplied(setting) {
                if p.saves(setting, &key) {
                    self.store.store(&p.uuid, setting, &key, &value, &format!("{} — {setting}", p.name), cancel)?;
                }
            }
        }
        Ok(())
    }
}

type Key = (String, String);
/// A GetSecrets in progress, numbered in the order calls arrived.
struct Request {
    key: Key,
    number: u64,
    cancel: Cancel,
    authorized: bool,
}
#[derive(Default)]
struct Requests {
    arrived: u64,
    list: Vec<Request>,
}
impl Requests {
    fn arrive(&mut self) -> u64 {
        self.arrived += 1;
        self.arrived
    }
    /// Cancels the requests for `key` that arrived before `number`.
    fn cancel_before(&mut self, key: &Key, number: u64) {
        self.list.retain(|r| {
            let earlier = r.key == *key && r.number < number;
            if earlier {
                r.cancel.cancel();
            }
            !earlier
        });
    }
}
pub struct Agent {
    engine: Arc<Engine>,
    requests: Arc<Mutex<Requests>>,
}
impl Agent {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine, requests: Arc::default() }
    }
    async fn authorize(bus: &zbus::Connection, header: &Header<'_>) -> Result<()> {
        let proxy = zbus::fdo::DBusProxy::new(bus)
            .await
            .map_err(|_| AgentError::NotAuthorized("NetworkManager is unavailable".into()))?;
        let owner = proxy
            .get_name_owner(NM.try_into().unwrap())
            .await
            .map_err(|_| AgentError::NotAuthorized("NetworkManager is unavailable".into()))?;
        if header.sender().map(|n| n.as_str()) != Some(owner.as_str()) {
            return Err(AgentError::NotAuthorized("Only NetworkManager may request credentials".into()));
        }
        Ok(())
    }
    pub fn cancel_all(&self) {
        for request in self.requests.lock().unwrap().list.drain(..) {
            request.cancel.cancel();
        }
    }
    async fn work<T: Send + 'static>(&self, f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(f());
        });
        rx.recv().await.map_err(|_| AgentError::Failed("Credential worker stopped".into()))?
    }
}
#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
impl Agent {
    // NetworkManager's method signature, and what zbus passes along.
    #[allow(clippy::too_many_arguments)]
    async fn get_secrets(
        &self,
        connection: Settings,
        connection_path: OwnedObjectPath,
        setting_name: String,
        hints: Vec<String>,
        flags: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> Result<Settings> {
        // NetworkManager gives the whole request two minutes, every dialog
        // and helper included, and then stops waiting without cancelling.
        let cancel = Cancel::within(GET_SECRETS_TIMEOUT);
        let key = (connection_path.to_string(), setting_name.clone());
        // Listed before authorizing, so that a CancelGetSecrets arriving
        // meanwhile finds it; it displaces an earlier request for the same
        // setting only once authorized.
        let number = {
            let mut requests = self.requests.lock().unwrap();
            let number = requests.arrive();
            requests.list.push(Request { key: key.clone(), number, cancel: cancel.clone(), authorized: false });
            number
        };
        let result = async {
            Self::authorize(bus, &header).await?;
            let profile = Profile::new(connection)?;
            {
                let mut requests = self.requests.lock().unwrap();
                if requests.list.iter().filter(|r| r.authorized && r.key != key).count() >= 16 {
                    return Err(AgentError::Failed("Too many outstanding authentication requests".into()));
                }
                requests.cancel_before(&key, number);
                if let Some(request) = requests.list.iter_mut().find(|r| r.number == number) {
                    request.authorized = true;
                }
            }
            let engine = self.engine.clone();
            let token = cancel.clone();
            self.work(move || engine.get(profile, &setting_name, &hints, flags, &token)).await
        }
        .await;
        self.requests.lock().unwrap().list.retain(|r| r.number != number);
        result
    }
    async fn cancel_get_secrets(
        &self,
        connection_path: OwnedObjectPath,
        setting_name: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> Result<()> {
        // Only requests that arrived before this.
        let number = self.requests.lock().unwrap().arrive();
        Self::authorize(bus, &header).await?;
        self.requests.lock().unwrap().cancel_before(&(connection_path.to_string(), setting_name), number);
        Ok(())
    }
    async fn save_secrets(
        &self,
        connection: Settings,
        _connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> Result<()> {
        let cancel = Cancel::within(SAVE_SECRETS_TIMEOUT);
        Self::authorize(bus, &header).await?;
        let profile = Profile::new(connection)?;
        let engine = self.engine.clone();
        let result = self.work(move || engine.save(profile, &cancel)).await;
        if result.is_ok() {
            log::info!("Saved network credentials follow a profile update");
        }
        result
    }
    async fn delete_secrets(
        &self,
        connection: Settings,
        _connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> Result<()> {
        let cancel = Cancel::within(SAVE_SECRETS_TIMEOUT);
        Self::authorize(bus, &header).await?;
        let profile = Profile::new(connection)?;
        let engine = self.engine.clone();
        self.work(move || engine.store.delete(&profile.uuid, &cancel)).await
    }
}

/// How soon to ask NetworkManager again after it refused to register us.
pub const REGISTER_RETRY: Duration = Duration::from_secs(2);

/// Keep the same agent process across shell reloads. Re-register after a
/// NetworkManager restart; cancel requests belonging to its old owner.
pub fn serve() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let _singleton = zbus::blocking::connection::Builder::session()?.name(IDENTIFIER)?.build()?;
    let engine = Arc::new(Engine {
        store: Arc::new(keyring::Keyring),
        ui: Arc::new(prompt::Native),
        plugins: vpn::Plugins::default(),
    });
    // Served as the connection is built, so that no call arrives before
    // the agent listens: one would be lost, and its caller left waiting.
    run(zbus::blocking::connection::Builder::system()?.serve_at(PATH, Agent::new(engine))?.build()?)
}

/// Follow NetworkManager on `connection` (the system bus, serving the
/// agent at PATH) until it closes, so that systemd starts the agent again.
pub fn run(connection: zbus::blocking::Connection) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let agent = connection.object_server().interface::<_, Agent>(PATH)?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
    // Watch before asking, so that no change is missed. The signals end
    // with the connection.
    let changes = bus.receive_name_owner_changed_with_args(&[(0, NM)])?;
    let (tx, owners) = mpsc::channel();
    std::thread::spawn(move || {
        for signal in changes {
            if let Ok(args) = signal.args()
                && tx.send((args.old_owner.is_some(), args.new_owner.as_ref().map(|n| n.to_string()))).is_err()
            {
                break;
            }
        }
    });
    let mut owner = match bus.get_name_owner(NM.try_into()?) {
        Ok(name) => Some(name.to_string()),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => None,
        Err(e) => return Err(e.into()),
    };
    let mut registration = Registration::default();
    loop {
        if owner.is_some() {
            registration.attempt(&connection);
        }
        let change = if owner.is_some() && !registration.done {
            match owners.recv_timeout(REGISTER_RETRY) {
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                change => change.ok(),
            }
        } else {
            owners.recv().ok()
        };
        let Some((left, new_owner)) = change else {
            return Err("The system bus connection closed".into());
        };
        if left {
            // The NetworkManager that asked can no longer be answered.
            agent.get().cancel_all();
        }
        if new_owner != owner {
            registration = Registration::default();
            owner = new_owner;
        }
    }
}

/// Registration with one NetworkManager.
#[derive(Default)]
struct Registration {
    done: bool,
    asked: bool,
}
impl Registration {
    fn attempt(&mut self, connection: &zbus::blocking::Connection) {
        if self.done {
            return;
        }
        let reply = zbus::blocking::Proxy::new(
            connection,
            NM,
            "/org/freedesktop/NetworkManager/AgentManager",
            "org.freedesktop.NetworkManager.AgentManager",
        )
        // VPN_HINTS: pass plugin challenge hints through unchanged.
        .and_then(|manager| manager.call::<_, _, ()>("RegisterWithCapabilities", &(IDENTIFIER, 1u32)));
        self.done = match reply {
            Ok(()) => true,
            // NetworkManager allows one agent per name and user. If this
            // process asked before, it is the one registered (a reply was
            // lost); if not, another still is, until its connection closes.
            Err(zbus::Error::MethodError(name, Some(message), _))
                if name == "org.freedesktop.NetworkManager.AgentManager.PermissionDenied"
                    && message.contains("already registered") =>
            {
                self.asked
            }
            Err(_) => false,
        };
        if self.done {
            log::info!("Network credential agent registered");
        } else if !self.asked {
            log::warn!("Network credential agent registration deferred");
        }
        self.asked = true;
    }
}
