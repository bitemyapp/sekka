// K2 stale-conversation. Choosing another identity starts conversation 2;
// the typed `prompt` from `Starting` runs the `current` guard, which
// refuses conversation 1's prompt and hands the machine back (refused at
// run time; `forge/F-starting-data-mut.rs` defeats it by rewriting the
// running conversation's number).
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie, Effect};

fn main() {
    let idle = Auth::new(Ctx::new(Config::default()));
    let begin = Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { panic!() };
    let mut starting = starting.choose_identity(1).unwrap();
    assert_eq!(starting.starting_data(), Some(&2));
    starting.take_effects();
    let Err((mut starting, _)) = starting.prompt((1, true)) else { panic!("a stale prompt was taken") };
    assert!(starting.take_effects().is_empty());
    let mut prompted = starting.prompt((2, true)).unwrap();
    assert_eq!(prompted.take_effects(), vec![Effect::ShowPrompt { echo: true }]);
}
