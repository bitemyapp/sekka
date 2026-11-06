// L6 shake-outside-locked. A shape probe: the shake step is a field of the
// one state struct, beside `phase`, so a running shake while unlocked is a
// value of the state's type: accepted.
use impl_implicit::lock::{Fields, Phase};
use spec::lock::Config;

fn main() {
    let shaking = Fields {
        config: Config::default(),
        phase: Phase::Unlocked,
        outputs: 1,
        has_text: false,
        busy: false,
        check: 0,
        error: None,
        shake: 3,
        sleep_pending: false,
        delay_held: false,
    };
    assert!(shaking.phase == Phase::Unlocked && shaking.shake > 0);
}
