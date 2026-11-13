// I2 bootloader-before-copy: go from step 3 straight to the bootloader.
// `Helper<Files>::done` leads to step 4 (`Step4`: `Copy` or
// `BuildOnTarget`); only their `done` leads to `Bootloader`.
use impl_statum::install::{Bootloader, Helper, Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let evaluation =
        Helper::<Idle>::new(Config::default()).begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(2 * GIB, 10 * GIB, GIB, &mut out) else { return };
    let Ok(erase) = prepared.confirm(true, &mut out) else { return };
    let files = erase.done(&mut out).done(&mut out);
    let _: Helper<Bootloader> = files.done(&mut out);
}
