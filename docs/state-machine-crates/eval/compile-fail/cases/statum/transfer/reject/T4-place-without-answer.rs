// T4 place-without-answer: place over the conflicting target while the sheet
// is up. Every way out of `Job<AskingFiles>` is an answer (`skip`, `cancel`,
// `keep_both`, `replace`); only `Job<SettingAside>` (after `replace`) places
// at the target, with the aside's id.
use impl_statum::transfer::{AskingFiles, Job, SettingAside};
use spec::transfer::{AsideId, Effect};

fn place_over(asking: Job<AskingFiles>, out: &mut Vec<Effect>) {
    let _ = Job::<SettingAside>::set_aside(asking, AsideId(9), out);
}

fn main() {}
