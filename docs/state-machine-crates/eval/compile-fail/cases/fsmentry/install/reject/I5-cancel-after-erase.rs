// I5 cancel-after-erase: cancel once erasing started. `Executing` has
// `complete` and `fail`; `cancel` is `Idle`'s, `Preparing`'s and
// `Prepared`'s.
use impl_fsmentry::install::{Install, SessionEntry};
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert_eq!(m.state(), StateName::Erase);
    if let SessionEntry::Executing(executing) = m.entry() {
        executing.cancel();
    }
}
