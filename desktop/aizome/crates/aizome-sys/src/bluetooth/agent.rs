//! The Bluetooth pairing agent: `org.bluez.Agent1`, served on our bus
//! connection and registered as BlueZ's default agent with the
//! `KeyboardDisplay` capability (it can show codes and take typed ones).
//!
//! BlueZ calls the agent while pairing (ours or one a device starts). Each
//! call becomes a [`PairingPrompt`] for the UI's callback. The UI shows it
//! and later answers through the prompt's [`Responder`]; until then the
//! D-Bus call stays pending (BlueZ gives up after about 30 seconds and
//! sends a [`PairingRequest::Cancelled`] prompt). A responder dropped
//! without an answer rejects the request.
//!
//! Registering and unregistering take several round trips to BlueZ (and
//! wait for it when it is busy), so they run on a thread of their own
//! ([`Worker`]), in the order asked; the UI thread never waits on them.

use super::BLUEZ;
use crate::bus::{Bus, Opts};
use crate::error::{Error, Result};
use crate::oneshot;
use crate::service::{Done, Task, Wake};
use std::sync::{Arc, Mutex, mpsc};
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

const AGENT_PATH: &str = "/org/aizome/bluetooth/agent";
const CAPABILITY: &str = "KeyboardDisplay";

/// What BlueZ asks of the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingRequest {
    /// Type the PIN the device shows or was set up with (legacy pairing);
    /// answer [`PairingResponse::PinCode`].
    PinCode,
    /// Type the 6-digit passkey the device shows; answer
    /// [`PairingResponse::Passkey`].
    Passkey,
    /// Show this PIN, to be typed on the device. No answer is needed.
    DisplayPinCode(String),
    /// Show this passkey (6 digits, zero-padded), to be typed on the device;
    /// `entered` keys have been typed so far. No answer is needed.
    DisplayPasskey { passkey: u32, entered: u16 },
    /// Does the device show this passkey too? Accept or reject.
    Confirm(u32),
    /// May this device pair (no code involved)? Accept or reject.
    Authorize,
    /// May this device use the service with this UUID? Accept or reject.
    AuthorizeService(String),
    /// BlueZ cancelled the request shown last (for this device: the one
    /// the agent last asked about): hide it.
    Cancelled,
}

/// The person's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingResponse {
    Accept,
    Reject,
    PinCode(String),
    Passkey(u32),
}

/// A request from BlueZ, for the UI to show.
#[derive(Debug)]
pub struct PairingPrompt {
    /// The device's object path.
    pub device: String,
    /// Its name, to show.
    pub name: String,
    pub request: PairingRequest,
    pub responder: Responder,
}

/// Answers a [`PairingPrompt`]. It may be moved to the UI thread and used
/// later; dropping it unanswered rejects.
pub struct Responder {
    tx: Option<oneshot::Sender<PairingResponse>>,
}

impl std::fmt::Debug for Responder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Responder").field("wants_answer", &self.wants_answer()).finish()
    }
}

impl Responder {
    fn none() -> Responder {
        Responder { tx: None }
    }

    /// Answer the request.
    pub fn respond(mut self, response: PairingResponse) {
        if let Some(tx) = self.tx.take() {
            tx.send(response);
        }
    }

    pub fn accept(self) {
        self.respond(PairingResponse::Accept)
    }

    pub fn reject(self) {
        self.respond(PairingResponse::Reject)
    }

    /// Whether an answer is still awaited (false for display-only prompts
    /// and once BlueZ gave up).
    pub fn wants_answer(&self) -> bool {
        self.tx.as_ref().is_some_and(|tx| !tx.is_closed())
    }
}

pub(crate) type Prompt = Arc<dyn Fn(PairingPrompt) + Send + Sync>;

struct Agent {
    bus: Bus,
    prompt: Prompt,
    /// The device of the request shown last, which BlueZ's `Cancel` (it
    /// names none) is about.
    current: Mutex<Option<String>>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    Rejected(String),
}

impl Agent {
    async fn name(&self, device: &str) -> String {
        let alias: Result<zbus::zvariant::OwnedValue> = self
            .bus
            .call_async(
                Opts::QUERY,
                BLUEZ,
                device,
                "org.freedesktop.DBus.Properties",
                "Get",
                &("org.bluez.Device1", "Alias"),
            )
            .await;
        alias.ok().and_then(|v| String::try_from(v).ok()).unwrap_or_else(|| device.to_owned())
    }

    /// Show a prompt and wait for the answer.
    async fn ask(&self, device: &OwnedObjectPath, request: PairingRequest) -> Option<PairingResponse> {
        let (tx, rx) = oneshot::channel();
        let name = self.name(device.as_str()).await;
        self.shown(device.as_str());
        (self.prompt)(PairingPrompt {
            device: device.to_string(),
            name,
            request,
            responder: Responder { tx: Some(tx) },
        });
        rx.await
    }

    /// Show a prompt that needs no answer.
    async fn show(&self, device: &OwnedObjectPath, request: PairingRequest) {
        let name = self.name(device.as_str()).await;
        self.shown(device.as_str());
        (self.prompt)(PairingPrompt { device: device.to_string(), name, request, responder: Responder::none() });
    }

    fn shown(&self, device: &str) {
        *crate::service::lock(&self.current) = Some(device.to_owned());
    }

    async fn accept_or_reject(&self, device: OwnedObjectPath, request: PairingRequest) -> Result<(), AgentError> {
        match self.ask(&device, request).await {
            Some(PairingResponse::Accept) => Ok(()),
            Some(_) => Err(AgentError::Rejected("Rejected".into())),
            None => Err(AgentError::Rejected("Rejected".into())),
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        match self.ask(&device, PairingRequest::PinCode).await {
            Some(PairingResponse::PinCode(pin)) if (1..=16).contains(&pin.len()) => Ok(pin),
            Some(_) => Err(AgentError::Rejected("Rejected".into())),
            None => Err(AgentError::Rejected("Rejected".into())),
        }
    }

    async fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) {
        self.show(&device, PairingRequest::DisplayPinCode(pincode)).await
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        match self.ask(&device, PairingRequest::Passkey).await {
            Some(PairingResponse::Passkey(passkey)) if passkey <= 999_999 => Ok(passkey),
            Some(_) => Err(AgentError::Rejected("Rejected".into())),
            None => Err(AgentError::Rejected("Rejected".into())),
        }
    }

    async fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16) {
        self.show(&device, PairingRequest::DisplayPasskey { passkey, entered }).await
    }

    async fn request_confirmation(&self, device: OwnedObjectPath, passkey: u32) -> Result<(), AgentError> {
        self.accept_or_reject(device, PairingRequest::Confirm(passkey)).await
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.accept_or_reject(device, PairingRequest::Authorize).await
    }

    async fn authorize_service(&self, device: OwnedObjectPath, uuid: String) -> Result<(), AgentError> {
        self.accept_or_reject(device, PairingRequest::AuthorizeService(uuid)).await
    }

    fn cancel(&self) {
        let device = crate::service::lock(&self.current).take().unwrap_or_default();
        (self.prompt)(PairingPrompt {
            device,
            name: String::new(),
            request: PairingRequest::Cancelled,
            responder: Responder::none(),
        });
    }
}

/// Registers and unregisters the agent on its own thread, one request
/// after another in the order they were made, so a quick hide and show
/// can't unregister the agent just registered.
#[derive(Default)]
pub(crate) struct Worker {
    tx: Mutex<Option<mpsc::Sender<Op>>>,
}

enum Op {
    Register(Bus, Prompt, Done<()>),
    Unregister(Done<()>),
}

impl Worker {
    /// Serves the agent and makes it BlueZ's default (replacing one
    /// registered before); the task says how it went.
    pub(crate) fn register(&self, bus: &Bus, prompt: Prompt, wake: Wake) -> Task<()> {
        let (task, done) = Task::pending(wake);
        self.send(Op::Register(bus.clone(), prompt, done));
        task
    }

    /// Stops serving it.
    pub(crate) fn unregister(&self, wake: Wake) -> Task<()> {
        let (task, done) = Task::pending(wake);
        self.send(Op::Unregister(done));
        task
    }

    fn send(&self, op: Op) {
        let mut tx = crate::service::lock(&self.tx);
        if tx.is_none() {
            let (sender, receiver) = mpsc::channel();
            let spawned =
                std::thread::Builder::new().name("aizome-sys BlueZ agent".into()).spawn(move || serve(receiver));
            if spawned.is_err() {
                // The op's Done, dropped, finishes its task as cancelled.
                return;
            }
            *tx = Some(sender);
        }
        if let Some(sender) = tx.as_ref() {
            // A dead thread drops the op, which cancels its task.
            let _ = sender.send(op);
        }
    }
}

fn serve(ops: mpsc::Receiver<Op>) {
    let mut current: Option<Registration> = None;
    for op in ops {
        match op {
            Op::Register(bus, prompt, done) => {
                // The one before goes first (its Drop unregisters it).
                current = None;
                match register(&bus, prompt) {
                    Ok(r) => {
                        current = Some(r);
                        done.finish(Ok(()));
                    }
                    Err(e) => done.finish(Err(e)),
                }
            }
            Op::Unregister(done) => {
                current = None;
                done.finish(Ok(()));
            }
        }
    }
    // The service is gone: so is its agent, unregistered here.
    drop(current);
}

/// The agent being served; unregistered when dropped.
pub(crate) struct Registration {
    bus: Bus,
}

pub(crate) fn register(bus: &Bus, prompt: Prompt) -> Result<Registration> {
    let conn = bus.connection().clone();
    let agent = Agent { bus: bus.clone(), prompt, current: Mutex::new(None) };
    zbus::block_on(async {
        let server = conn.object_server();
        let _ = server.remove::<Agent, _>(AGENT_PATH).await;
        server.at(AGENT_PATH, agent).await
    })?;
    crate::bus::wait_until_serving(bus, AGENT_PATH)?;
    let path = ObjectPath::try_from(AGENT_PATH)?;
    let registered = bus
        .call::<_, ()>(BLUEZ, "/org/bluez", "org.bluez.AgentManager1", "RegisterAgent", &(&path, CAPABILITY))
        .or_else(|error| match &error {
            Error::DBus { name, .. } if name == "org.bluez.Error.AlreadyExists" => Ok(()),
            _ => Err(error),
        })
        .and_then(|()| {
            bus.call::<_, ()>(BLUEZ, "/org/bluez", "org.bluez.AgentManager1", "RequestDefaultAgent", &(&path,))
        });
    if let Err(error) = registered {
        let _ = zbus::block_on(conn.object_server().remove::<Agent, _>(AGENT_PATH));
        return Err(error);
    }
    Ok(Registration { bus: bus.clone() })
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(path) = ObjectPath::try_from(AGENT_PATH) {
            let _ =
                self.bus.call::<_, ()>(BLUEZ, "/org/bluez", "org.bluez.AgentManager1", "UnregisterAgent", &(&path,));
        }
        let conn = self.bus.connection().clone();
        let _ = zbus::block_on(async move { conn.object_server().remove::<Agent, _>(AGENT_PATH).await });
    }
}
