//! The one error type every service returns.
//!
//! Errors are plain data (cloneable, comparable) so a [`Task`](crate::Task)'s
//! result can be inspected more than once and shown in the UI. Each variant
//! says what the person can do about it: start a service, ask an
//! administrator, change the system configuration, try again.

use std::fmt;

/// What went wrong with a system service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The service's daemon is not running or not installed (no
    /// NetworkManager, no BlueZ, no session bus).
    Unavailable(String),
    /// The system configuration pins this setting, so it can't be changed
    /// here. On NixOS this is a module option, such as `time.timeZone` or
    /// `i18n.defaultLocale`; the message names it when it can.
    ManagedBySystem(String),
    /// Polkit or the daemon refused: not allowed, or authentication failed.
    NotAuthorized(String),
    /// The daemon did not answer in time.
    Timeout,
    /// Someone cancelled: dismissed an authentication dialog, or called
    /// `cancel` on an operation.
    Cancelled,
    /// An argument is not acceptable: an empty network name, a volume out
    /// of range, a path that is not a device.
    InvalidArgument(String),
    /// The other side refused: a wrong Wi-Fi password, a Bluetooth device
    /// that declined pairing, a wrong PIN.
    Rejected(String),
    /// The object is gone or never existed (a device unplugged, a player
    /// that quit).
    NotFound(String),
    /// A D-Bus error reply that none of the variants above describes.
    DBus { name: String, message: String },
    /// A command failed: what ran, and what it said.
    Command { program: String, message: String },
    /// Reading or writing a file failed.
    Io(String),
    /// Anything else, described.
    Failed(String),
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// The error for a D-Bus error reply, sorted into the variants above by
    /// its name: access denials become [`Error::NotAuthorized`], missing
    /// services [`Error::Unavailable`], NixOS's "managed by the
    /// configuration" refusals [`Error::ManagedBySystem`].
    pub fn from_dbus(name: &str, message: &str) -> Error {
        let message = message.trim().to_owned();
        let lower = message.to_ascii_lowercase();
        match name {
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.Disconnected"
            | "org.freedesktop.DBus.Error.Spawn.ChildExited"
            | "org.freedesktop.DBus.Error.Spawn.ExecFailed"
            | "org.freedesktop.DBus.Error.Spawn.ServiceNotFound" => Error::Unavailable(message),
            "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => Error::Timeout,
            "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownProperty"
            | "org.freedesktop.NetworkManager.UnknownConnection"
            | "org.freedesktop.NetworkManager.UnknownDevice"
            | "org.freedesktop.NetworkManager.Settings.Connection.NotFound"
            | "org.bluez.Error.DoesNotExist"
            | "org.freedesktop.Accounts.Error.UserDoesNotExist" => Error::NotFound(message),
            "org.freedesktop.DBus.Error.InvalidArgs"
            | "org.bluez.Error.InvalidArguments"
            | "org.freedesktop.NetworkManager.Settings.Connection.InvalidProperty"
            | "org.freedesktop.NetworkManager.Settings.InvalidConnection" => Error::InvalidArgument(message),
            "org.bluez.Error.Canceled"
            | "org.bluez.Error.AuthenticationCanceled"
            | "org.freedesktop.UDisks2.Error.Cancelled"
            | "org.freedesktop.UDisks2.Error.NotAuthorizedDismissed"
            | "org.freedesktop.PolicyKit1.Error.Cancelled" => Error::Cancelled,
            _ if lower.contains("nixos") => Error::ManagedBySystem(message),
            "org.freedesktop.DBus.Error.AccessDenied"
            | "org.freedesktop.DBus.Error.AuthFailed"
            | "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
            | "org.freedesktop.PolicyKit1.Error.NotAuthorized"
            | "org.freedesktop.NetworkManager.PermissionDenied"
            | "org.freedesktop.NetworkManager.Settings.PermissionDenied"
            | "org.freedesktop.NetworkManager.AgentManager.PermissionDenied"
            | "org.freedesktop.UDisks2.Error.NotAuthorized"
            | "org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain"
            | "org.freedesktop.Accounts.Error.PermissionDenied"
            | "org.bluez.Error.NotAuthorized" => Error::NotAuthorized(message),
            "org.bluez.Error.AuthenticationFailed" | "org.bluez.Error.AuthenticationRejected" => {
                Error::Rejected(message)
            }
            "System.Error.EROFS" => Error::ManagedBySystem(message),
            "System.Error.EPERM" | "System.Error.EACCES" => Error::NotAuthorized(message),
            "org.freedesktop.DBus.Error.FileExists" | "org.freedesktop.DBus.Error.NotSupported"
                if lower.contains("read-only") || lower.contains("read only") =>
            {
                Error::ManagedBySystem(message)
            }
            _ if lower.contains("read-only file system") => Error::ManagedBySystem(message),
            _ => Error::DBus { name: name.to_owned(), message },
        }
    }

    /// A short sentence for the UI.
    pub fn describe(&self) -> String {
        match self {
            Error::Unavailable(what) if what.is_empty() => "The service is not running".into(),
            Error::Unavailable(what) => format!("Not available: {what}"),
            Error::ManagedBySystem(what) if what.is_empty() => "Managed by the system configuration".into(),
            Error::ManagedBySystem(what) => format!("Managed by the system configuration ({what})"),
            Error::NotAuthorized(what) if what.is_empty() => "Not allowed".into(),
            Error::NotAuthorized(what) => format!("Not allowed: {what}"),
            Error::Timeout => "The service did not answer in time".into(),
            Error::Cancelled => "Cancelled".into(),
            Error::InvalidArgument(what) => what.clone(),
            Error::Rejected(what) if what.is_empty() => "Refused".into(),
            Error::Rejected(what) => what.clone(),
            Error::NotFound(what) if what.is_empty() => "No longer there".into(),
            Error::NotFound(what) => format!("Not found: {what}"),
            Error::DBus { message, name } if message.is_empty() => name.clone(),
            Error::DBus { message, .. } => message.clone(),
            Error::Command { program, message } if message.is_empty() => {
                format!("{program} failed")
            }
            Error::Command { program, message } => format!("{program}: {message}"),
            Error::Io(what) | Error::Failed(what) => what.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

impl std::error::Error for Error {}

impl From<zbus::Error> for Error {
    fn from(error: zbus::Error) -> Error {
        match error {
            zbus::Error::MethodError(name, message, _) => {
                Error::from_dbus(name.as_str(), message.as_deref().unwrap_or(""))
            }
            zbus::Error::FDO(fdo) => Error::from(*fdo),
            zbus::Error::InputOutput(io) => Error::Io(io.to_string()),
            zbus::Error::InterfaceNotFound | zbus::Error::Unsupported => Error::Unavailable(error.to_string()),
            zbus::Error::Variant(variant) => Error::Failed(format!("unexpected data from the service: {variant}")),
            other => Error::Failed(other.to_string()),
        }
    }
}

impl From<zbus::fdo::Error> for Error {
    fn from(error: zbus::fdo::Error) -> Error {
        use zbus::DBusError;
        match error {
            zbus::fdo::Error::ZBus(error) => Error::from(error),
            error => Error::from_dbus(error.name().as_str(), error.description().unwrap_or("")),
        }
    }
}

impl From<zbus::zvariant::Error> for Error {
    fn from(error: zbus::zvariant::Error) -> Error {
        Error::Failed(format!("unexpected data from the service: {error}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Error::NotAuthorized(error.to_string()),
            std::io::ErrorKind::NotFound => Error::NotFound(error.to_string()),
            std::io::ErrorKind::TimedOut => Error::Timeout,
            std::io::ErrorKind::ReadOnlyFilesystem => Error::ManagedBySystem(error.to_string()),
            _ => Error::Io(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_errors_are_sorted_by_what_they_mean() {
        assert!(matches!(Error::from_dbus("org.freedesktop.DBus.Error.ServiceUnknown", "x"), Error::Unavailable(_)));
        assert!(matches!(
            Error::from_dbus("org.freedesktop.PolicyKit1.Error.NotAuthorized", "no"),
            Error::NotAuthorized(_)
        ));
        assert_eq!(Error::from_dbus("org.freedesktop.UDisks2.Error.NotAuthorizedDismissed", ""), Error::Cancelled);
        // NixOS's patched timedated and localed refuse with NotSupported
        // and a message naming NixOS.
        assert!(matches!(
            Error::from_dbus(
                "org.freedesktop.DBus.Error.NotSupported",
                "Changing system settings via systemd is not supported on NixOS."
            ),
            Error::ManagedBySystem(_)
        ));
        assert!(matches!(Error::from_dbus("org.example.Odd", "Something"), Error::DBus { .. }));
    }
}
