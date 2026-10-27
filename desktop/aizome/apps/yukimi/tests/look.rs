//! Pictures of each page of the sample system, written where
//! `YUKIMI_PICTURES` says (for looking at while working on the look).
use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_yukimi::app::{Changes, Msg, Page, Yukimi};

/// Let background work answer: wait until `done` says so (or a while).
fn settle(h: &mut Harness<Yukimi>, done: impl Fn(&Yukimi) -> bool) {
    let start = Instant::now();
    while !done(h.program()) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::NoteGone(0));
    }
}

#[test]
fn pictures() {
    let Some(dir) = std::env::var_os("YUKIMI_PICTURES") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let appearance = Appearance { theme, reduced_motion: true, ..Appearance::default() };
        let mut h = Harness::with(Yukimi::sample(), appearance, false);
        let w = h.window();
        for page in Page::ALL {
            h.send(Msg::Show(page));
            if page == Page::History {
                h.send(Msg::ToggleGeneration(28));
                settle(&mut h, |a| matches!(a.changes.get(&28), Some(Changes::Ready { .. })));
            }
            if page == Page::Discover
                && let Ok(query) = std::env::var("YUKIMI_QUERY")
            {
                h.send(Msg::Search(query));
                settle(&mut h, |a| a.found.is_some());
            }
            h.save_png(w, dir.join(format!("{}-{name}.png", page.id())), 1.0);
            if page == Page::Desktops {
                let pick = |setting: &str, id: &str, on: bool| Msg::Pick { setting: setting.into(), id: id.into(), on };
                h.send(pick("calamares.desktops", "gnome", false));
                h.send(pick("calamares.desktops", "aizome", false));
                h.save_png(w, dir.join(format!("desktops-problem-{name}.png")), 1.0);
                h.send(pick("calamares.desktops", "aizome", true));
                h.send(pick("calamares.loginScreen", "aizome", true));
                h.save_png(w, dir.join(format!("desktops-changed-{name}.png")), 1.0);
                h.send(Msg::ApplyChoices);
                h.save_png(w, dir.join(format!("desktops-apply-{name}.png")), 1.0);
                for key in h.windows() {
                    if key != w {
                        h.save_png(key, dir.join(format!("desktops-apply-alert-{name}.png")), 1.0);
                    }
                }
            }
            if page == Page::Installed {
                h.send(Msg::ShowAll);
                h.scroll(w, aizome_kit::layout::Point::new(600.0, 400.0), aizome_kit::layout::Vec2::new(0.0, 4000.0));
                h.save_png(w, dir.join(format!("installed-all-{name}.png")), 1.0);
            }
        }
    }
}

/// The sheets: a package's details, a question, an operation under way
/// and one that failed, and choosing a branch.
#[test]
fn sheet_pictures() {
    use std::sync::Arc;

    use aizome_yukimi::app::{Clock, Runner, Source};
    use aizome_yukimi::ops::{Operation, Run};

    let Some(dir) = std::env::var_os("YUKIMI_PICTURES") else { return };
    let dir = std::path::PathBuf::from(dir);
    let script = |text: &'static str| -> Vec<aizome_yukimi::ops::Step> {
        vec![Box::new(move || {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", text]);
            Ok(Some(Run { command, as_root: false }))
        })]
    };
    let runner = Runner::Mock(Arc::new(move |op: &Operation| match op {
        Operation::Rollback { .. } => script(
            r#"echo 'these derivations will be built:' >&2
echo '  /nix/store/0123456789abcdfghijklmnpqrsvwxyz-gimp2-2.10.38.drv' >&2
echo 'building /nix/store/0123456789abcdfghijklmnpqrsvwxyz-gimp2-2.10.38.drv' >&2
echo '@nix {"action":"msg","level":0,"msg":"error: builder for gimp2 failed with exit code 1"}' >&2; exit 1"#,
        ),
        _ => script(
            r#"echo '@nix {"action":"start","id":2,"level":4,"type":103,"text":"","fields":[]}' >&2
echo '@nix {"action":"result","id":2,"type":105,"fields":[34,120,3,0]}' >&2
sleep 0.2
echo 'copying path /nix/store/0123456789abcdfghijklmnpqrsvwxyz-babl-0.1.114 from https://cache.nixos.org' >&2
echo '@nix {"action":"start","id":4,"level":4,"type":104,"text":"","fields":[]}' >&2
echo '@nix {"action":"result","id":4,"type":105,"fields":[1,3,1,0]}' >&2
echo '@nix {"action":"start","id":5,"level":3,"type":105,"text":"building","fields":["/nix/store/0123456789abcdfghijklmnpqrsvwxyz-gimp2-2.10.38.drv","",1,1]}' >&2
echo '@nix {"action":"result","id":5,"type":104,"fields":["installPhase"]}' >&2
echo 'installing' >&2
sleep 0.3
echo '@nix {"action":"result","id":5,"type":101,"fields":["  install -Dm755 gimp-2.10 $out/bin/gimp-2.10"]}' >&2
sleep 30"#,
        ),
    }));
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let model = Arc::new(aizome_yukimi::fixture::model());
        let mut app =
            Yukimi::with(Source::Sample(model.clone()), runner.clone(), Clock::Fixed(aizome_yukimi::fixture::NOW, 21));
        app.check = Some(Arc::new(aizome_yukimi::fixture::check(&model)));
        app.index = Some(Arc::new(aizome_yukimi::fixture::index()));
        let appearance = Appearance { theme, reduced_motion: true, ..Appearance::default() };
        let mut h = Harness::with(app, appearance, false);
        let w = h.window();
        h.send(Msg::Show(Page::Discover));
        h.send(Msg::Open(aizome_yukimi::fixture::index().get("gimp").cloned().unwrap()));
        h.save_png(w, dir.join(format!("details-{name}.png")), 1.0);
        h.send(Msg::InstallForEveryone("gimp2".into()));
        h.save_png(w, dir.join(format!("confirm-{name}.png")), 1.0);
        h.named(w, aizome_kit::sidestep_ui::NamedKey::Escape, aizome_kit::sidestep_ui::Modifiers::default());
        h.send(Msg::Run(Operation::InstallForMe { attr: "gimp2".into(), unfree: false }));
        settle(&mut h, |a| a.job.as_ref().is_some_and(|j| !j.summary.is_empty()));
        h.send(Msg::JobDetails(true));
        h.save_png(w, dir.join(format!("job-{name}.png")), 1.0);
        h.send(Msg::StopJob);
        settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
        h.send(Msg::CloseJob);
        h.send(Msg::Run(Operation::Rollback { generation: 26 }));
        settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
        h.save_png(w, dir.join(format!("failed-{name}.png")), 1.0);
        h.send(Msg::CloseJob);
        h.send(Msg::Show(Page::Updates));
        h.send(Msg::ChooseBranch("sekka".into()));
        h.send(Msg::Branches(Ok(vec![
            "main".into(),
            "nightly".into(),
            "selective-updates".into(),
            "fish-shell".into(),
        ])));
        h.save_png(w, dir.join(format!("branch-{name}.png")), 1.0);
    }
}

/// This computer, read where `AIZOME_YUKIMI_ETC` says (looking only).
#[test]
fn this_computer() {
    use std::sync::Arc;

    use aizome_yukimi::app::{Clock, Runner, Source};
    use aizome_yukimi::paths::Paths;

    let (Some(dir), true) = (std::env::var_os("YUKIMI_PICTURES"), std::env::var_os("AIZOME_YUKIMI_ETC").is_some())
    else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let runner = Runner::Mock(Arc::new(|_| Vec::new()));
    let mut app = Yukimi::with(Source::System(Paths::from_env()), runner, Clock::System);
    app.auto_check = false;
    let mut h = Harness::with(app, Appearance::default(), false);
    let w = h.window();
    settle(&mut h, |a| !a.loading && !a.reading_store);
    for page in Page::ALL {
        h.send(Msg::Show(page));
        settle(&mut h, |a| !a.indexing);
        h.save_png(w, dir.join(format!("real-{}.png", page.id())), 1.0);
        if page == Page::Updates && h.program().model.inputs.iter().any(|i| i.name == "sekka") {
            h.send(Msg::InspectChanges("sekka".into()));
            settle(&mut h, |a| a.comparison.as_ref().is_some_and(|c| c.details.is_some()));
            if let Some(comparison) = &h.program().comparison {
                let sheet = comparison.sheet;
                h.save_png(sheet, dir.join("real-comparison.png"), 1.0);
                if h.program().comparison.as_ref().unwrap().node().is_some_and(|n| !n.children.is_empty()) {
                    h.send(Msg::ComparisonInto(0));
                    h.save_png(sheet, dir.join("real-comparison-input.png"), 1.0);
                }
                h.send(Msg::ComparisonPackages(true));
                h.save_png(sheet, dir.join("real-comparison-packages.png"), 1.0);
                h.send(Msg::CloseSheet(sheet));
            }
        }
    }
}

/// The narrowest window Yukimi allows.
#[test]
fn narrow_pictures() {
    let Some(dir) = std::env::var_os("YUKIMI_PICTURES") else { return };
    let dir = std::path::PathBuf::from(dir);
    let appearance = Appearance { reduced_motion: true, ..Appearance::default() };
    let mut h = Harness::with(Yukimi::sample(), appearance, false);
    let w = h.window();
    h.resize(w, aizome_kit::layout::Size::new(760.0, 520.0));
    for page in [Page::Overview, Page::Updates, Page::Installed, Page::Storage] {
        h.send(Msg::Show(page));
        h.save_png(w, dir.join(format!("narrow-{}.png", page.id())), 1.0);
    }
}
