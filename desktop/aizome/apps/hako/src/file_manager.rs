//! The standard "show in folder" interface used by browsers and the
//! OpenURI portal. The Aizome session owns it even with no Files window.
//! No global D-Bus activation entry: other desktops keep their provider.

use std::collections::HashSet;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use aizome_sys::apps::{LaunchEvent, LaunchObserver, LaunchOptions, find_program, launch_command_observed};
use zbus::blocking::{Connection, connection::Builder};
use zbus::fdo;

use crate::fsops::names;
use crate::model::uri;
use crate::ui::{Request, instance};

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

/// The most windows one call may open.
pub const MAX_WINDOWS: usize = 16;

pub struct FileManager {
    /// The running Hako's instance socket.
    socket: PathBuf,
    /// The Hako to start when none runs.
    program: PathBuf,
}

impl FileManager {
    pub fn new(socket: PathBuf, program: PathBuf) -> FileManager {
        FileManager { socket, program }
    }

    fn show(&self, uris: Vec<String>, startup_id: &str, select: bool) -> fdo::Result<()> {
        // Validate the whole request before opening anything.
        let paths = uris
            .iter()
            .map(|u| {
                uri::path_of(u)
                    .filter(|p| !p.as_os_str().as_bytes().contains(&0))
                    .map(|p| names::normalize(&p))
                    .ok_or_else(|| fdo::Error::InvalidArgs("Hako needs local file:// URIs".into()))
            })
            .collect::<fdo::Result<Vec<_>>>()?;
        if paths.is_empty() {
            return Ok(());
        }
        // Hako gives items in one folder a single window.
        let windows = if select {
            paths.iter().map(|p| p.parent().unwrap_or(p)).collect::<HashSet<&Path>>().len()
        } else {
            paths.len()
        };
        if windows > MAX_WINDOWS {
            return Err(fdo::Error::InvalidArgs(format!("Hako opens at most {MAX_WINDOWS} folders at a time")));
        }
        let token = Some(startup_id).filter(|t| !t.is_empty());
        let requests: Vec<Request> =
            paths.iter().map(|p| if select { Request::Reveal(p.clone()) } else { Request::Open(p.clone()) }).collect();
        // A running Hako takes the requests directly, with the caller's
        // permission to take focus.
        if instance::hand_over(&self.socket, &requests, token).is_ok() {
            return Ok(());
        }
        // Otherwise start one as the Dock would, in an app scope of its
        // own, so it outlives this service and runs with apps' weight.
        // Canonical file URIs as separate arguments, never through a shell,
        // keep names that aren't UTF-8.
        let mut argv = vec![self.program.to_string_lossy().into_owned()];
        for p in &paths {
            if select {
                argv.push("--select".into());
            }
            argv.push(uri::file_uri(p));
        }
        let options = LaunchOptions { activation_token: token.map(str::to_owned), ..LaunchOptions::default() };
        let observer = LaunchObserver::new(|event| {
            if let LaunchEvent::Exited { status: Ok(status), .. } = event
                && !status.success()
            {
                log::warn!("Hako exited with {status}");
            }
        });
        launch_command_observed(&argv, Some(crate::APP_ID), &options, &observer)
            .map_err(|e| fdo::Error::Failed(format!("could not start Hako: {e}")))?;
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    fn show_items(&self, uris: Vec<String>, startup_id: &str) -> fdo::Result<()> {
        self.show(uris, startup_id, true)
    }

    fn show_folders(&self, uris: Vec<String>, startup_id: &str) -> fdo::Result<()> {
        self.show(uris, startup_id, false)
    }

    fn show_item_properties(&self, _uris: Vec<String>, _startup_id: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Open Get Info in Hako to view file properties".into()))
    }
}

/// Export the object before claiming the name. Don't queue behind another
/// provider or allow a file manager opened later to displace this session's
/// provider. An old provider may be replaced only if it permits replacement.
pub fn connect(builder: Builder<'_>, manager: FileManager) -> zbus::Result<Connection> {
    builder.serve_at(OBJECT_PATH, manager)?.name(BUS_NAME)?.allow_name_replacements(false).build()
}

pub fn serve() -> Result<(), Box<dyn std::error::Error>> {
    // The Hako on PATH (the current system's), as the Dock starts it: this
    // service's own build is older after an update until the next login.
    let program = match find_program("aizome-hako") {
        Some(_) => PathBuf::from("aizome-hako"),
        None => std::env::current_exe()?,
    };
    let connection = match connect(Builder::session()?, FileManager::new(instance::socket_path(), program)) {
        Err(zbus::Error::NameTaken) => return Err(format!("another file manager owns {BUS_NAME}").into()),
        connection => connection?,
    };
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
    // Keep serving until the session bus closes. systemd stops this process
    // with aizome-session.target, releasing the name on logout.
    if bus.receive_name_lost()?.next().is_some() {
        return Err("the file manager's bus name was lost".into());
    }
    Err("the session bus closed".into())
}
