//! Users & Groups: your name, picture and password; the other accounts,
//! administrator or standard, added and removed; who logs in by
//! themselves. Through AccountsService (polkit asks through the shell's
//! agent); accounts the system configuration fixes
//! (`users.mutableUsers = false`) say so.
//!
//! Who logs in by themselves is the system's own choice on Sekka
//! (`calamares.autoLogin`), as every login screen it offers reads it from
//! the configuration: Yukimi makes the change, from the next start.
//! Elsewhere on NixOS, and under greetd anywhere, only the configuration
//! sets it; AccountsService's flag, which GDM reads, is for other systems.

use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{TimerHandle, column, row};
use aizome_sys::users::{AccountType, PasswordMode, User};

use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::system::choices;
use crate::widgets::avatar;

/// The setting Sekka declares for who logs in by themselves.
pub const AUTO_LOGIN: &str = "calamares.autoLogin";

/// How this computer comes to log in by itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AutoLogin {
    /// Sekka's choice, which Yukimi changes: the account now and from the
    /// next start (empty for no one).
    Choice(choices::Now<String>),
    /// AccountsService's flag, which GDM reads (on systems other than
    /// NixOS).
    #[default]
    Accounts,
    /// Only the system's configuration sets it
    /// (`services.displayManager.autoLogin`): whom greetd's initial session
    /// is, when greetd says. Otherwise AccountsService reports GDM's.
    Configuration(Option<String>),
}

/// How this computer logs in by itself.
pub fn auto_login() -> AutoLogin {
    if let Some(now) = choices::read(AUTO_LOGIN) {
        return AutoLogin::Choice(now);
    }
    let greetd = aizome_sys::Bus::system().and_then(|bus| aizome_sys::users::greetd_config(&bus)).ok().flatten();
    match greetd {
        Some(path) => {
            AutoLogin::Configuration(std::fs::read_to_string(path).ok().and_then(|t| greetd_initial_user(&t)))
        }
        // NixOS writes GDM's custom.conf itself, read-only, and SDDM and
        // Plasma Login never read AccountsService.
        None if aizome_sys::sysinfo::os_release().id == "nixos" => AutoLogin::Configuration(None),
        None => AutoLogin::Accounts,
    }
}

/// The user of greetd's initial session (`[initial_session]`, `user = "…"`).
pub fn greetd_initial_user(config: &str) -> Option<String> {
    let config: toml::Table = toml::from_str(config).ok()?;
    config.get("initial_session")?.get("user")?.as_str().filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Who may be chosen to log in by themselves, by account and as shown:
/// everyone AccountsService lists (under Sekka's choice, those Yukimi
/// takes), and whoever the choice or the configuration names besides.
fn auto_login_people(how: &AutoLogin, users: &[User]) -> Vec<(String, String)> {
    let choice = matches!(how, AutoLogin::Choice(_));
    let mut people: Vec<(String, String)> = users
        .iter()
        .filter(|u| !choice || yukimi_takes(u))
        .map(|u| (u.name.clone(), u.display_name().to_owned()))
        .collect();
    let named: Vec<&String> = match how {
        AutoLogin::Choice(c) => vec![&c.now, &c.next],
        AutoLogin::Configuration(Some(who)) => vec![who],
        _ => vec![],
    };
    for name in named {
        if !name.is_empty() && !people.iter().any(|(n, _)| n == name) {
            people.push((name.clone(), name.clone()));
        }
    }
    people
}

/// Whether Yukimi takes `u` to log in by themselves, by the rules of its
/// `people` module (`yukimi_system::people`, from `/etc/passwd`): a local
/// account from uid 1000 to 59999 with a `useradd`-style name and a shell to
/// log in with (none given is `/bin/sh`).
fn yukimi_takes(u: &User) -> bool {
    let shell = if u.shell.is_empty() { "sh" } else { u.shell.rsplit('/').next().unwrap_or("") };
    u.local_account
        && (1000..60000).contains(&u.uid)
        && aizome_sys::users::valid_user_name(&u.name)
        && !shell.is_empty()
        && !matches!(shell, "nologin" | "false")
}

/// What the login row says under Sekka's choice: whom Settei proposed in
/// Yukimi while that waits to be applied, who it is from the next start
/// while a change waits for one, and what it does.
fn choice_help(c: &choices::Now<String>, proposed: Option<&str>, people: &[(String, String)]) -> String {
    let what = "The computer starts straight into this account, without asking for a password. Yukimi makes the \
                change, which asks for an administrator password, and it applies from the next start.";
    let shown = |name: &str| -> String {
        if name.is_empty() {
            return "no one".to_owned();
        }
        people.iter().find(|(n, _)| n == name).map_or(name, |(_, shown)| shown.as_str()).to_owned()
    };
    let mut lines = Vec::new();
    if let Some(p) = proposed.filter(|p| *p != c.next) {
        lines.push(format!("Proposed in Yukimi: {}. Apply there to make the change.", shown(p)));
    }
    if c.now != c.next {
        lines.push(format!("From the next start: {}.", shown(&c.next)));
    }
    lines.push(what.to_owned());
    lines.join(" ")
}

/// What the sheet asks.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Sheet {
    Password {
        user: String,
        new: String,
        again: String,
        hint: String,
        error: Option<String>,
    },
    Add {
        full: String,
        login: String,
        login_edited: bool,
        admin: bool,
        password: String,
        error: Option<String>,
    },
    /// Remove `user`, whose home folder is `home` (AccountsService's).
    Remove {
        user: String,
        home: String,
        delete_files: bool,
    },
}

#[derive(Default)]
pub struct Users {
    sheet: Option<Sheet>,
    name_draft: Option<String>,
    pictures: crate::widgets::Pictures,
    /// A password to set once a new account appears.
    pending_password: Option<(String, String)>,
    /// How the computer logs in by itself; `None` until read, when it
    /// can't be changed.
    auto_login: Option<AutoLogin>,
    /// Whom Settei proposed in Yukimi ("" for no one), until the next
    /// start's choice shows it or the pane is left.
    proposed: Option<String>,
    /// Reads the choice again meanwhile.
    checking: Option<TimerHandle>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Name(String),
    SaveName,
    ChoosePicture,
    Picture(Option<Vec<PathBuf>>),
    ChangePassword(String),
    New(String),
    Again(String),
    Hint(String),
    SavePassword,
    Cancel,
    AddUser,
    Full(String),
    Login(String),
    Admin(bool),
    FirstPassword(String),
    Create,
    Created(Result<String, String>),
    Type(String, bool),
    TypeConfirmed(String, bool),
    AutoLogin(usize),
    AutoLoginConfirmed(Option<String>),
    AutoLoginRead(AutoLogin),
    AutoLoginCheck,
    Remove(String),
    DeleteFiles(bool),
    RemoveConfirmed,
    Removed(String, Result<(), String>),
    Done(&'static str, Result<(), String>),
}

/// A login name from a full name: "Aiko Mori" → "aiko".
pub fn login_from(full: &str) -> String {
    let first = full.split_whitespace().next().unwrap_or("");
    let s: String = first.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    match s.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => s,
        Some(_) => format!("u{s}"),
        None => String::new(),
    }
}

/// The groups the account `login` is in, by name, from the group database
/// (not this process's groups, which a sandbox may change).
pub fn groups_of(login: &str) -> Vec<String> {
    let Ok(name) = std::ffi::CString::new(login) else { return Vec::new() };
    // SAFETY: getpwnam_r, getgrouplist and getgrgid_r write only into the
    // buffers given.
    unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut pbuf = vec![0 as libc::c_char; 4096];
        let mut pout: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwnam_r(name.as_ptr(), &mut pw, pbuf.as_mut_ptr(), pbuf.len(), &mut pout) != 0 || pout.is_null() {
            return Vec::new();
        }
        let mut n: libc::c_int = 64;
        let mut ids = vec![0 as libc::gid_t; n as usize];
        if libc::getgrouplist(name.as_ptr(), pw.pw_gid, ids.as_mut_ptr(), &mut n) < 0 {
            ids = vec![0 as libc::gid_t; n.max(0) as usize];
            if libc::getgrouplist(name.as_ptr(), pw.pw_gid, ids.as_mut_ptr(), &mut n) < 0 {
                return Vec::new();
            }
        }
        ids.truncate(n.max(0) as usize);
        let mut names = Vec::new();
        for id in ids {
            let mut gr: libc::group = std::mem::zeroed();
            let mut buf = vec![0 as libc::c_char; 4096];
            let mut out: *mut libc::group = std::ptr::null_mut();
            if libc::getgrgid_r(id, &mut gr, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
                names.push(std::ffi::CStr::from_ptr(gr.gr_name).to_string_lossy().into_owned());
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

/// The Remove sheet's checkbox: the user's own home folder, wherever it is.
fn remove_files_words(home: &str) -> String {
    if home.trim().is_empty() {
        "Also delete their home folder and everything in it".to_owned()
    } else {
        format!("Also delete {home} and everything in it")
    }
}

fn account_label(t: AccountType) -> &'static str {
    match t {
        AccountType::Administrator => "Administrator",
        AccountType::Standard => "Standard",
    }
}

impl Users {
    fn picture(&self, path: Option<&str>) -> Option<Image> {
        self.pictures.get(path?)
    }

    fn user(cx: &PaneCx<Msg>, name: &str) -> Option<User> {
        cx.sys().accounts.snapshot().user(name).cloned()
    }

    /// Propose `who` ("" for no one) in Yukimi, which shows the change and
    /// builds it once applied; read the choice again until the next
    /// start's shows it.
    fn propose_auto_login(&mut self, who: &str, cx: &mut PaneCx<Msg>) {
        let argv = choices::propose(AUTO_LOGIN, who);
        cx.launch(&argv.each_ref().map(String::as_str));
        self.proposed = Some(who.to_owned());
        if self.checking.is_none() && !cx.sample() {
            self.checking = Some(cx.every(Duration::from_secs(2), Msg::AutoLoginCheck));
        }
    }

    fn stop_checking(&mut self, cx: &mut PaneCx<Msg>) {
        self.proposed = None;
        if let Some(t) = self.checking.take() {
            cx.cancel(t);
        }
    }

    fn sheet_view(&self, sheet: &Sheet) -> Element<Msg> {
        let buttons = |label: &'static str, ok: bool, msg: Msg, destructive: bool| -> Element<Msg> {
            let b = button(label).default().enabled(ok).on_press(msg);
            let b = if destructive { b.destructive() } else { b.primary() };
            row![spacer(), button("Cancel").cancel().on_press(Msg::Cancel), b].spacing(8.0).width(Fill).into()
        };
        match sheet {
            Sheet::Password { user, new, again, hint, error } => column![
                text(format!("Change the password for {user}")).size(15.0).weight(650.0),
                form::note("A long password is better than a clever one: a few words you'll remember."),
                text_field(new)
                    .password()
                    .placeholder("New password")
                    .on_change(Msg::New)
                    .id("new-password")
                    .width(Fill),
                text_field(again)
                    .password()
                    .placeholder("The same again")
                    .on_change(Msg::Again)
                    .error(error.clone())
                    .width(Fill),
                text_field(hint).placeholder("A hint (optional)").on_change(Msg::Hint).width(Fill),
                buttons("Change Password", !new.is_empty(), Msg::SavePassword, false),
            ]
            .spacing(10.0)
            .width(400.0)
            .into(),
            Sheet::Add { full, login, admin, password, error, .. } => column![
                text("Add a User").size(15.0).weight(650.0),
                form::note("They'll have a home folder, settings and desktop of their own."),
                text_field(full).placeholder("Full name").on_change(Msg::Full).id("full-name").width(Fill),
                text_field(login)
                    .mono()
                    .placeholder("Account name")
                    .on_change(Msg::Login)
                    .error(error.clone())
                    .width(Fill),
                text_field(password)
                    .password()
                    .placeholder("Password, or they choose one at their first login")
                    .on_change(Msg::FirstPassword)
                    .width(Fill),
                checkbox("Administrator: can change the system and manage users", *admin).on_toggle(Msg::Admin),
                buttons("Add User", !full.trim().is_empty() && !login.is_empty(), Msg::Create, false),
            ]
            .spacing(10.0)
            .width(420.0)
            .into(),
            Sheet::Remove { user, home, delete_files } => {
                let starts_into = match &self.auto_login {
                    Some(AutoLogin::Choice(c)) if c.next == *user => Some(
                        "The computer starts straight into their account. Once they're removed, Yukimi offers to \
                         log no one in by themselves, which asks for an administrator password.",
                    ),
                    Some(AutoLogin::Configuration(Some(who))) if who == user => Some(
                        "The system's configuration starts the computer straight into their account \
                         (services.displayManager.autoLogin): change it there too.",
                    ),
                    _ => None,
                };
                let mut sheet = column![
                    text(format!("Remove {user}?")).size(15.0).weight(650.0),
                    form::note("They won't be able to log in any more."),
                ]
                .spacing(12.0)
                .width(420.0);
                if let Some(why) = starts_into {
                    sheet = sheet.push(form::note(why));
                }
                sheet
                    .push(checkbox(remove_files_words(home), *delete_files).on_toggle(Msg::DeleteFiles))
                    .push(buttons("Remove User", true, Msg::RemoveConfirmed, true))
                    .into()
            }
        }
    }
}

impl Pane for Users {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "users"
    }

    fn title(&self) -> &'static str {
        "Users & Groups"
    }

    fn icon(&self) -> Icon {
        Icon::Users
    }

    fn tile(&self) -> Tile {
        Tile::Hanada
    }

    fn group(&self) -> Group {
        Group::Security
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["accounts", "people", "login", "admin"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("name", "Your name").keywords(&["real name", "full name"]),
            SettingEntry::new("picture", "Your picture").keywords(&["avatar", "photo", "icon"]),
            SettingEntry::new("password", "Change password").keywords(&["passphrase", "login password"]),
            SettingEntry::new("groups", "Groups").keywords(&["wheel", "membership"]),
            SettingEntry::new("others", "Other users").keywords(&[
                "add user",
                "remove user",
                "administrator",
                "standard",
            ]),
            SettingEntry::new("auto-login", "Log in automatically").keywords(&[
                "automatic login",
                "autologin",
                "no password",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        // Read each time: a change Yukimi applied since moves it. Until the
        // first read the row can't change. (The sample keeps
        // AccountsService's.)
        if cx.sample() {
            self.auto_login.get_or_insert(AutoLogin::Accounts);
        } else {
            cx.spawn(|| Msg::AutoLoginRead(auto_login()));
        }
    }

    fn hidden(&mut self, cx: &mut PaneCx<Msg>) {
        self.stop_checking(cx);
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        let me = cx.sys().accounts.snapshot().current().cloned();
        match msg {
            Msg::Name(s) => self.name_draft = Some(s),
            Msg::SaveName => {
                let (Some(name), Some(me)) = (self.name_draft.take(), me) else { return };
                let task = cx.sys().accounts.set_real_name(&me, name.trim());
                cx.wait(task, |r| Msg::Done("Changed your name", r.map_err(|e| e.to_string())));
            }
            Msg::ChoosePicture => {
                let dialog = FileDialog::new()
                    .title("Choose a Picture")
                    .directory(aizome_config::home_dir().join("Pictures"))
                    .filter("Pictures", &["png", "jpg", "jpeg"]);
                cx.open_panel(dialog, Msg::Picture);
            }
            Msg::Picture(Some(paths)) => {
                let (Some(p), Some(me)) = (paths.into_iter().next(), me) else { return };
                let task = cx.sys().accounts.set_icon(&me, &p);
                cx.wait(task, |r| Msg::Done("Changed your picture", r.map_err(|e| e.to_string())));
            }
            Msg::Picture(None) => {}
            Msg::ChangePassword(user) => {
                self.sheet = Some(Sheet::Password {
                    user,
                    new: String::new(),
                    again: String::new(),
                    hint: String::new(),
                    error: None,
                });
                cx.open_sheet();
                cx.focus("new-password");
            }
            Msg::New(s) => {
                if let Some(Sheet::Password { new, error, .. }) = &mut self.sheet {
                    *new = s;
                    *error = None;
                }
            }
            Msg::Again(s) => {
                if let Some(Sheet::Password { again, error, .. }) = &mut self.sheet {
                    *again = s;
                    *error = None;
                }
            }
            Msg::Hint(s) => {
                if let Some(Sheet::Password { hint, .. }) = &mut self.sheet {
                    *hint = s;
                }
            }
            Msg::SavePassword => {
                let Some(Sheet::Password { user, new, again, hint, error }) = &mut self.sheet else { return };
                if new != again {
                    *error = Some("The two don't match".into());
                    return;
                }
                let Some(u) = Self::user(cx, user) else { return };
                let task = cx.sys().accounts.set_password(&u, new, hint);
                self.sheet = None;
                cx.close_sheet();
                cx.wait(task, |r| Msg::Done("Changed the password", r.map_err(|e| e.to_string())));
            }
            Msg::Cancel => {
                self.sheet = None;
                cx.close_sheet();
            }
            Msg::AddUser => {
                self.sheet = Some(Sheet::Add {
                    full: String::new(),
                    login: String::new(),
                    login_edited: false,
                    admin: false,
                    password: String::new(),
                    error: None,
                });
                cx.open_sheet();
                cx.focus("full-name");
            }
            Msg::Full(s) => {
                if let Some(Sheet::Add { full, login, login_edited, .. }) = &mut self.sheet {
                    if !*login_edited {
                        *login = login_from(&s);
                    }
                    *full = s;
                }
            }
            Msg::Login(s) => {
                if let Some(Sheet::Add { login, login_edited, error, .. }) = &mut self.sheet {
                    *login = s.to_lowercase();
                    *login_edited = true;
                    *error = None;
                }
            }
            Msg::Admin(on) => {
                if let Some(Sheet::Add { admin, .. }) = &mut self.sheet {
                    *admin = on;
                }
            }
            Msg::FirstPassword(s) => {
                if let Some(Sheet::Add { password, .. }) = &mut self.sheet {
                    *password = s;
                }
            }
            Msg::Create => {
                let Some(Sheet::Add { full, login, admin, password, error, .. }) = &mut self.sheet else { return };
                if !aizome_sys::users::valid_user_name(login) {
                    *error = Some("Lowercase letters, digits, - and _, starting with a letter".into());
                    return;
                }
                if cx.sys().accounts.snapshot().user(login).is_some() {
                    *error = Some(format!("{login} is taken"));
                    return;
                }
                let t = if *admin { AccountType::Administrator } else { AccountType::Standard };
                let task = cx.sys().accounts.create_user(login, full.trim(), t);
                self.pending_password = Some((login.clone(), password.clone()));
                self.sheet = None;
                cx.close_sheet();
                cx.wait(task, |r| Msg::Created(r.map_err(|e| e.to_string())));
            }
            Msg::Created(Ok(_)) => {
                cx.say("Added the user");
                self.system_changed(cx);
            }
            Msg::Created(Err(e)) => {
                self.pending_password = None;
                cx.failed("Add the user", e);
            }
            Msg::Type(user, admin) => {
                // The configuration owns the accounts: nothing to change here.
                let snap = cx.sys().accounts.snapshot();
                if snap.users_managed || !snap.available {
                    return;
                }
                let is_me = me.as_ref().is_some_and(|m| m.name == user);
                if !admin && is_me {
                    cx.confirm(
                        Confirm::new(
                            "Stop being an administrator?",
                            "You won't be able to change the system, install software for everyone or manage users, unless another administrator gives it back.",
                            "Make Me Standard",
                            Msg::TypeConfirmed(user, admin),
                        )
                        .destructive(),
                    );
                } else if admin {
                    cx.confirm(Confirm::new(
                        format!("Make {user} an administrator?"),
                        "Administrators can change anything on this computer, other people's accounts included.",
                        "Make Administrator",
                        Msg::TypeConfirmed(user, admin),
                    ));
                } else {
                    cx.send(Msg::TypeConfirmed(user, admin));
                }
            }
            Msg::TypeConfirmed(user, admin) => {
                let Some(u) = Self::user(cx, &user) else { return };
                let t = if admin { AccountType::Administrator } else { AccountType::Standard };
                let task = cx.sys().accounts.set_account_type(&u, t);
                cx.wait(task, |r| Msg::Done("Changed the account type", r.map_err(|e| e.to_string())));
            }
            Msg::AutoLogin(i) => {
                let Some(how) = &self.auto_login else { return };
                let people = auto_login_people(how, &cx.sys().accounts.snapshot().users);
                let who = if i == 0 { None } else { people.get(i - 1).map(|(name, _)| name.clone()) };
                match &who {
                    Some(name) => cx.confirm(Confirm::new(
                        format!("Log in as {name} without a password?"),
                        "Anyone who turns this computer on can use it as them. The lock screen still asks for the password after sleep.",
                        "Turn On",
                        Msg::AutoLoginConfirmed(who.clone()),
                    )),
                    None => cx.send(Msg::AutoLoginConfirmed(None)),
                }
            }
            Msg::AutoLoginConfirmed(who) if matches!(self.auto_login, Some(AutoLogin::Choice(_))) => {
                self.propose_auto_login(who.as_deref().unwrap_or(""), cx);
            }
            Msg::AutoLoginCheck => cx.spawn(|| Msg::AutoLoginRead(auto_login())),
            Msg::AutoLoginRead(how) => {
                // What was proposed has come, or the choice has gone.
                let waiting = match &how {
                    AutoLogin::Choice(c) => self.proposed.as_deref().is_some_and(|p| p != c.next),
                    _ => false,
                };
                if !waiting {
                    self.stop_checking(cx);
                }
                self.auto_login = Some(how);
            }
            Msg::AutoLoginConfirmed(who) if matches!(self.auto_login, Some(AutoLogin::Accounts)) => {
                let snap = cx.sys().accounts.snapshot();
                let target = match &who {
                    Some(n) => snap.user(n).cloned(),
                    None => snap.users.iter().find(|u| u.automatic_login).cloned(),
                };
                let Some(u) = target else { return };
                let task = cx.sys().accounts.set_automatic_login(&u, who.is_some());
                cx.wait(task, |r| Msg::Done("Automatic login", r.map_err(|e| e.to_string())));
            }
            // Not read yet, or only the configuration sets it.
            Msg::AutoLoginConfirmed(_) => {}
            Msg::Remove(user) => {
                let Some(home) = Self::user(cx, &user).map(|u| u.home.clone()) else { return };
                self.sheet = Some(Sheet::Remove { user, home, delete_files: false });
                cx.open_sheet();
            }
            Msg::DeleteFiles(on) => {
                if let Some(Sheet::Remove { delete_files, .. }) = &mut self.sheet {
                    *delete_files = on;
                }
            }
            Msg::RemoveConfirmed => {
                let Some(Sheet::Remove { user, delete_files, .. }) = self.sheet.take() else { return };
                cx.close_sheet();
                let Some(u) = Self::user(cx, &user) else { return };
                let task = cx.sys().accounts.delete_user(&u, delete_files);
                cx.wait(task, move |r| Msg::Removed(user, r.map_err(|e| e.to_string())));
            }
            Msg::Removed(user, Ok(())) => {
                cx.say("Removed the user");
                // Nobody should start into an account that's gone, nor into
                // one made later with its name.
                if let Some(AutoLogin::Choice(c)) = &self.auto_login
                    && c.next == user
                {
                    self.propose_auto_login("", cx);
                }
            }
            Msg::Removed(_, Err(e)) => cx.failed("Removed the user", e),
            Msg::Done(what, Ok(())) => cx.say(what),
            Msg::Done(what, Err(e)) => cx.failed(what, e),
        }
    }

    fn system_changed(&mut self, cx: &mut PaneCx<Msg>) {
        // A new account has come: give it the password chosen, or have it
        // choose one at its first login.
        let Some((login, password)) = self.pending_password.clone() else { return };
        let Some(u) = cx.sys().accounts.snapshot().user(&login).cloned() else { return };
        self.pending_password = None;
        let task = if password.is_empty() {
            if u.password_mode == PasswordMode::SetAtLogin {
                return;
            }
            cx.sys().accounts.require_password_change(&u)
        } else {
            cx.sys().accounts.set_password(&u, &password, "")
        };
        cx.wait(task, |r| Msg::Done("Set up the new user's password", r.map_err(|e| e.to_string())));
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let snap = vcx.sys().accounts.snapshot();
        let mut sections = Vec::new();
        if snap.users_managed {
            sections.push(vcx.section(
                "",
                vec![form::managed(
                    "Accounts",
                    "Fixed",
                    "They come from users.users with users.mutableUsers = false, so changes made here wouldn't last: change them in the configuration and rebuild.",
                )],
            ));
        }
        if !snap.available {
            sections.push(form::note(
                "AccountsService isn't running, so accounts can't be changed here (services.accounts-daemon.enable).",
            ));
        }
        let me = snap.current().cloned();
        let fixed = snap.users_managed || !snap.available;
        if let Some(me) = &me {
            let pic = self.picture(me.icon_file.as_deref());
            let name = self.name_draft.clone().unwrap_or_else(|| me.real_name.clone());
            let kind = badge(account_label(me.account_type)).role(if me.account_type == AccountType::Administrator {
                Role::Accent
            } else {
                Role::Text3
            });
            let hero = row![
                avatar(pic, &me.initials(), &me.name, 72.0),
                column![
                    text(me.display_name().to_owned()).size(20.0).weight(650.0),
                    text(me.name.clone()).mono().size(12.0).role(Role::Text3),
                    row![kind],
                ]
                .spacing(5.0),
            ]
            .spacing(16.0)
            .align(Align::Center);
            sections.push(container(hero).padding([4.0, 2.0]).into());
            let groups = if vcx.sample() {
                vec!["audio".to_owned(), "networkmanager".into(), "users".into(), "video".into(), "wheel".into()]
            } else {
                groups_of(&me.name)
            };
            sections.push(vcx.section(
                "You",
                vec![
                        form::row(
                            "Name",
                            text_field(&name)
                                .on_change(Msg::Name)
                                .on_submit(Msg::SaveName)
                                .on_blur(Msg::SaveName)
                                .enabled(!fixed)
                                .width(240.0),
                        )
                        .entry("name")
                        .help("How you appear at login, on the lock screen and in the sidebar."),
                        form::row("Picture", button("Choose…").enabled(!fixed).on_press(Msg::ChoosePicture))
                            .entry("picture"),
                        form::row(
                            "Password",
                            button("Change Password…")
                                .enabled(snap.available)
                                .on_press(Msg::ChangePassword(me.name.clone())),
                        )
                        .entry("password"),
                        form::row(
                            "Groups",
                            text(groups.join(", ")).mono().size(11.0).role(Role::Text3).wrap().max_width(300.0),
                        )
                        .entry("groups")
                        .help("wheel can act as administrator; the others reach devices."),
                    ],
            ));
        }

        // Other people.
        let others: Vec<&User> = snap.users.iter().filter(|u| Some(u.uid) != me.as_ref().map(|m| m.uid)).collect();
        let mut rows: Vec<form::Row<Msg>> = others
            .iter()
            .map(|u| {
                let pic = self.picture(u.icon_file.as_deref());
                let name = u.name.clone();
                let n2 = u.name.clone();
                let controls = row![
                    // Fixed by the configuration: shown, not changed.
                    dropdown(
                        ["Standard".to_owned(), "Administrator".to_owned()],
                        usize::from(u.account_type == AccountType::Administrator)
                    )
                    .on_select(move |i| Msg::Type(name.clone(), i == 1))
                    .enabled(!fixed)
                    .min_width(160.0),
                    icon_button(Icon::Trash).toolbar().enabled(!fixed).on_press(Msg::Remove(n2)).tooltip("Remove"),
                ]
                .spacing(8.0)
                .align(Align::Center);
                form::row(u.display_name().to_owned(), controls)
                    .leading(avatar(pic, &u.initials(), &u.name, 32.0))
                    .detail(u.name.clone())
            })
            .collect();
        if rows.is_empty() {
            rows.push(form::whole(
                container(text("Nobody else has an account here.").role(Role::Text3)).padding([12.0, form::INSET]),
            ));
        }
        let first = rows.remove(0);
        rows.insert(0, first.entry("others"));
        rows.push(form::whole(
            container(
                row![button("Add User…").icon(Icon::UserPlus).enabled(!fixed).on_press(Msg::AddUser), spacer()]
                    .width(Fill),
            )
            .padding([8.0, form::INSET])
            .width(Fill),
        ));
        sections.push(vcx.section("Other Users", rows));

        let people = auto_login_people(self.auto_login.as_ref().unwrap_or(&AutoLogin::Accounts), &snap.users);
        let mut names = vec!["Off".to_owned()];
        names.extend(people.iter().map(|(_, shown)| shown.clone()));
        let at = |name: Option<&str>| name.and_then(|w| people.iter().position(|(n, _)| n == w)).map_or(0, |i| i + 1);
        // AccountsService's flag: GDM's custom.conf, as it reads it.
        let flagged = || snap.users.iter().find(|u| u.automatic_login).map(|u| u.name.as_str());
        let plain = "The computer starts straight into this account, without asking for a password.";
        let (auto, enabled, help) = match &self.auto_login {
            None => (at(flagged()), false, plain.to_owned()),
            Some(AutoLogin::Choice(c)) => {
                (at(Some(c.next.as_str())), true, choice_help(c, self.proposed.as_deref(), &people))
            }
            Some(AutoLogin::Accounts) => (at(flagged()), !fixed, plain.to_owned()),
            Some(AutoLogin::Configuration(who)) => (
                at(who.as_deref().or_else(flagged)),
                false,
                "This login screen logs in by itself only as the system's configuration says \
                 (services.displayManager.autoLogin)."
                    .to_owned(),
            ),
        };
        sections.push(vcx.section(
            "Login",
            vec![
                    form::row(
                        "Log in automatically as",
                        dropdown(names, auto).on_select(Msg::AutoLogin).enabled(enabled).min_width(200.0),
                    )
                    .entry("auto-login")
                    .help(help),
                ],
        ));
        form::page(sections)
    }

    fn sheet(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        self.sheet.as_ref().map(|s| self.sheet_view(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(now: &str, next: &str) -> AutoLogin {
        AutoLogin::Choice(choices::Now { now: now.into(), next: next.into() })
    }

    #[test]
    fn greetds_initial_session_says_who_logs_in_by_themselves() {
        let config = "[default_session]\ncommand = \"aizome-greeter\"\nuser = \"greeter\"\n\n\
                      [initial_session]\ncommand = \"aizome-greeter --session aizome\"\nuser = \"adama\"\n";
        assert_eq!(greetd_initial_user(config).as_deref(), Some("adama"));
        assert_eq!(greetd_initial_user("[default_session]\nuser = \"greeter\"\n"), None);
        assert_eq!(
            greetd_initial_user("[initial_session] # at boot\nuser = 'kara' # account\n").as_deref(),
            Some("kara")
        );
        assert_eq!(greetd_initial_user("initial_session = { user = \"kara\" }\n").as_deref(), Some("kara"));
        assert_eq!(greetd_initial_user("[initial_session]\nuser = 1000\n"), None);
    }

    fn person(uid: u64, name: &str, real_name: &str, shell: &str) -> User {
        User {
            uid,
            name: name.into(),
            real_name: real_name.into(),
            shell: shell.into(),
            local_account: true,
            ..User::default()
        }
    }

    #[test]
    fn whoever_the_choice_names_can_be_shown() {
        let users = [person(1000, "adama", "William Adama", "/run/current-system/sw/bin/fish")];
        // Someone AccountsService doesn't list (Yukimi takes any account).
        let people = auto_login_people(&now("", "kara"), &users);
        assert_eq!(people, [("adama".into(), "William Adama".into()), ("kara".into(), "kara".into())]);
        assert_eq!(auto_login_people(&AutoLogin::Accounts, &users).len(), 1);
        assert_eq!(auto_login_people(&AutoLogin::Configuration(Some("adama".into())), &users).len(), 1);
    }

    #[test]
    fn the_choice_offers_only_people_yukimi_takes() {
        let users = [
            person(1000, "adama", "William Adama", "/run/current-system/sw/bin/fish"),
            // No shell given is /bin/sh.
            person(1001, "kara", "Kara Thrace", ""),
            person(1002, "locked", "Locked", "/run/current-system/sw/bin/nologin"),
            person(60001, "high", "High", "/bin/sh"),
            person(1003, "Odd.Name", "Odd", "/bin/sh"),
            User { local_account: false, ..person(1004, "ldap", "Directory", "/bin/sh") },
        ];
        let names = |how: &AutoLogin| auto_login_people(how, &users).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names(&now("", "")), ["adama", "kara"]);
        // AccountsService's flag takes anyone it lists.
        assert_eq!(names(&AutoLogin::Accounts).len(), users.len());
    }

    #[test]
    fn a_change_waiting_for_the_next_start_says_so() {
        let people = [("adama".to_owned(), "William Adama".to_owned())];
        let AutoLogin::Choice(same) = now("adama", "adama") else { unreachable!() };
        assert!(choice_help(&same, None, &people).starts_with("The computer starts straight"));
        let AutoLogin::Choice(on) = now("", "adama") else { unreachable!() };
        assert!(choice_help(&on, None, &people).starts_with("From the next start: William Adama."));
        let AutoLogin::Choice(off) = now("adama", "") else { unreachable!() };
        assert!(choice_help(&off, None, &people).starts_with("From the next start: no one."));
        // Proposed in Yukimi, not yet applied; and once it is.
        let AutoLogin::Choice(none) = now("", "") else { unreachable!() };
        assert!(choice_help(&none, Some("adama"), &people).starts_with("Proposed in Yukimi: William Adama. Apply"));
        assert_eq!(choice_help(&on, Some("adama"), &people), choice_help(&on, None, &people));
    }
}
