//! Users & Groups: your name, picture and password; the other accounts,
//! administrator or standard, added and removed; who logs in by
//! themselves; and the TV account, which opens straight into Chanoma.
//! Through AccountsService (polkit asks through the shell's agent), and
//! the TV account through its own helper (`aizome_sys::tv`, an
//! administrator's password through pkexec); accounts the system
//! configuration fixes (`users.mutableUsers = false`) say so.

use std::path::PathBuf;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::tv::{self, Auth, TvAccount};
use aizome_sys::users::{AccountType, PasswordMode, User};

use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::widgets::avatar;

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
    /// Set up the TV account (`new`), or change what it signs in with.
    Tv {
        new: bool,
        auth: Auth,
        secret: String,
        again: String,
        error: Option<String>,
    },
    TvRemove {
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
    /// The sample's TV account (a live one is the system's record).
    sample_tv: Option<TvAccount>,
    /// What a TV change will make the record, once it's done (the sample).
    tv_pending: Option<Option<TvAccount>>,
}

/// How the TV account may sign in, as the menus say it.
const TV_AUTHS: [Auth; 3] = [Auth::None, Auth::Pin, Auth::Password];

fn tv_auth_words(a: Auth) -> &'static str {
    match a {
        Auth::None => "Nothing",
        Auth::Pin => "A PIN",
        Auth::Password => "A password",
    }
}

/// Whether there's a television to set up: Chanoma is installed.
fn chanoma_installed() -> bool {
    std::path::Path::new("/run/current-system/sw/bin/aizome-chanoma-session").exists()
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
    Remove(String),
    DeleteFiles(bool),
    RemoveConfirmed,
    Done(&'static str, Result<(), String>),
    TvSetUp,
    /// What it signs in with: from the menu in the sheet, or in the pane.
    TvAuth(usize),
    TvSheetAuth(usize),
    TvSecret(String),
    TvAgain(String),
    TvChange,
    TvSave,
    TvRemove,
    TvDeleteFiles(bool),
    TvRemoveConfirmed,
    TvForgetPin,
    TvForgetPinConfirmed,
    TvDone(&'static str, Result<(), String>),
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
            Sheet::Remove { user, home, delete_files } => column![
                text(format!("Remove {user}?")).size(15.0).weight(650.0),
                form::note("They won't be able to log in any more."),
                checkbox(remove_files_words(home), *delete_files).on_toggle(Msg::DeleteFiles),
                buttons("Remove User", true, Msg::RemoveConfirmed, true),
            ]
            .spacing(12.0)
            .width(420.0)
            .into(),
            Sheet::Tv { new, auth, secret, again, error } => {
                let title = if *new { "Set Up the TV Account" } else { "How the TV Account Signs In" };
                let mut parts: Vec<Element<Msg>> = vec![
                    text(title).size(15.0).weight(650.0).into(),
                    form::note(
                        "It opens straight into Chanoma in kids' mode and can't open anything else; leaving \
                         Chanoma logs out. Chanoma's grown-up PIN still unlocks grown-up mode.",
                    ),
                    dropdown(
                        [
                            "Nothing: choose TV and it starts".to_owned(),
                            "A PIN, pressed on a controller".to_owned(),
                            "A password".to_owned(),
                        ],
                        TV_AUTHS.iter().position(|a| a == auth).unwrap_or(0),
                    )
                    .on_select(Msg::TvSheetAuth)
                    .width(Fill)
                    .into(),
                ];
                let ok = match auth {
                    Auth::None => true,
                    Auth::Pin => tv::valid_pin(secret) && !again.is_empty(),
                    Auth::Password => !secret.is_empty() && !again.is_empty(),
                };
                if *auth != Auth::None {
                    let pin = *auth == Auth::Pin;
                    parts.push(
                        text_field(secret)
                            .password()
                            .placeholder(if pin { "PIN: four to eight digits" } else { "Password" })
                            .on_change(Msg::TvSecret)
                            .id("tv-secret")
                            .width(Fill)
                            .into(),
                    );
                    parts.push(
                        text_field(again)
                            .password()
                            .placeholder("The same again")
                            .on_change(Msg::TvAgain)
                            .error(error.clone())
                            .width(Fill)
                            .into(),
                    );
                    if pin {
                        parts.push(form::note(
                            "At the login screen each digit is a controller's button: the arrows are 1 to \
                                 4, the shoulders 5 and 6, the top and left face buttons 7 and 8, and the \
                                 triggers 9 and 0. A keyboard types them.",
                        ));
                    }
                }
                parts.push(buttons(if *new { "Set Up" } else { "Save" }, ok, Msg::TvSave, false));
                aizome_kit::column(parts).spacing(10.0).width(440.0).into()
            }
            Sheet::TvRemove { delete_files } => column![
                text("Remove the TV account?").size(15.0).weight(650.0),
                form::note("TV won't be at the login screen any more."),
                checkbox("Also delete its home folder, and its Chanoma settings with it", *delete_files)
                    .on_toggle(Msg::TvDeleteFiles),
                buttons("Remove TV Account", true, Msg::TvRemoveConfirmed, true),
            ]
            .spacing(12.0)
            .width(420.0)
            .into(),
        }
    }

    /// The TV account: the system's record, or the sample's.
    fn tv(&self, sample: bool) -> Option<TvAccount> {
        if sample { self.sample_tv.clone() } else { TvAccount::read() }
    }

    /// Ask the helper for `change`; the sample's record becomes `after`.
    fn change_tv(&mut self, cx: &mut PaneCx<Msg>, change: tv::Change, after: Option<TvAccount>, what: &'static str) {
        self.tv_pending = Some(after);
        let task = cx.sys().accounts.change_tv(change);
        cx.wait(task, move |r| Msg::TvDone(what, r.map_err(|e| e.to_string())));
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
            SettingEntry::new("tv", "TV account").keywords(&[
                "television",
                "chanoma",
                "living room",
                "kiosk",
                "kids",
                "controller",
            ]),
        ]
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
                let users = cx.sys().accounts.snapshot().users.clone();
                let who = if i == 0 { None } else { users.get(i - 1).map(|u| u.name.clone()) };
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
            Msg::AutoLoginConfirmed(who) => {
                let snap = cx.sys().accounts.snapshot();
                let target = match &who {
                    Some(n) => snap.user(n).cloned(),
                    None => snap.users.iter().find(|u| u.automatic_login).cloned(),
                };
                let Some(u) = target else { return };
                let task = cx.sys().accounts.set_automatic_login(&u, who.is_some());
                cx.wait(task, |r| Msg::Done("Automatic login", r.map_err(|e| e.to_string())));
            }
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
                cx.wait(task, |r| Msg::Done("Removed the user", r.map_err(|e| e.to_string())));
            }
            Msg::Done(what, Ok(())) => cx.say(what),
            Msg::Done(what, Err(e)) => cx.failed(what, e),
            Msg::TvSetUp => {
                self.sheet =
                    Some(Sheet::Tv { new: true, auth: Auth::None, secret: String::new(), again: String::new(), error: None });
                cx.open_sheet();
            }
            Msg::TvChange => {
                let Some(t) = self.tv(cx.sample()) else { return };
                self.sheet =
                    Some(Sheet::Tv { new: false, auth: t.auth, secret: String::new(), again: String::new(), error: None });
                cx.open_sheet();
                cx.focus("tv-secret");
            }
            // In the pane: nothing to type for Nothing; a PIN or a password
            // is asked for.
            Msg::TvAuth(i) => {
                let (Some(t), Some(auth)) = (self.tv(cx.sample()), TV_AUTHS.get(i).copied()) else { return };
                if auth == t.auth {
                    return;
                }
                if auth == Auth::None {
                    let after = TvAccount { auth, pin_length: None, ..t };
                    let change = tv::Change::Set { auth, secret: String::new() };
                    self.change_tv(cx, change, Some(after), "The TV account signs in with nothing");
                } else {
                    self.sheet =
                        Some(Sheet::Tv { new: false, auth, secret: String::new(), again: String::new(), error: None });
                    cx.open_sheet();
                    cx.focus("tv-secret");
                }
            }
            Msg::TvSheetAuth(i) => {
                if let Some(Sheet::Tv { auth, secret, again, error, .. }) = &mut self.sheet {
                    *auth = TV_AUTHS.get(i).copied().unwrap_or(Auth::None);
                    secret.clear();
                    again.clear();
                    *error = None;
                }
                cx.focus("tv-secret");
            }
            Msg::TvSecret(s) => {
                if let Some(Sheet::Tv { auth, secret, error, .. }) = &mut self.sheet {
                    // A PIN is digits: nothing else goes in.
                    *secret = if *auth == Auth::Pin { s.chars().filter(char::is_ascii_digit).take(8).collect() } else { s };
                    *error = None;
                }
            }
            Msg::TvAgain(s) => {
                if let Some(Sheet::Tv { again, error, .. }) = &mut self.sheet {
                    *again = s;
                    *error = None;
                }
            }
            Msg::TvSave => {
                let Some(Sheet::Tv { new, auth, secret, again, error }) = &mut self.sheet else { return };
                if *auth != Auth::None && secret != again {
                    *error = Some("The two don't match".into());
                    return;
                }
                let (new, auth, secret) = (*new, *auth, std::mem::take(secret));
                let after = TvAccount {
                    user: self.tv(cx.sample()).map_or_else(|| tv::NAME.to_owned(), |t| t.user),
                    auth,
                    pin_length: (auth == Auth::Pin).then_some(secret.len() as u8),
                };
                self.sheet = None;
                cx.close_sheet();
                let what = if new { "Set up the TV account" } else { "Changed how the TV account signs in" };
                self.change_tv(cx, tv::Change::Set { auth, secret }, Some(after), what);
            }
            Msg::TvRemove => {
                self.sheet = Some(Sheet::TvRemove { delete_files: false });
                cx.open_sheet();
            }
            Msg::TvDeleteFiles(on) => {
                if let Some(Sheet::TvRemove { delete_files }) = &mut self.sheet {
                    *delete_files = on;
                }
            }
            Msg::TvRemoveConfirmed => {
                let Some(Sheet::TvRemove { delete_files }) = self.sheet.take() else { return };
                cx.close_sheet();
                self.change_tv(cx, tv::Change::Remove { files: delete_files }, None, "Removed the TV account");
            }
            Msg::TvForgetPin => cx.confirm(Confirm::new(
                "Forget Chanoma's PIN on the TV account?",
                "Chanoma on the TV asks a grown-up to choose a new PIN when it next starts. Until then it stays in kids' mode.",
                "Forget PIN",
                Msg::TvForgetPinConfirmed,
            )),
            Msg::TvForgetPinConfirmed => {
                let after = self.tv(cx.sample());
                self.change_tv(cx, tv::Change::ForgetChanomaPin, after, "Chanoma will ask for a new PIN");
            }
            Msg::TvDone(what, Ok(())) => {
                if let Some(after) = self.tv_pending.take()
                    && cx.sample()
                {
                    self.sample_tv = after;
                }
                cx.say(what);
            }
            Msg::TvDone(what, Err(e)) => {
                self.tv_pending = None;
                cx.failed(what, e);
            }
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

        // Other people (the TV account has a section of its own).
        let tv_account = self.tv(vcx.sample());
        let others: Vec<&User> = snap
            .users
            .iter()
            .filter(|u| Some(u.uid) != me.as_ref().map(|m| m.uid))
            .filter(|u| tv_account.as_ref().is_none_or(|t| t.user != u.name))
            .collect();
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

        let mut names = vec!["Off".to_owned()];
        names.extend(snap.users.iter().map(|u| u.display_name().to_owned()));
        let auto = snap.users.iter().position(|u| u.automatic_login).map_or(0, |i| i + 1);
        sections.push(vcx.section(
            "Login",
            vec![
                    form::row(
                        "Log in automatically as",
                        dropdown(names, auto).on_select(Msg::AutoLogin).enabled(!fixed).min_width(200.0),
                    )
                    .entry("auto-login")
                    .help("The computer starts straight into this account, without asking for a password."),
                ],
        ));

        // The television's account.
        if vcx.sample() || chanoma_installed() {
            let rows = match &tv_account {
                None => vec![
                    form::row("TV account", button("Set Up…").icon(Icon::Tv).enabled(!fixed).on_press(Msg::TvSetUp))
                        .entry("tv")
                        .help(
                            "An account for the television: it opens straight into Chanoma in kids' mode, can't \
                             open anything else, and leaving Chanoma logs out. It can sign in with nothing, a PIN \
                             pressed on a controller, or a password.",
                        ),
                ],
                Some(t) => {
                    let mut rows = vec![
                        form::row(
                            "TV signs in with",
                            dropdown(
                                TV_AUTHS.map(|a| tv_auth_words(a).to_owned()),
                                TV_AUTHS.iter().position(|a| *a == t.auth).unwrap_or(0),
                            )
                            .on_select(Msg::TvAuth)
                            .enabled(!fixed)
                            .min_width(160.0),
                        )
                        .entry("tv")
                        .help(match t.auth {
                            Auth::None => "Choosing TV at the login screen starts Chanoma.",
                            Auth::Pin => "Pressed on a controller at the login screen, each button a digit, or typed.",
                            Auth::Password => "Typed at the login screen, on a keyboard or with a controller.",
                        }),
                    ];
                    if t.auth != Auth::None {
                        let what = if t.auth == Auth::Pin { "Change PIN…" } else { "Change Password…" };
                        rows.push(form::row("", button(what).enabled(!fixed).on_press(Msg::TvChange)));
                    }
                    rows.push(
                        form::row("Chanoma's PIN", button("Forget…").enabled(!fixed).on_press(Msg::TvForgetPin))
                            .help("The grown-ups' PIN in Chanoma on the TV: forgotten, it asks for a new one."),
                    );
                    rows.push(form::whole(
                        container(
                            row![
                                button("Remove TV Account…").destructive().enabled(!fixed).on_press(Msg::TvRemove),
                                spacer()
                            ]
                            .width(Fill),
                        )
                        .padding([8.0, form::INSET])
                        .width(Fill),
                    ));
                    rows
                }
            };
            sections.push(vcx.section("Television", rows));
        }
        form::page(sections)
    }

    fn sheet(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        self.sheet.as_ref().map(|s| self.sheet_view(s))
    }
}
