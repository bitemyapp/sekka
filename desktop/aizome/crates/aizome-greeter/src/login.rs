//! One login, as a conversation with greetd: pick a person, answer PAM's
//! questions (a password, or more), then start their session. Pure: it
//! turns what greetd says and what the person does into what to ask greetd
//! next and what the screen shows, so it's tested without either.

use crate::greetd::{AuthMessageType, ErrorType, Request, Response};

/// What the screen shows for the login.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Nobody picked yet (the user name is to be typed).
    Nobody,
    /// Waiting for greetd.
    Waiting,
    /// A question to answer: hidden as it's typed (`secret`), or not.
    Ask { secret: bool, prompt: String },
    /// Authenticated: starting the session.
    Starting,
    /// greetd will start the session once the greeter is gone.
    Done,
    /// greetd couldn't begin (an unknown user): Enter tries again.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

/// What each request still waiting for its answer was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Create,
    Answer,
    Start,
    Cancel,
}

/// The session to start once authenticated.
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: Vec<String>,
    pub env: Vec<String>,
}

#[derive(Debug)]
pub struct Login {
    pub user: Option<String>,
    pub step: Step,
    /// What PAM or greetd said, or why the last try failed.
    pub message: Option<(String, Tone)>,
    /// A wrong answer: the field shakes.
    pub failed: u32,
    /// The requests waiting for their answers, oldest first, each with
    /// the login (generation) it belongs to: an answer to an earlier one
    /// (someone else was picked meanwhile) is let go.
    asked: std::collections::VecDeque<(Asked, u32)>,
    generation: u32,
    /// A greetd session is open (created and not finished or cancelled).
    open: bool,
}

impl Default for Login {
    fn default() -> Self {
        Login {
            user: None,
            step: Step::Nobody,
            message: None,
            failed: 0,
            asked: Default::default(),
            generation: 0,
            open: false,
        }
    }
}

/// PAM's prompt as a field's placeholder: `Password: ` is `Password`.
pub fn placeholder(prompt: &str) -> String {
    let p = prompt.trim().trim_end_matches(':').trim();
    if p.is_empty() { "Password".to_owned() } else { p.to_owned() }
}

impl Login {
    /// Begin logging `user` in (giving up on whoever was before).
    pub fn begin(&mut self, user: &str) -> Vec<Request> {
        let mut out = Vec::new();
        if self.open {
            out.push(self.ask(Asked::Cancel, Request::CancelSession));
        }
        self.generation += 1;
        self.user = Some(user.to_owned());
        self.step = Step::Waiting;
        out.push(self.ask(Asked::Create, Request::CreateSession { username: user.to_owned() }));
        self.open = true;
        out
    }

    /// Give up on this login (back to picking someone).
    pub fn cancel(&mut self) -> Vec<Request> {
        let mut out = Vec::new();
        if self.open {
            out.push(self.ask(Asked::Cancel, Request::CancelSession));
        }
        self.generation += 1;
        self.open = false;
        self.user = None;
        self.step = Step::Nobody;
        self.message = None;
        out
    }

    /// The person answered the question (Enter in the field).
    pub fn answer(&mut self, response: String) -> Option<Request> {
        if !matches!(self.step, Step::Ask { .. }) {
            return None;
        }
        self.step = Step::Waiting;
        self.message = None;
        Some(self.ask(Asked::Answer, Request::PostAuthMessageResponse { response: Some(response) }))
    }

    /// greetd answered the oldest request waiting; `launch` is what to start
    /// once authenticated. Returns what to ask next.
    pub fn heard(&mut self, response: Response, launch: &Launch) -> Vec<Request> {
        let Some((what, generation)) = self.asked.pop_front() else {
            // Nothing was asked: nothing to do with it.
            return Vec::new();
        };
        // Cancelling a session greetd already ended is fine; an answer for
        // an earlier login belongs to nobody now.
        if what == Asked::Cancel || generation != self.generation {
            return Vec::new();
        }
        let asked = Some(what);
        match response {
            Response::AuthMessage { auth_message_type, auth_message } => match auth_message_type {
                AuthMessageType::Secret | AuthMessageType::Visible => {
                    self.step = Step::Ask {
                        secret: auth_message_type == AuthMessageType::Secret,
                        prompt: placeholder(&auth_message),
                    };
                    Vec::new()
                }
                AuthMessageType::Info | AuthMessageType::Error => {
                    let tone = if auth_message_type == AuthMessageType::Error { Tone::Error } else { Tone::Info };
                    let text = auth_message.trim().to_owned();
                    if !text.is_empty() {
                        self.message = Some((text, tone));
                    }
                    self.step = Step::Waiting;
                    vec![self.ask(Asked::Answer, Request::PostAuthMessageResponse { response: None })]
                }
            },
            Response::Success if asked == Some(Asked::Start) => {
                self.step = Step::Done;
                self.open = false;
                Vec::new()
            }
            Response::Success => {
                // Authenticated (also with no questions, an account with no
                // password): start the session.
                self.step = Step::Starting;
                vec![self.ask(Asked::Start, Request::StartSession { cmd: launch.cmd.clone(), env: launch.env.clone() })]
            }
            Response::Error { error_type, description } => {
                let text = match (error_type, asked) {
                    (ErrorType::AuthError, _) => {
                        self.failed += 1;
                        "That password didn't work. Try again.".to_owned()
                    }
                    (_, Some(Asked::Start)) => format!("The session didn't start: {}", tidy(&description)),
                    _ => tidy(&description),
                };
                self.message = Some((text, Tone::Error));
                // greetd may still hold the session (it does after a wrong
                // password, and refuses a new one: "a session is already
                // being configured"), so beginning again cancels it first;
                // cancelling one it already dropped is harmless. Begin again
                // for the same person, unless beginning is what failed.
                self.open = true;
                match self.user.clone() {
                    Some(u) if asked != Some(Asked::Create) => self.begin(&u),
                    _ => {
                        self.step = Step::Failed;
                        Vec::new()
                    }
                }
            }
        }
    }

    /// greetd went away (it restarts the greeter itself).
    pub fn lost(&mut self, why: &str) {
        self.message = Some((format!("Lost the login service: {why}"), Tone::Error));
        self.step = Step::Waiting;
        self.asked.clear();
        self.open = false;
    }

    fn ask(&mut self, what: Asked, r: Request) -> Request {
        self.asked.push_back((what, self.generation));
        r
    }
}

/// greetd's or PAM's words, made a sentence.
fn tidy(s: &str) -> String {
    let s = s.trim();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Something went wrong.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch { cmd: vec!["aizome-session".into()], env: vec!["XDG_SESSION_TYPE=wayland".into()] }
    }
    fn secret(p: &str) -> Response {
        Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: p.into() }
    }

    #[test]
    fn a_password_logs_in_and_starts_the_session() {
        let mut l = Login::default();
        assert_eq!(l.begin("alice"), [Request::CreateSession { username: "alice".into() }]);
        assert!(l.heard(secret("Password: "), &launch()).is_empty());
        assert_eq!(l.step, Step::Ask { secret: true, prompt: "Password".into() });
        assert_eq!(
            l.answer("hunter2".into()),
            Some(Request::PostAuthMessageResponse { response: Some("hunter2".into()) })
        );
        assert_eq!(l.step, Step::Waiting);
        assert_eq!(
            l.heard(Response::Success, &launch()),
            [Request::StartSession {
                cmd: vec!["aizome-session".into()],
                env: vec!["XDG_SESSION_TYPE=wayland".into()]
            }]
        );
        assert_eq!(l.step, Step::Starting);
        assert!(l.heard(Response::Success, &launch()).is_empty());
        assert_eq!(l.step, Step::Done);
    }

    #[test]
    fn a_wrong_password_says_so_and_asks_again() {
        let mut l = Login::default();
        l.begin("alice");
        l.heard(secret("Password: "), &launch());
        l.answer("wrong".into());
        let next = l.heard(
            Response::Error { error_type: ErrorType::AuthError, description: "pam_authenticate: AUTH_ERR".into() },
            &launch(),
        );
        assert_eq!(next, [Request::CancelSession, Request::CreateSession { username: "alice".into() }]);
        assert_eq!(l.failed, 1);
        assert_eq!(l.message, Some(("That password didn't work. Try again.".into(), Tone::Error)));
        // greetd answers the cancel, then asks again.
        assert!(l.heard(Response::Success, &launch()).is_empty());
        l.heard(secret("Password: "), &launch());
        assert!(matches!(l.step, Step::Ask { .. }));
        // The message stays until they answer again.
        assert!(l.message.is_some());
        l.answer("right".into());
        assert!(l.message.is_none());
    }

    #[test]
    fn info_is_shown_and_acknowledged_and_more_questions_are_asked() {
        let mut l = Login::default();
        l.begin("alice");
        let next = l.heard(
            Response::AuthMessage { auth_message_type: AuthMessageType::Info, auth_message: "Touch your key".into() },
            &launch(),
        );
        assert_eq!(next, [Request::PostAuthMessageResponse { response: None }]);
        assert_eq!(l.message, Some(("Touch your key".into(), Tone::Info)));
        l.heard(
            Response::AuthMessage { auth_message_type: AuthMessageType::Visible, auth_message: "Code: ".into() },
            &launch(),
        );
        assert_eq!(l.step, Step::Ask { secret: false, prompt: "Code".into() });
    }

    #[test]
    fn picking_someone_else_cancels_first_and_ignores_the_cancels_answer() {
        let mut l = Login::default();
        l.begin("alice");
        l.heard(secret("Password: "), &launch());
        assert_eq!(l.begin("bob"), [Request::CancelSession, Request::CreateSession { username: "bob".into() }]);
        // The cancel's answer (an error if greetd had no session) changes nothing.
        assert!(
            l.heard(Response::Error { error_type: ErrorType::Error, description: "no session".into() }, &launch())
                .is_empty()
        );
        assert!(l.message.is_none());
        l.heard(secret("Password: "), &launch());
        assert_eq!(l.user.as_deref(), Some("bob"));
        assert!(matches!(l.step, Step::Ask { .. }));
    }

    #[test]
    fn an_account_without_a_password_starts_at_once_and_a_failed_start_says_why() {
        let mut l = Login::default();
        l.begin("kiosk");
        let next = l.heard(Response::Success, &launch());
        assert!(matches!(next[..], [Request::StartSession { .. }]));
        let next =
            l.heard(Response::Error { error_type: ErrorType::Error, description: "exec failed".into() }, &launch());
        assert_eq!(l.message, Some(("The session didn't start: Exec failed".into(), Tone::Error)));
        assert_eq!(next, [Request::CancelSession, Request::CreateSession { username: "kiosk".into() }]);
    }

    #[test]
    fn a_failure_to_begin_waits_rather_than_looping() {
        let mut l = Login::default();
        l.begin("ghost");
        let next =
            l.heard(Response::Error { error_type: ErrorType::Error, description: "unknown user".into() }, &launch());
        assert!(next.is_empty());
        assert_eq!(l.step, Step::Failed);
        assert_eq!(l.message, Some(("Unknown user".into(), Tone::Error)));
        // Return tries again, cancelling whatever greetd kept first.
        assert_eq!(l.begin("ghost"), [Request::CancelSession, Request::CreateSession { username: "ghost".into() }]);
    }

    #[test]
    fn a_late_answer_for_someone_no_longer_picked_starts_nothing() {
        let mut l = Login::default();
        l.begin("alice");
        l.heard(secret("Password: "), &launch());
        l.answer("alices".into());
        // Bob is picked while greetd checks Alice's password.
        assert_eq!(l.begin("bob"), [Request::CancelSession, Request::CreateSession { username: "bob".into() }]);
        // Alice's success, then the cancel's answer: neither starts anything.
        assert!(l.heard(Response::Success, &launch()).is_empty());
        assert!(l.heard(Response::Success, &launch()).is_empty());
        assert_eq!(l.step, Step::Waiting);
        l.heard(secret("Password: "), &launch());
        assert_eq!(l.user.as_deref(), Some("bob"));
        assert!(matches!(l.step, Step::Ask { .. }));
        // Back to the people: cancelled, and its answer let go.
        assert_eq!(l.cancel(), [Request::CancelSession]);
        assert_eq!(l.step, Step::Nobody);
        assert!(l.heard(Response::Success, &launch()).is_empty());
        assert!(l.cancel().is_empty());
    }

    #[test]
    fn answering_only_when_asked() {
        let mut l = Login::default();
        assert_eq!(l.answer("x".into()), None);
        l.begin("alice");
        assert_eq!(l.answer("x".into()), None);
    }
}
