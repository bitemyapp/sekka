// Forge (K2 stale-conversation). The generated, public `starting_data_mut`
// rewrites the running conversation's number: conversation 1's prompt then
// counts while conversation 2 runs. Other means that stay public:
// `with_<state>_data`, `<state>_data_mut` and the generic
// `state_data_<state>_mut` on every typed machine (`active_data_mut` can
// replace the request with `Request::default()`); `set_<state>_data` and
// `<state>_data_mut` on the wrapper `into_dynamic()` returns;
// `initialize()` on every state type (polkit has no entry hooks).
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie, Effect};

fn main() {
    let idle = Auth::new(Ctx::new(Config::default()));
    let begin = Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { panic!() };
    let mut starting = starting.choose_identity(1).unwrap();
    assert_eq!(starting.starting_data(), Some(&2));
    *starting.starting_data_mut().unwrap() = 1;
    starting.take_effects();
    let mut prompted = starting.prompt((1, true)).unwrap();
    assert_eq!(prompted.take_effects(), vec![Effect::ShowPrompt { echo: true }]);
}
