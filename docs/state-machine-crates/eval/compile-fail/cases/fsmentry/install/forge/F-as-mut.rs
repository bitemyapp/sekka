// F-as-mut (install): commits I2 bootloader-before-copy. fsmentry generates
// `AsMut<Steps>` on the `Executing` handle, public with the handles:
// swapping two live machines' steps puts `a`, which finished step 3, at the
// bootloader without step 4. (`Steps` has a private field and isn't `Copy`,
// so a swap is the only write. Other crate-generated means: `AsMut` on the
// `Preparing`, `Prepared` and inner data handles, and every exit handing
// back its data (a `Route`, a `Steps`) for another machine's edge to take.)
use impl_fsmentry::install::{Install, SessionEntry};
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let (mut a, mut b) = (Install::new(Config::default()), Install::new(Config::default()));
    let mut out = Vec::new();
    for m in [&mut a, &mut b] {
        m.handle(Event::Begin, &mut out);
        m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
        m.handle(Event::Configured, &mut out);
        m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
        m.handle(Event::Confirm { phrase_ok: true }, &mut out);
        m.handle(Event::StepDone, &mut out);
        m.handle(Event::StepDone, &mut out);
    }
    b.handle(Event::StepDone, &mut out);
    b.handle(Event::StepDone, &mut out);
    assert_eq!((a.state(), b.state()), (StateName::Files, StateName::Bootloader));
    if let (SessionEntry::Executing(mut x), SessionEntry::Executing(mut y)) = (a.entry(), b.entry()) {
        std::mem::swap(x.as_mut(), y.as_mut());
    }
    assert_eq!(a.state(), StateName::Bootloader);
    out.clear();
    a.handle(Event::StepDone, &mut out);
    assert_eq!(out, vec![Effect::Run(Stage::Record)]);
}
