//! The standard "show in folder" interface used by browsers and the
//! OpenURI portal. The Aizome session owns it even with no Files window.
//! No global D-Bus activation entry: other desktops keep their provider.

use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;

use zbus::blocking::{Connection, connection::Builder};
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};

use crate::model::uri;

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

pub struct FileManager {
    executable: PathBuf,
}

impl FileManager {
    fn launch(&self, uris: Vec<String>, startup_id: &str, select: bool) -> fdo::Result<()> {
        // Validate the whole request before opening anything. Pass canonical
        // file URIs as individual argv entries, never through a shell; this
        // also preserves Unix filenames that aren't UTF-8.
        let uris = uris
            .iter()
            .map(|u| {
                let path = uri::path_of(u)
                    .filter(|p| !p.as_os_str().as_bytes().contains(&0))
                    .ok_or_else(|| fdo::Error::InvalidArgs("Hako needs local file:// URIs".into()))?;
                Ok(uri::file_uri(&path))
            })
            .collect::<fdo::Result<Vec<_>>>()?;
        if uris.is_empty() {
            return Ok(());
        }
        let mut command = Command::new(&self.executable);
        for uri in uris {
            if select {
                command.arg("--select");
            }
            command.arg(uri);
        }
        if !startup_id.is_empty() {
            command.env("DESKTOP_STARTUP_ID", startup_id);
            // OpenURI passes its Wayland activation token in StartupId.
            command.env("XDG_ACTIVATION_TOKEN", startup_id);
        }
        let mut child = command.spawn().map_err(|e| fdo::Error::Failed(format!("could not start Hako: {e}")))?;
        // A new window may live for hours; an existing instance returns at
        // once. Neither holds up the bus, and both are reaped when done.
        std::thread::spawn(move || {
            if let Ok(status) = child.wait()
                && !status.success()
            {
                log::warn!("Hako exited with {status}");
            }
        });
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    fn show_items(&self, uris: Vec<String>, startup_id: &str) -> fdo::Result<()> {
        self.launch(uris, startup_id, true)
    }

    fn show_folders(&self, uris: Vec<String>, startup_id: &str) -> fdo::Result<()> {
        self.launch(uris, startup_id, false)
    }

    fn show_item_properties(&self, _uris: Vec<String>, _startup_id: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Open Get Info in Hako to view file properties".into()))
    }
}

/// Export the object before claiming the name. Don't queue behind another
/// provider or allow a file manager opened later to displace this session's
/// provider. An old provider may be replaced only if it permits replacement.
pub fn connect(builder: Builder<'_>, executable: PathBuf) -> zbus::Result<Connection> {
    let connection = builder.serve_at(OBJECT_PATH, FileManager { executable })?.build()?;
    let reply = connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting)?;
    if reply != RequestNameReply::PrimaryOwner {
        return Err(zbus::Error::Failure("another file manager owns the session bus name".into()));
    }
    Ok(connection)
}

pub fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let connection = connect(Builder::session()?, std::env::current_exe()?)?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
    // Keep serving until the session bus closes. systemd stops this process
    // with aizome-session.target, releasing the name on logout.
    if bus.receive_name_lost()?.next().is_some() {
        return Err("the file manager's bus name was lost".into());
    }
    Err("the session bus closed".into())
}
