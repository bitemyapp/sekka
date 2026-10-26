//! The accounts (AccountsService), live or sampled, and who is using the
//! computer: the sidebar's card falls back to `/etc/passwd` when
//! AccountsService isn't running.

use std::sync::Arc;

use aizome_sys::users::{AccountType, DeleteUser, PasswordMode, User, Users, UsersState};
use aizome_sys::{Error, Task, Wake};

use super::Cell;

pub enum Accounts {
    /// Not started (the listing, the first view).
    None,
    Live(Users),
    Sample(Cell<UsersState>),
}

/// The person at the computer, for the sidebar's card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Me {
    pub login: String,
    pub real_name: String,
    pub picture: Option<String>,
    pub admin: bool,
}

impl Me {
    pub fn display_name(&self) -> &str {
        if self.real_name.trim().is_empty() { &self.login } else { &self.real_name }
    }

    pub fn initials(&self) -> String {
        let u = User { real_name: self.real_name.clone(), name: self.login.clone(), ..User::default() };
        u.initials()
    }
}

impl Accounts {
    pub fn empty() -> Accounts {
        Accounts::None
    }

    pub fn live(wake: Wake) -> Accounts {
        Accounts::Live(Users::new(wake))
    }

    pub fn sample(wake: Wake) -> Accounts {
        Accounts::Sample(Cell::new(sample_users(), wake))
    }

    pub fn snapshot(&self) -> Arc<UsersState> {
        match self {
            Accounts::None => Arc::new(UsersState::default()),
            Accounts::Live(u) => u.snapshot(),
            Accounts::Sample(c) => c.get(),
        }
    }

    /// Who's using the computer: AccountsService's account, else the
    /// password database's.
    pub fn me(&self) -> Me {
        let s = self.snapshot();
        if let Some(u) = s.current() {
            return Me {
                login: u.name.clone(),
                real_name: u.real_name.clone(),
                picture: u.icon_file.clone(),
                admin: u.account_type == AccountType::Administrator,
            };
        }
        if matches!(self, Accounts::Sample(_)) {
            return Me::default();
        }
        passwd_me()
    }

    fn sample_update(&self, name: &str, f: impl FnOnce(&mut User)) -> Task<()> {
        match self {
            Accounts::Sample(c) => {
                let name = name.to_owned();
                c.update(move |s| {
                    if let Some(u) = s.users.iter_mut().find(|u| u.name == name) {
                        f(u);
                    }
                })
            }
            _ => Task::ready(Err(Error::Unavailable("AccountsService".into()))),
        }
    }

    pub fn set_real_name(&self, user: &User, name: &str) -> Task<()> {
        match self {
            Accounts::Live(u) => u.set_real_name(user, name),
            _ => {
                let n = name.to_owned();
                self.sample_update(&user.name, move |u| u.real_name = n)
            }
        }
    }

    pub fn set_icon(&self, user: &User, file: &std::path::Path) -> Task<()> {
        match self {
            Accounts::Live(u) => u.set_icon(user, file),
            _ => {
                let f = file.display().to_string();
                self.sample_update(&user.name, move |u| u.icon_file = Some(f))
            }
        }
    }

    pub fn set_account_type(&self, user: &User, t: AccountType) -> Task<()> {
        match self {
            Accounts::Live(u) => u.set_account_type(user, t),
            _ => self.sample_update(&user.name, move |u| u.account_type = t),
        }
    }

    /// Ask for a new password at the account's next login.
    pub fn require_password_change(&self, user: &User) -> Task<()> {
        match self {
            Accounts::Live(u) => u.require_password_change(user),
            _ => self.sample_update(&user.name, |u| u.password_mode = PasswordMode::SetAtLogin),
        }
    }

    pub fn set_automatic_login(&self, user: &User, on: bool) -> Task<()> {
        match self {
            Accounts::Live(u) => u.set_automatic_login(user, on),
            Accounts::Sample(c) => {
                let name = user.name.clone();
                // Only one account logs in by itself.
                c.update(move |s| {
                    for u in &mut s.users {
                        u.automatic_login = on && u.name == name;
                    }
                })
            }
            Accounts::None => Task::ready(Err(Error::Unavailable("AccountsService".into()))),
        }
    }

    pub fn set_password(&self, user: &User, password: &str, hint: &str) -> Task<()> {
        match self {
            Accounts::Live(u) => u.set_password(user, password, hint),
            _ if password.is_empty() => Task::ready(Err(Error::InvalidArgument("The password is empty".into()))),
            _ => self.sample_update(&user.name, |u| u.password_mode = PasswordMode::Regular),
        }
    }

    pub fn create_user(&self, name: &str, real_name: &str, t: AccountType) -> Task<String> {
        match self {
            Accounts::Live(u) => u.create_user(name, real_name, t),
            Accounts::Sample(c) => {
                if !aizome_sys::users::valid_user_name(name) {
                    return Task::ready(Err(Error::InvalidArgument(format!(
                        "A user name is lowercase letters, digits, '_' and '-', starting with a letter: {name}"
                    ))));
                }
                if c.get().user(name).is_some() {
                    return Task::ready(Err(Error::InvalidArgument(format!("{name} already exists"))));
                }
                let (n, r) = (name.to_owned(), real_name.to_owned());
                let path = format!("/org/freedesktop/Accounts/User{}", 1100 + c.get().users.len());
                let p2 = path.clone();
                let _ = c.update(move |s| {
                    let uid = s.users.iter().map(|u| u.uid).max().unwrap_or(1000) + 1;
                    s.users.push(User {
                        path: p2,
                        uid,
                        home: format!("/home/{n}"),
                        name: n,
                        real_name: r,
                        account_type: t,
                        shell: "/run/current-system/sw/bin/fish".into(),
                        password_mode: PasswordMode::SetAtLogin,
                        local_account: true,
                        ..User::default()
                    });
                });
                Task::ready(Ok(path))
            }
            Accounts::None => Task::ready(Err(Error::Unavailable("AccountsService".into()))),
        }
    }

    pub fn delete_user(&self, user: &User, remove_files: bool) -> Task<()> {
        match self {
            Accounts::Live(u) => u.delete_user(DeleteUser::confirm(user, remove_files)),
            Accounts::Sample(c) => {
                if user.uid == c.get().current_uid {
                    return Task::ready(Err(Error::InvalidArgument("You can't delete your own account".into())));
                }
                let uid = user.uid;
                c.update(move |s| s.users.retain(|u| u.uid != uid))
            }
            Accounts::None => Task::ready(Err(Error::Unavailable("AccountsService".into()))),
        }
    }
}

/// The sample computer's accounts.
pub fn sample_users() -> UsersState {
    let user = |uid: u64, name: &str, real: &str, admin: bool| User {
        path: format!("/org/freedesktop/Accounts/User{uid}"),
        uid,
        name: name.into(),
        real_name: real.into(),
        icon_file: None,
        account_type: if admin { AccountType::Administrator } else { AccountType::Standard },
        home: format!("/home/{name}"),
        shell: "/run/current-system/sw/bin/fish".into(),
        email: String::new(),
        language: "en_GB.UTF-8".into(),
        automatic_login: false,
        locked: false,
        password_mode: PasswordMode::Regular,
        last_login: None,
        login_count: 12,
        system_account: false,
        local_account: true,
    };
    UsersState {
        available: true,
        users: vec![
            user(1000, "aiko", "Aiko Mori", true),
            user(1001, "ren", "Ren Ishida", false),
            user(1002, "guest", "Guest", false),
        ],
        current_uid: 1000,
        users_managed: false,
    }
}

/// The password database's entry for this process's user.
fn passwd_me() -> Me {
    // SAFETY: getpwuid_r writes only into the buffers given.
    unsafe {
        let uid = libc::getuid();
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) != 0 || out.is_null() {
            return Me { login: std::env::var("USER").unwrap_or_default(), ..Me::default() };
        }
        let s = |p: *const libc::c_char| {
            if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        let gecos = s(pw.pw_gecos);
        Me {
            login: s(pw.pw_name),
            real_name: gecos.split(',').next().unwrap_or("").to_owned(),
            picture: None,
            admin: false,
        }
    }
}
