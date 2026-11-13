// I4 deferred-without-space-check. `evaluation`'s handler checks the root's
// room before the deferred route's `prepared`; nothing in the types makes it
// (any handler may return `Transition(State::prepared(true))`), but a
// selection that doesn't fit is refused (refused at run time).
use impl_statig::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    out.clear();
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 20 * GIB }, &mut out);
    assert_eq!(out, vec![Effect::Refused { needed_mib: 308 * GIB, root_mib: 39 * GIB }]);
    assert_eq!(m.state(), StateName::Refused);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert_eq!(out.len(), 1);
}
