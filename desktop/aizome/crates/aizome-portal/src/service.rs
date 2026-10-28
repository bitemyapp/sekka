//! The bus side: `org.freedesktop.impl.portal.FileChooser`, and for each
//! call an `org.freedesktop.impl.portal.Request` at its handle, which the
//! frontend closes when the app gives up (or goes): that ends the
//! chooser.

use std::process::Command;
use std::sync::{Arc, Mutex};

use zbus::object_server::ObjectServer;
use zbus::zvariant::OwnedObjectPath;

use crate::request::{Answer, Mode, Options, Request};
use crate::worker::{self, Cancel, Outcome};

/// The portal's responses.
pub const SUCCESS: u32 = 0;
pub const CANCELLED: u32 = 1;
pub const ENDED: u32 = 2;

/// How many choosers may be open at once: more is an app gone wrong.
pub const MAX_OPEN: usize = 16;

/// What runs a chooser.
pub type Launcher = Arc<dyn Fn() -> Command + Send + Sync>;

/// This program again, as a chooser.
pub fn own_chooser() -> Launcher {
    Arc::new(|| {
        let exe = std::env::current_exe().unwrap_or_else(|_| "aizome-portal".into());
        let mut c = Command::new(exe);
        c.arg("--file-chooser");
        c
    })
}

pub struct FileChooser {
    launch: Launcher,
    open: Arc<Mutex<usize>>,
}

impl FileChooser {
    pub fn new(launch: Launcher) -> FileChooser {
        FileChooser { launch, open: Arc::default() }
    }

    async fn ask(&self, server: &ObjectServer, handle: OwnedObjectPath, request: Request) -> (u32, Options) {
        let mode = request.mode;
        {
            let mut open = self.open.lock().unwrap();
            if *open >= MAX_OPEN {
                log::warn!("a file chooser was refused: {MAX_OPEN} are open");
                return (ENDED, Options::new());
            }
            *open += 1;
        }
        let cancel = Cancel::default();
        // The handle is the frontend's to close it by. A handle that's
        // taken can't be closed through us; the chooser still runs.
        if let Err(e) = server.at(&handle, RequestHandle { cancel: cancel.clone() }).await {
            log::warn!("the file chooser's request couldn't be served at {}: {e}", handle.as_str());
        }
        let (tx, rx) = async_channel::bounded(1);
        let launch = self.launch.clone();
        let token = cancel.clone();
        std::thread::spawn(move || {
            let input = serde_json::to_vec(&request).unwrap_or_default();
            let _ = tx.send_blocking(worker::run(&mut launch(), &input, &token));
        });
        let outcome = rx.recv().await.unwrap_or(Outcome::Failed("the chooser's thread stopped".into()));
        let _ = server.remove::<RequestHandle, _>(&handle).await;
        *self.open.lock().unwrap() -= 1;
        match outcome {
            Outcome::Answered(bytes) => match serde_json::from_slice::<Option<Answer>>(&bytes) {
                Ok(Some(answer)) => (SUCCESS, answer.to_bus(mode)),
                Ok(None) => (CANCELLED, Options::new()),
                Err(e) => {
                    log::warn!("the file chooser answered nonsense: {e}");
                    (ENDED, Options::new())
                }
            },
            Outcome::Closed => (ENDED, Options::new()),
            Outcome::Failed(why) => {
                log::warn!("{why}");
                (ENDED, Options::new())
            }
        }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Options) {
        let request = Request::from_bus(Mode::Open, &app_id, &parent_window, &title, &options);
        self.ask(server, handle, request).await
    }

    async fn save_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Options) {
        let request = Request::from_bus(Mode::Save, &app_id, &parent_window, &title, &options);
        self.ask(server, handle, request).await
    }

    async fn save_files(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Options) {
        let request = Request::from_bus(Mode::SaveFiles, &app_id, &parent_window, &title, &options);
        self.ask(server, handle, request).await
    }
}

/// A call in progress, at its handle.
struct RequestHandle {
    cancel: Cancel,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl RequestHandle {
    async fn close(&self) {
        self.cancel.cancel();
    }
}

/// Serve the portal on the session bus until the bus goes, or another
/// backend takes the name.
pub fn serve() -> Result<(), Box<dyn std::error::Error>> {
    // Served before the name is taken, so that no call finds nothing.
    let connection = zbus::blocking::connection::Builder::session()?
        .serve_at(crate::OBJECT_PATH, FileChooser::new(own_chooser()))?
        .name(crate::BUS_NAME)?
        .build()?;
    log::info!("serving {}", crate::BUS_NAME);
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
    let ours = connection.unique_name().map(|n| n.to_string());
    for signal in bus.receive_name_owner_changed_with_args(&[(0, crate::BUS_NAME)])? {
        if let Ok(args) = signal.args()
            && args.new_owner.as_ref().map(|n| n.to_string()) != ours
        {
            return Err("another file chooser portal took the name".into());
        }
    }
    Err("the session bus closed".into())
}
