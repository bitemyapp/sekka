// I2 bootloader-before-copy: go from step 3 straight to the bootloader.
// `Files` has `copy` and `build_on_target`; only they lead to `Bootloader`.
use impl_fsmentry::install::{ExecEntry, Install, SessionEntry};
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    m.handle(Event::StepDone, &mut out);
    m.handle(Event::StepDone, &mut out);
    assert_eq!(m.state(), StateName::Files);
    if let SessionEntry::Executing(mut executing) = m.entry()
        && let ExecEntry::Files(files) = executing.as_mut().entry()
    {
        files.bootloader();
    }
}
