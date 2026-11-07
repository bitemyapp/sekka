//! SHB-12 (regression): the lock screen told every PAM refusal as a wrong
//! password. Headless shell, locked, with a stand-in authenticator that
//! answers as `aizome_shell::pam::Pam` does when PAM refuses for a reason
//! other than the password: the step and code that refused, what the
//! modules said, and pam_strerror's text. The messages are those of
//! pam_faillock (a lockout, `PAM_AUTH_ERR` from `pam_authenticate`) and
//! pam_unix's account management (an expired password or account).

mod common;

use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_shell::actions::{Action, SessionVerb};
use aizome_shell::lock::Phase;
use aizome_shell::pam::{Authenticator, Code, Reply, Step};
use aizome_shell::shell::Msg;
use common::*;

/// PAM refusing the right password, as `reply` says.
struct Refuses(Reply);

impl Authenticator for Refuses {
    fn authenticate(&self, _user: &str, _password: &str) -> Reply {
        self.0.clone()
    }
}

const WRONG: &str = "That password didn't work. Try again.";

fn refused(step: Step, code: Code, said: &[&str], reason: &str) -> Reply {
    Reply::Refused { step, code, said: said.iter().map(|s| s.to_string()).collect(), reason: reason.into() }
}

/// What the lock surface says once the password is submitted.
fn shown(reply: Reply) -> Option<String> {
    let mut h = scene(false);
    {
        let s = h.program_mut();
        s.authenticator = Arc::new(Refuses(reply));
        s.user = ("callen".into(), "Chris Allen".into());
    }
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    h.confirm_lock();
    assert_eq!(h.program().lock.phase(), Phase::Locked);
    let lock = h.windows().into_iter().find(|k| h.title(*k).is_empty()).expect("a lock surface");
    h.resize(lock, Size::new(W, H));
    h.type_text(lock, "the right password");
    h.named(lock, NamedKey::Enter, Modifiers::NONE);
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(10));
        h.send(Msg::Nothing);
        if h.program().lock.phase() != Phase::Checking {
            break;
        }
    }
    assert_eq!(h.program().lock.phase(), Phase::Locked);
    // What the lock surface draws under the field (lock.rs's view).
    h.program().lock.error().map(str::to_owned)
}

#[test]
fn a_lockout_or_an_expired_password_is_not_called_a_wrong_password() {
    let locked = "The account is locked due to 3 failed logins.";
    let expired = "You are required to change your password immediately (password expired)";
    let account = "Your account has expired; please contact your system administrator.";
    let strerror = "Authentication token is no longer valid; new one required";
    let cases = [
        ("faillock lockout", refused(Step::Authenticate, Code::AUTH_ERR, &[locked], "Authentication failure"), {
            locked.to_owned()
        }),
        (
            "faillock lockout, with the time left",
            refused(
                Step::Authenticate,
                Code::AUTH_ERR,
                &[locked, "(10 minutes left to unlock)"],
                "Authentication failure",
            ),
            format!("{locked} (10 minutes left to unlock)"),
        ),
        ("expired password", refused(Step::Account, Code::NEW_AUTHTOK_REQD, &[expired], strerror), expired.to_owned()),
        (
            "expired password, pam_strerror",
            refused(Step::Account, Code::NEW_AUTHTOK_REQD, &[], strerror),
            "Your password has expired and must be changed.".to_owned(),
        ),
        (
            "account expired",
            refused(Step::Account, Code::ACCT_EXPIRED, &[account], "User account has expired"),
            account.to_owned(),
        ),
    ];
    let mut seen = Vec::new();
    for (why, reply, want) in cases {
        seen.push((why, shown(reply), want));
    }
    let wrong: Vec<_> = seen.iter().filter(|(_, got, want)| got.as_deref() != Some(want.as_str())).collect();
    assert!(wrong.is_empty(), "PAM's reason not shown as PAM gave it (why, shown, wanted): {wrong:#?}");
    // A plain wrong password still says so.
    let wrong_password = refused(Step::Authenticate, Code::AUTH_ERR, &[], "Authentication failure");
    assert_eq!(shown(wrong_password).as_deref(), Some(WRONG));
}
