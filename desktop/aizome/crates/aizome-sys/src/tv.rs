//! The TV account (Settings › Users & Groups): an account that opens
//! straight into Chanoma, the television, in kids' mode, and reaches
//! nothing else; leaving Chanoma logs out. It signs in with no password, a
//! PIN pressed on a controller (its password is the PIN's digits), or a
//! password, as the grown-up who set it up chose. Its login shell is
//! nologin, so a console or SSH won't let it in; the login screen starts
//! its session itself.
//!
//! What's known of it is [`PATH`]: written by root (`aizome-tv-account`,
//! which Settings runs through pkexec), read by the login screen (which
//! lists it, asks for what it signs in with and starts Chanoma's session
//! for it), by Aizome's session (which sends it to Chanoma's) and by
//! Chanoma. Nothing secret is in it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::service::{Task, Wake};

/// Where it's written.
pub const PATH: &str = "/var/lib/aizome/tv-account.toml";
/// The account's name when Settings makes it.
pub const NAME: &str = "tv";
/// Its full name, as the login screen shows it.
pub const REAL_NAME: &str = "TV";
/// The helper that changes it, run as root.
pub const HELPER: &str = "aizome-tv-account";

/// What it signs in with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Auth {
    /// Nothing: choosing it at the login screen starts Chanoma.
    #[default]
    None,
    /// Four to eight digits, pressed on a controller's buttons (the PIN
    /// pad's legend) or typed.
    Pin,
    Password,
}

impl Auth {
    pub fn word(self) -> &'static str {
        match self {
            Auth::None => "none",
            Auth::Pin => "pin",
            Auth::Password => "password",
        }
    }

    pub fn parse(s: &str) -> Option<Auth> {
        [Auth::None, Auth::Pin, Auth::Password].into_iter().find(|a| a.word() == s)
    }
}

/// The TV account, as [`PATH`] says.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TvAccount {
    pub user: String,
    pub auth: Auth,
    /// The PIN's length, so a PIN pad can sign in as soon as it's pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_length: Option<u8>,
}

impl TvAccount {
    /// The TV account, if there's one.
    pub fn read() -> Option<TvAccount> {
        TvAccount::read_from(Path::new(PATH))
    }

    pub fn read_from(path: &Path) -> Option<TvAccount> {
        TvAccount::parse(&std::fs::read_to_string(path).ok()?)
    }

    pub fn parse(text: &str) -> Option<TvAccount> {
        toml::from_str::<TvAccount>(text).ok().filter(|t| crate::users::valid_user_name(&t.user))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// Whether `user` is the TV account.
    pub fn is(user: &str) -> bool {
        TvAccount::read().is_some_and(|t| t.user == user)
    }
}

/// Whether `pin` can be the TV account's PIN: four to eight digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// What Settings asks of the helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Make the account if there's none, and have it sign in with `auth`
    /// (`secret`, the PIN or the password, unless `None`).
    Set { auth: Auth, secret: String },
    /// Remove the account; its home folder too, with `files`.
    Remove { files: bool },
    /// Forget the PIN of the account's Chanoma (its grown-ups' PIN), so
    /// Chanoma asks for a new one: a kid can't reset it from the TV.
    ForgetChanomaPin,
}

impl Change {
    /// The helper's arguments; the secret goes on its input.
    pub fn args(&self) -> Vec<String> {
        match self {
            Change::Set { auth, .. } => vec!["set".into(), auth.word().into()],
            Change::Remove { files: true } => vec!["remove".into(), "--files".into()],
            Change::Remove { files: false } => vec!["remove".into()],
            Change::ForgetChanomaPin => vec!["forget-chanoma-pin".into()],
        }
    }

    /// Refused before asking anyone: a PIN that isn't one, an empty
    /// password.
    pub fn check(&self) -> Result<()> {
        match self {
            Change::Set { auth: Auth::Pin, secret } if !valid_pin(secret) => {
                Err(Error::InvalidArgument("A PIN is four to eight digits".into()))
            }
            Change::Set { auth: Auth::Password, secret } if secret.is_empty() => {
                Err(Error::InvalidArgument("The password is empty".into()))
            }
            _ => Ok(()),
        }
    }
}

/// Where the helper is: `../libexec` beside this program in Aizome's
/// package (the path its polkit action names), or the same folder in a
/// build.
fn helper() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../libexec").join(HELPER), dir.join(HELPER)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// Run `change` through pkexec (polkit asks for an administrator, through
/// the session's agent), off this thread.
pub fn change(wake: &Wake, change: Change) -> Task<()> {
    if let Err(e) = change.check() {
        return Task::ready(Err(e));
    }
    let Some(helper) = helper() else {
        return Task::ready(Err(Error::Unavailable(format!("{HELPER} isn't installed"))));
    };
    Task::spawn("tv-account", wake, move || {
        use std::io::Write;
        // On NixOS only the wrapper is setuid (pkexec in a store path
        // refuses to run).
        let wrapper = Path::new("/run/wrappers/bin/pkexec");
        let pkexec = if wrapper.exists() { wrapper } else { Path::new("pkexec") };
        let mut child = std::process::Command::new(pkexec)
            .arg("--disable-internal-agent")
            .arg(helper)
            .args(change.args())
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| Error::Unavailable(format!("pkexec: {e}")))?;
        if let Some(mut input) = child.stdin.take() {
            let secret = match &change {
                Change::Set { secret, .. } => secret.as_str(),
                _ => "",
            };
            let _ = input.write_all(secret.as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| Error::Failed(e.to_string()))?;
        match out.status.code() {
            Some(0) => Ok(()),
            // pkexec's: dismissed, or not authorised.
            Some(126) => Err(Error::Cancelled),
            Some(127) => Err(Error::NotAuthorized("Not allowed to change the TV account".into())),
            _ => Err(Error::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_reads_and_writes_without_secrets() {
        let t = TvAccount { user: "tv".into(), auth: Auth::Pin, pin_length: Some(4) };
        let text = t.to_toml();
        assert_eq!(TvAccount::parse(&text), Some(t));
        assert!(!text.contains("secret"));
        let open = TvAccount::parse("user = \"tv\"\nauth = \"none\"\n").unwrap();
        assert_eq!((open.auth, open.pin_length), (Auth::None, None));
        assert_eq!(TvAccount::parse("user = \"../root\"\nauth = \"none\"\n"), None, "not an account's name");
        assert_eq!(TvAccount::parse("user = \"tv\"\nauth = \"magic\"\n"), None);
        assert_eq!(TvAccount::read_from(Path::new("/nonexistent/tv.toml")), None);
    }

    #[test]
    fn changes_are_checked_before_anyone_is_asked() {
        assert!(Change::Set { auth: Auth::Pin, secret: "1234".into() }.check().is_ok());
        assert!(Change::Set { auth: Auth::Pin, secret: "12a4".into() }.check().is_err());
        assert!(Change::Set { auth: Auth::Pin, secret: "123".into() }.check().is_err());
        assert!(Change::Set { auth: Auth::Password, secret: String::new() }.check().is_err());
        assert!(Change::Set { auth: Auth::None, secret: String::new() }.check().is_ok());
        assert_eq!(Change::Set { auth: Auth::Pin, secret: "1234".into() }.args(), ["set", "pin"]);
        assert_eq!(Change::Remove { files: true }.args(), ["remove", "--files"]);
        assert_eq!(Auth::parse("password"), Some(Auth::Password));
    }
}
