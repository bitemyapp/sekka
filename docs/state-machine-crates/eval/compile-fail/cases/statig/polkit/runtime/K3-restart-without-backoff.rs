// K3 restart-without-backoff. A failure in `starting` goes through
// `back_off`: `retry_wait`, whose entry action arms the timer; no new helper
// (refused at run time).
use impl_statig::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::Failure { conv: 1 }, &mut out);
    assert_eq!(
        out,
        vec![Effect::EndHelper { conv: 1 }, Effect::ShowNote(Note::Retrying), Effect::ArmRetry { ms: 500 }]
    );
    assert_eq!(m.state(), StateName::RetryWait);
}
