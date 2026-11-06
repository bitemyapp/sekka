// Forge, T3 lost-aside. `restored` declares the edge `Restoring → Finished`
// (its `Next`), so statum implements the public `CanTransitionWith<FinishedTok>`
// for `Job<Restoring>`, and taking it skips `restored`'s body. Outside code
// can't make a `FinishedTok`, but `state_data` is always a public field: it
// moves one out of another job that legitimately finished. The aside is
// dropped without a restore result, nothing is reported, and the job ends.
// (The other public means: `transition_map` and `CanTransitionMap`; T4 and
// T6 the same way, with payloads from jobs that reached `SettingAside` or
// `AskingFolders`.)
use impl_statum::transfer::{Finished, Idle, Job, Probed};
use spec::transfer::{AsideId, Config, Effect, Found};
use statum::{Branch, CanTransitionWith};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Branch::First(probing) = Job::<Idle>::new(Config::default()).start(2, &mut out) else { panic!("no item") };
    let Probed::AskingFiles(asking) = probing.probed(Found::Conflict { can_merge: false }, &mut out) else {
        panic!("not asking");
    };
    let restoring = asking.replace(&mut out).set_aside(AsideId(5), &mut out).failed(false, &mut out);
    let mut donor_out: Vec<Effect> = Vec::new();
    let Branch::Second(donor) = Job::<Idle>::new(Config::default()).start(0, &mut donor_out) else { panic!("items") };
    out.clear();
    let _: Job<Finished> = restoring.transition_with_data(donor.state_data);
    // No outcome for item 0, no problem for its aside, item 1 never probed.
    assert!(out.is_empty());
}
