// I3 confirm-during-preparation: the typed `confirm` from a preparation
// stage returns an `Installer<Failed>`, not an `Installer<Erase>`.
use impl_state_machines::install::{Ctx, Erase, Installer};
use spec::install::{Config, GIB};

fn main() {
    let configuration = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap();
    let _erasing: Installer<Erase> = configuration.confirm(true).unwrap();
}
