// I2 bootloader-before-copy: go from step 3 straight to the bootloader.
// `Files::done` leads to step 4 (`Step4`: `Copy` or `BuildOnTarget`); only
// their `done` leads to `Bootloader`.
use impl_typestate::install::{Bootloader, Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let c = Config::default();
    let mut out: Vec<Effect> = Vec::new();
    let evaluation = Idle::new().begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(&c, 2 * GIB, 10 * GIB, GIB, &mut out) else {
        return;
    };
    let Ok(erase) = prepared.confirm(true, &mut out) else { return };
    let files = erase.done(&mut out).done(&mut out);
    let _: Bootloader = files.done(&mut out);
}
