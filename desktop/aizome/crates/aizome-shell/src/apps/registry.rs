//! The apps connected to the shell socket and the menus they published,
//! keyed by process ID (the menu bar shows the menus of the app whose
//! window has focus).

use std::collections::BTreeMap;

use aizome_shell_proto as proto;

/// A connection to the shell socket, while it's open.
pub type ConnId = u64;

/// An app that said hello.
#[derive(Clone, Debug, PartialEq)]
pub struct AppConn {
    pub conn: ConnId,
    pub app_id: String,
    pub pid: u32,
    pub name: String,
    pub menus: Vec<proto::Menu>,
    /// Bumped whenever its menus change.
    pub generation: u64,
}

/// Every app connected.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    conns: BTreeMap<ConnId, AppConn>,
}

/// What a message changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryChange {
    Nothing,
    /// An app came or went: its pid.
    App(u32),
    /// An app's menus changed: its pid.
    Menus(u32),
}

impl Registry {
    /// Take in a message from connection `conn`.
    pub fn take(&mut self, conn: ConnId, msg: &proto::AppMessage) -> RegistryChange {
        match msg {
            proto::AppMessage::Hello { app_id, pid, name } => {
                self.conns.insert(
                    conn,
                    AppConn {
                        conn,
                        app_id: app_id.clone(),
                        pid: *pid,
                        name: name.clone(),
                        menus: Vec::new(),
                        generation: 0,
                    },
                );
                RegistryChange::App(*pid)
            }
            proto::AppMessage::Menus { menus } => match self.conns.get_mut(&conn) {
                Some(a) if a.menus != *menus => {
                    a.menus = menus.clone();
                    a.generation += 1;
                    RegistryChange::Menus(a.pid)
                }
                Some(_) => RegistryChange::Nothing,
                None => {
                    log::debug!("menus from connection {conn} before its hello");
                    RegistryChange::Nothing
                }
            },
            _ => RegistryChange::Nothing,
        }
    }

    /// Connection `conn` closed.
    pub fn gone(&mut self, conn: ConnId) -> RegistryChange {
        match self.conns.remove(&conn) {
            Some(a) => RegistryChange::App(a.pid),
            None => RegistryChange::Nothing,
        }
    }

    pub fn by_pid(&self, pid: u32) -> Option<&AppConn> {
        // The latest connection of a pid wins (an app that reconnected).
        self.conns.values().rev().find(|a| a.pid == pid)
    }

    pub fn by_conn(&self, conn: ConnId) -> Option<&AppConn> {
        self.conns.get(&conn)
    }

    /// The app for a window: by its process, else by its app id.
    pub fn for_window(&self, pid: Option<i32>, app_id: &str) -> Option<&AppConn> {
        pid.and_then(|p| u32::try_from(p).ok())
            .and_then(|p| self.by_pid(p))
            .or_else(|| self.conns.values().rev().find(|a| !app_id.is_empty() && a.app_id == app_id))
    }

    pub fn apps(&self) -> impl Iterator<Item = &AppConn> {
        self.conns.values()
    }

    pub fn len(&self) -> usize {
        self.conns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(pid: u32, app: &str) -> proto::AppMessage {
        proto::AppMessage::Hello { app_id: app.into(), pid, name: app.rsplit('.').next().unwrap().into() }
    }

    fn menus(title: &str) -> proto::AppMessage {
        proto::AppMessage::Menus { menus: vec![proto::Menu { id: 1, title: title.into(), items: vec![] }] }
    }

    #[test]
    fn apps_are_found_by_pid_then_app_id() {
        let mut r = Registry::default();
        assert_eq!(r.take(1, &hello(100, "org.aizome.Sumi")), RegistryChange::App(100));
        assert_eq!(r.take(1, &menus("Sumi")), RegistryChange::Menus(100));
        assert_eq!(r.take(1, &menus("Sumi")), RegistryChange::Nothing);
        assert_eq!(r.by_pid(100).unwrap().menus[0].title, "Sumi");
        assert_eq!(r.for_window(Some(100), "").unwrap().app_id, "org.aizome.Sumi");
        // A sandboxed window's pid may not be the app's.
        assert_eq!(r.for_window(Some(7), "org.aizome.Sumi").unwrap().pid, 100);
        assert!(r.for_window(Some(7), "foot").is_none());
    }

    #[test]
    fn menus_before_hello_and_goodbyes() {
        let mut r = Registry::default();
        assert_eq!(r.take(3, &menus("X")), RegistryChange::Nothing);
        r.take(3, &hello(5, "a"));
        assert_eq!(r.gone(3), RegistryChange::App(5));
        assert_eq!(r.gone(3), RegistryChange::Nothing);
        assert!(r.is_empty());
    }

    #[test]
    fn a_reconnected_app_replaces_its_old_self() {
        let mut r = Registry::default();
        r.take(1, &hello(9, "a"));
        r.take(1, &menus("Old"));
        r.take(2, &hello(9, "a"));
        r.take(2, &menus("New"));
        assert_eq!(r.by_pid(9).unwrap().menus[0].title, "New");
    }
}
