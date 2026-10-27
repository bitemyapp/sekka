//! Yukimi driven headlessly on the sample system: navigating, searching,
//! scrolling, and the try / install for me / install for everyone, remove,
//! update, go back and clean up flows, with stand-ins for Nix and the
//! helper that print what Nix would. Nothing here changes the machine.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_kit::WindowKey;
use aizome_kit::layout::{Point, Vec2};
use aizome_kit::testing::Harness;
use aizome_yukimi::app::{Changes, Clock, Launch, Msg, Page, Runner, Source, Yukimi, launch_from_args};
use aizome_yukimi::job::Outcome;
use aizome_yukimi::ops::{Operation, Run, Step, SystemChange};

/// What a stand-in step prints: progress on its error output, as Nix's
/// `internal-json` log, and the helper's report.
const WORKS: &str = r#"echo '@nix {"action":"start","id":1,"level":0,"type":0,"text":"Building the new system","fields":[]}' >&2
echo '@nix {"action":"start","id":2,"level":4,"type":104,"text":"","fields":[]}' >&2
echo '@nix {"action":"result","id":2,"type":105,"fields":[1,1,0,0]}' >&2
echo 'building /nix/store/0123456789abcdfghijklmnpqrsvwxyz-gimp2-2.10.38.drv' >&2
echo '{"ok":true}'"#;

const FAILS: &str = r#"echo 'building the new system' >&2
echo '@nix {"action":"msg","level":0,"msg":"error: builder for gimp2 failed with exit code 1"}' >&2
exit 1"#;

/// A step that runs `script` in place of Nix or the helper.
fn script(text: &'static str) -> Step {
    Box::new(move || {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", text]);
        Ok(Some(Run { command, as_root: false }))
    })
}

/// The sample, with every operation it runs written down and run as
/// `script`.
fn sample(script_text: &'static str) -> (Harness<Yukimi>, WindowKey, Arc<Mutex<Vec<Operation>>>) {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let seen = ran.clone();
    let model = Arc::new(aizome_yukimi::fixture::model());
    let runner = Runner::Mock(Arc::new(move |op: &Operation| {
        seen.lock().unwrap().push(op.clone());
        vec![script(script_text)]
    }));
    let mut app = Yukimi::with(Source::Sample(model.clone()), runner, Clock::Fixed(aizome_yukimi::fixture::NOW, 21));
    app.check = Some(Arc::new(aizome_yukimi::fixture::check(&model)));
    app.index = Some(Arc::new(aizome_yukimi::fixture::index()));
    let h = Harness::new(app);
    let w = h.window();
    (h, w, ran)
}

/// Let background work answer, until `done` (or five seconds).
fn settle(h: &mut Harness<Yukimi>, done: impl Fn(&Yukimi) -> bool) {
    let start = Instant::now();
    while !done(h.program()) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::NoteGone(0));
    }
    assert!(done(h.program()), "it settled");
}

fn has(h: &Harness<Yukimi>, w: WindowKey, text: &str) -> bool {
    h.texts(w).iter().any(|t| t == text)
}

#[test]
fn the_sidebar_goes_to_each_page() {
    let (mut h, w, _) = sample(WORKS);
    assert_eq!(h.program().page, Page::Overview);
    assert!(has(&h, w, "ganymede"));
    for (title, page) in [
        ("Installed", Page::Installed),
        ("Discover", Page::Discover),
        ("Updates", Page::Updates),
        ("History", Page::History),
        ("Storage", Page::Storage),
        ("Overview", Page::Overview),
    ] {
        let at =
            h.semantics(w).into_iter().find(|(s, _)| s.label.as_deref() == Some(title)).map(|(_, r)| r).expect(title);
        h.click(w, at.center());
        assert_eq!(h.program().page, page, "{title}");
    }
    // The menu's shortcuts go there too.
    h.command(w, '4');
    assert_eq!(h.program().page, Page::Updates);
}

#[test]
fn searching_finds_packages_and_opens_one() {
    let (mut h, w, _) = sample(WORKS);
    h.send(Msg::Show(Page::Discover));
    // The search field takes the keyboard when Discover opens.
    h.type_text(w, "image");
    assert_eq!(h.program().query, "image");
    settle(&mut h, |a| a.found.as_ref().is_some_and(|f| f.query == "image"));
    assert!(has(&h, w, "imagemagick"));
    assert!(has(&h, w, "broken"));
    // A newer query's answer is the one shown.
    h.send(Msg::Search("gimp".into()));
    settle(&mut h, |a| a.found.as_ref().is_some_and(|f| f.query == "gimp"));
    assert!(has(&h, w, "gimp2"));
    // The catalog's application by that name comes first.
    assert!(has(&h, w, "GIMP"));
    assert!(h.click_text(w, "gimp2"));
    let details = h.program().details.as_ref().map(|(k, _)| *k).expect("the details sheet");
    assert!(has(&h, details, "GNU Image Manipulation Program, the 2.10 series"));
    assert!(has(&h, details, "Install for Everyone"));
}

#[test]
fn implicit_catalog_apps_are_installed_but_are_not_written_when_adding_another_app() {
    let mut model = aizome_yukimi::fixture::model();
    model.store = None;
    model.catalog = yukimi_system::catalog::parse(
        r#"[{"id":"fish","name":"fish","description":"The desktop's shell.","category":"Terminals","packages":["fish"]},
            {"id":"kitty","name":"Kitty","description":"A terminal.","category":"Terminals","packages":["kitty"]}]"#,
    )
    .unwrap();
    for app in &mut model.catalog {
        app.setting = Some("calamares.applications".into());
    }
    model.configured.settings.insert("calamares.applications".into(), vec!["firefox".into()]);
    model.installer_applications = ["firefox".into(), "fish".into()].into();
    let model = Arc::new(model);
    let ran = Arc::new(Mutex::new(Vec::new()));
    let seen = ran.clone();
    let runner = Runner::Mock(Arc::new(move |op: &Operation| {
        seen.lock().unwrap().push(op.clone());
        vec![script(WORKS)]
    }));
    let mut app = Yukimi::with(Source::Sample(model), runner, Clock::Fixed(aizome_yukimi::fixture::NOW, 21));
    app.index = Some(Arc::new(aizome_yukimi::fixture::index()));
    let mut h = Harness::new(app);
    let w = h.window();
    h.send(Msg::Show(Page::Discover));
    assert!(has(&h, w, "fish"));
    assert!(h.semantics(w).iter().any(|(s, _)| s.label.as_deref() == Some("Installed") && s.disabled));
    // Only Kitty can be installed; fish already has a disabled Installed button.
    assert_eq!(h.semantics(w).iter().filter(|(s, _)| s.label.as_deref() == Some("Install")).count(), 1);
    assert!(h.click_text(w, "Install"));
    assert!(has(&h, w, "Install Kitty?"));
    assert!(ran.lock().unwrap().is_empty());
    assert!(click_last(&mut h, w, "Install"));
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    assert_eq!(
        ran.lock().unwrap().as_slice(),
        &[Operation::ChangeSystem {
            change: SystemChange {
                applications: Some(("calamares.applications".into(), vec!["firefox".into(), "kitty".into()])),
                ..SystemChange::default()
            },
            what: "Kitty".into(),
            adding: true,
        }]
    );
}

#[test]
fn install_for_everyone_asks_first_then_runs_the_helper() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::Discover));
    let gimp2 = aizome_yukimi::fixture::index().get("gimp2").cloned().unwrap();
    h.send(Msg::Open(gimp2));
    let details = h.program().details.as_ref().map(|(k, _)| *k).unwrap();
    assert!(h.click_text(details, "Install for Everyone"));
    // Nothing runs before the question is answered.
    assert!(ran.lock().unwrap().is_empty());
    assert!(has(&h, w, "Install gimp2 for everyone?"), "{:?}", h.texts(w));
    assert!(click_last(&mut h, w, "Install"));
    let expected = Operation::ChangeSystem {
        change: SystemChange {
            packages: Some(vec!["gimp".into(), "inkscape".into(), "ripgrep".into(), "gimp2".into()]),
            ..SystemChange::default()
        },
        what: "gimp2".into(),
        adding: true,
    };
    assert_eq!(ran.lock().unwrap().as_slice(), std::slice::from_ref(&expected));
    // What the helper is asked, as the GTK Yukimi asks it.
    assert_eq!(expected.helper_args(None), ["change", "--packages", "gimp,inkscape,ripgrep,gimp2", "--mode", "auto"]);
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    let job = h.program().job.as_ref().unwrap();
    assert_eq!(job.outcome, Some(Outcome::Done("gimp2 is installed for everyone".into())));
    assert!(job.log.iter().any(|l| l.contains("gimp2-2.10.38.drv")));
    // The sheet says so a moment, then goes; the note stays in the status line.
    let sheet = h.program().job_sheet.unwrap();
    assert!(has(&h, sheet, "gimp2 is installed for everyone"));
    h.advance(Duration::from_secs(1));
    assert!(h.program().job.is_none());
    assert!(has(&h, w, "gimp2 is installed for everyone"));
}

#[test]
fn install_for_me_and_try_it_need_no_password() {
    let (mut h, _, ran) = sample(WORKS);
    let htop = aizome_yukimi::fixture::index().get("htop").cloned().unwrap();
    h.send(Msg::InstallForMe(htop.clone()));
    assert_eq!(ran.lock().unwrap().as_slice(), [Operation::InstallForMe { attr: "htop".into(), unfree: false }]);
    assert!(!ran.lock().unwrap()[0].is_system());
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    h.advance(Duration::from_secs(1));
    // Trying runs nothing of Yukimi's: a terminal opens with nix shell.
    h.send(Msg::TryIt(htop));
    assert_eq!(ran.lock().unwrap().len(), 1);
    assert!(h.program().notes.iter().any(|(_, n)| n == "Trying htop in a terminal"));
}

#[test]
fn a_failure_stays_with_its_log() {
    let (mut h, _, _) = sample(FAILS);
    h.send(Msg::Run(Operation::Rollback { generation: 26 }));
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    let sheet = h.program().job_sheet.unwrap();
    let job = h.program().job.as_ref().unwrap();
    let Some(Outcome::Failed(why)) = &job.outcome else { panic!("{:?}", job.outcome) };
    assert!(why.starts_with("error: builder for gimp2 failed"), "{why}");
    assert!(!why.contains("was put back"));
    assert!(!why.contains("system was not changed"));
    assert!(job.details, "the log shows");
    assert!(has(&h, sheet, "building the new system"));
    // It stays until closed.
    h.advance(Duration::from_secs(5));
    assert!(h.program().job.is_some());
    assert!(h.click_text(sheet, "Close"), "{:?}", h.texts(sheet));
    assert!(h.program().job.is_none());
}

#[test]
fn stopping_stops() {
    let (mut h, _, _) = sample("sleep 30");
    let started = Instant::now();
    h.send(Msg::Run(Operation::Clean { older_than_days: Some(30) }));
    std::thread::sleep(Duration::from_millis(100));
    h.send(Msg::StopJob);
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    assert!(started.elapsed() < Duration::from_secs(10));
    let Some(Outcome::Failed(why)) = &h.program().job.as_ref().unwrap().outcome else { panic!() };
    assert!(why.starts_with("Stopped."), "{why}");
}

#[test]
fn removing_takes_just_that_entry_out() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::Installed));
    assert!(has(&h, w, "ADDED WITH YUKIMI"));
    h.send(Msg::Remove(aizome_yukimi::app::Removal::Package("ripgrep".into())));
    assert!(has(&h, w, "Remove ripgrep?"));
    assert!(click_last(&mut h, w, "Remove"));
    assert_eq!(
        ran.lock().unwrap().as_slice(),
        [Operation::ChangeSystem {
            change: SystemChange { packages: Some(vec!["gimp".into(), "inkscape".into()]), ..SystemChange::default() },
            what: "ripgrep".into(),
            adding: false,
        }]
    );
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    h.advance(Duration::from_secs(1));
    // An entry written in configuration.nix comes out of its list.
    h.send(Msg::Remove(aizome_yukimi::app::Removal::Listed { attr: "wget".into(), user: None }));
    assert!(click_last(&mut h, w, "Remove"));
    let last = ran.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.helper_args(None), ["change", "--unlist-system", "wget", "--mode", "auto"]);
}

#[test]
fn updating_takes_what_moves_forward() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::Updates));
    assert!(has(&h, w, "newer version"));
    assert!(has(&h, w, "couldn't check"));
    assert!(has(&h, w, "Changes"));
    assert!(has(&h, w, "Pinned commit"));
    assert!(h.texts(w).iter().any(|t| t.contains("Pinned:") && t.contains("Target:")));
    // On a Sekka system one update takes Sekka and every newer source.
    assert!(h.click_text(w, "Update"));
    let op = ran.lock().unwrap().first().cloned().unwrap();
    assert_eq!(op, Operation::UpdateSekka { inputs: vec!["nixpkgs".into(), "sekka".into()] });
    assert_eq!(op.helper_args(None), ["change", "--sekka", "--mode", "auto", "--update", "nixpkgs,sekka"]);
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
}

#[test]
fn going_back_and_cleaning_up_ask_first() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::History));
    h.send(Msg::ToggleGeneration(27));
    settle(&mut h, |a| matches!(a.changes.get(&27), Some(Changes::Ready { .. })));
    // What 27 changed from 26: it has its packages, and can be returned to.
    assert!(has(&h, w, "Return…"));
    assert!(h.click_text(w, "Return…"));
    assert!(has(&h, w, "Return to generation 27?"));
    assert!(click_last(&mut h, w, "Return"));
    assert_eq!(ran.lock().unwrap().as_slice(), [Operation::Rollback { generation: 27 }]);
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    h.advance(Duration::from_secs(1));

    h.send(Msg::Show(Page::Storage));
    h.send(Msg::Older(2));
    h.send(Msg::CleanUp);
    assert!(has(&h, w, "Clean up the store?"));
    assert!(click_last(&mut h, w, "Clean up"));
    assert_eq!(ran.lock().unwrap().last(), Some(&Operation::Clean { older_than_days: Some(90) }));
}

#[test]
fn the_running_generation_shows_what_it_changed() {
    let (mut h, w, _) = sample(WORKS);
    h.send(Msg::Show(Page::History));
    h.send(Msg::ToggleGeneration(28));
    settle(&mut h, |a| matches!(a.changes.get(&28), Some(Changes::Ready { .. })));
    for name in ["firefox", "linux", "nodejs", "gimp", "vlc"] {
        assert!(has(&h, w, name), "{name}");
    }
    assert!(has(&h, w, "24.9.0 → 22.20.0"));
    // The running one can't be returned to: it's where the system is.
    assert!(!has(&h, w, "Return…"));
    h.send(Msg::ToggleGeneration(28));
    assert!(!has(&h, w, "vlc"));
}

#[test]
fn pages_scroll() {
    let (mut h, w, _) = sample(WORKS);
    h.send(Msg::Show(Page::Installed));
    let before = h.find(w, "ADDED WITH YUKIMI").expect("the first group");
    h.scroll(w, Point::new(600.0, 400.0), Vec2::new(0.0, 300.0));
    let after = h.find(w, "ADDED WITH YUKIMI");
    assert!(after.is_none_or(|r| r.y0 < before.y0 - 200.0), "{before:?} {after:?}");
    // Each page keeps its own place.
    h.send(Msg::Show(Page::Storage));
    assert!(h.find(w, "The Nix store").is_some_and(|r| r.y0 < 120.0));
}

#[test]
fn nothing_is_changed_when_looking_at_another_system() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let seen = ran.clone();
    let dir = std::env::temp_dir().join(format!("aizome-yukimi-elsewhere-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("nixos")).unwrap();
    std::fs::write(
        dir.join("nixos/configuration.nix"),
        "{ pkgs, ... }: { environment.systemPackages = [ pkgs.git ]; }\n",
    )
    .unwrap();
    let paths = aizome_yukimi::paths::Paths { etc: dir.clone(), elsewhere: true };
    let runner = Runner::Mock(Arc::new(move |op: &Operation| {
        seen.lock().unwrap().push(op.clone());
        Vec::new()
    }));
    let mut h = Harness::new(Yukimi::with(Source::System(paths), runner, Clock::System));
    settle(&mut h, |a| !a.loading);
    assert_eq!(h.program().model.configured.system.len(), 1);
    h.send(Msg::Run(Operation::InstallForMe { attr: "htop".into(), unfree: false }));
    h.send(Msg::InstallForEveryone("htop".into()));
    assert!(ran.lock().unwrap().is_empty());
    assert!(h.program().job.is_none());
    assert!(h.program().notes.iter().any(|(_, n)| n.contains("changes nothing")));
    let _ = std::fs::remove_dir_all(dir);
}

/// Click the last button called `label`: an alert's, over a page that
/// has one of the same name.
fn click_last(h: &mut Harness<Yukimi>, w: WindowKey, label: &str) -> bool {
    let found =
        h.semantics(w).into_iter().filter(|(s, _)| s.label.as_deref() == Some(label)).map(|(_, r)| r).next_back();
    match found {
        Some(r) => {
            h.click(w, r.center());
            true
        }
        None => false,
    }
}

#[test]
fn the_rest_of_the_system_shows_all_and_filters() {
    let (mut h, w, _) = sample(WORKS);
    h.send(Msg::Show(Page::Installed));
    // 40 more than the groups above show: the first 25, then "Show all".
    assert!(has(&h, w, "Show all 40"), "{:?}", h.texts(w));
    // It's further down: scroll to it.
    h.scroll(w, Point::new(600.0, 400.0), Vec2::new(0.0, 4000.0));
    assert!(h.click_text(w, "Show all 40"));
    assert!(h.program().show_all);
    assert!(!has(&h, w, "Show all 40"));
    assert!(has(&h, w, "aizome"));
    // Filtering starts from the first rows again, and says when nothing matches.
    h.send(Msg::Filter("mes".into()));
    assert!(!h.program().show_all);
    assert!(has(&h, w, "mesa"));
    assert!(!has(&h, w, "aizome"));
    h.send(Msg::Filter("emacs".into()));
    assert!(has(&h, w, "Nothing called “emacs” is part of the system."));
}

fn pick(setting: &str, id: &str, on: bool) -> Msg {
    Msg::Pick { setting: setting.into(), id: id.into(), on }
}

#[test]
fn the_desktops_and_login_screen_are_chosen_then_applied_once() {
    let (mut h, w, ran) = sample(WORKS);
    assert!(h.click_text(w, "Desktops"), "the sidebar offers Desktops");
    assert_eq!(h.program().page, Page::Desktops);
    for text in ["Desktops and login", "Default desktop", "Login screen", "Automatic", "Plasma Login"] {
        assert!(h.texts(w).iter().any(|t| t.eq_ignore_ascii_case(text)), "{text}: {:?}", h.texts(w));
    }
    // Picking changes nothing yet.
    h.send(pick("calamares.desktops", "gnome", false));
    h.send(pick("calamares.loginScreen", "aizome", true));
    assert!(ran.lock().unwrap().is_empty());
    assert!(has(&h, w, "not applied yet"));
    assert!(has(&h, w, "removing"));
    // Apply asks first, saying what changes.
    h.send(Msg::ApplyChoices);
    assert!(ran.lock().unwrap().is_empty());
    assert!(has(&h, w, "Apply these changes?"), "{:?}", h.texts(w));
    let body = h.texts(w).into_iter().find(|t| t.starts_with("Desktops: removing GNOME.")).expect("what changes");
    assert!(body.contains("Login screen: Aizome, instead of Automatic."), "{body}");
    assert!(click_last(&mut h, w, "Apply"));
    let expected = Operation::Choose {
        choices: vec![
            ("calamares.desktops".into(), vec!["aizome".into(), "tatami".into()]),
            ("calamares.loginScreen".into(), vec!["aizome".into()]),
        ],
        what: "the desktops and the login screen".into(),
    };
    assert_eq!(ran.lock().unwrap().as_slice(), std::slice::from_ref(&expected));
    assert_eq!(
        expected.helper_args(None),
        [
            "change",
            "--choose",
            "calamares.desktops=aizome,tatami",
            "--choose",
            "calamares.loginScreen=aizome",
            "--mode",
            "auto"
        ]
    );
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    assert_eq!(
        h.program().job.as_ref().unwrap().outcome,
        Some(Outcome::Done("The desktops and the login screen changed".into()))
    );
}

#[test]
fn a_default_desktop_stays_among_the_desktops() {
    let (mut h, _, _) = sample(WORKS);
    h.send(Msg::Show(Page::Desktops));
    // Taking out the default desktop moves the default to one that's left.
    h.send(pick("calamares.desktops", "aizome", false));
    assert_eq!(h.program().chosen("calamares.defaultDesktop"), ["tatami"]);
    assert!(h.program().choice_problems().is_empty(), "{:?}", h.program().choice_problems());
    // Putting it back puts the default back.
    h.send(pick("calamares.desktops", "aizome", true));
    assert_eq!(h.program().chosen("calamares.defaultDesktop"), ["aizome"]);
    assert!(h.program().choice_changes().is_empty(), "{:?}", h.program().choice_changes());
    // One chosen on purpose stays.
    h.send(pick("calamares.defaultDesktop", "gnome", true));
    h.send(pick("calamares.desktops", "aizome", false));
    h.send(pick("calamares.desktops", "aizome", true));
    assert_eq!(h.program().chosen("calamares.defaultDesktop"), ["gnome"]);
    // A value that isn't offered isn't taken.
    h.send(pick("calamares.defaultDesktop", "windows", true));
    assert_eq!(h.program().chosen("calamares.defaultDesktop"), ["gnome"]);
    h.send(Msg::RevertChoices);
    assert!(h.program().choice_changes().is_empty());
}

#[test]
fn what_an_option_needs_must_be_chosen_before_applying() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::Desktops));
    h.send(pick("calamares.loginScreen", "aizome", true));
    h.send(pick("calamares.desktops", "aizome", false));
    let problems = h.program().choice_problems();
    assert_eq!(problems, ["Aizome as the login screen needs Aizome among the desktops"], "{problems:?}");
    assert!(has(&h, w, "Aizome as the login screen needs Aizome among the desktops."));
    h.send(Msg::ApplyChoices);
    assert!(!has(&h, w, "Apply these changes?"));
    assert!(ran.lock().unwrap().is_empty());
    // Every desktop out is a problem too.
    h.send(Msg::RevertChoices);
    for id in ["aizome", "tatami", "gnome"] {
        h.send(pick("calamares.desktops", id, false));
    }
    assert_eq!(h.program().choice_problems(), ["Choose at least one of the desktops"]);
}

#[test]
fn early_updates_ask_then_follow_sekkas_main() {
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Show(Page::Updates));
    assert!(has(&h, w, "Early updates"));
    h.send(Msg::Early(true));
    assert!(has(&h, w, "Turn on early updates?"), "{:?}", h.texts(w));
    assert!(ran.lock().unwrap().is_empty());
    assert!(click_last(&mut h, w, "Turn On"));
    assert_eq!(ran.lock().unwrap().as_slice(), [Operation::Early { on: true }]);
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    // Already following the nightly: nothing to ask.
    h.advance(Duration::from_secs(1));
    h.send(Msg::Early(false));
    assert!(h.program().notes.iter().any(|(_, n)| n == "Sekka updates from the nightly already"));
    assert_eq!(ran.lock().unwrap().len(), 1);
}

#[test]
fn a_launch_proposes_and_changes_nothing_by_itself() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        launch_from_args(&args(&["--choose", "calamares.loginScreen=aizome", "--early"])).unwrap(),
        Launch { page: None, choose: vec![("calamares.loginScreen".into(), vec!["aizome".into()])], early: Some(true) }
    );
    assert_eq!(launch_from_args(&args(&["--page", "desktops"])).unwrap().page, Some(Page::Desktops));
    for bad in [&["--choose"][..], &["--choose", "x"], &["--choose", "a b=c"], &["--choose", "a="], &["--page", "x"]] {
        assert!(launch_from_args(&args(bad)).is_err(), "{bad:?}");
    }
    assert!(launch_from_args(&args(&["--choose", "a=b", "--choose", "a=c"])).is_err(), "once each");

    // A running Yukimi handed a launch shows what it proposes.
    let (mut h, w, ran) = sample(WORKS);
    h.send(Msg::Launched(args(&["--choose", "calamares.loginScreen=aizome", "--early"])));
    assert_eq!(h.program().page, Page::Desktops);
    assert_eq!(h.program().choice_changes(), [("calamares.loginScreen".to_owned(), vec!["aizome".to_owned()])]);
    assert!(has(&h, w, "Turn on early updates?"));
    assert!(ran.lock().unwrap().is_empty());
    // What the system doesn't offer is only noted.
    h.send(Msg::Launched(args(&["--choose", "services.openssh.enable=true"])));
    h.send(Msg::Launched(args(&["--choose", "calamares.loginScreen=lightdm"])));
    let notes: Vec<&str> = h.program().notes.iter().map(|(_, n)| n.as_str()).collect();
    assert!(notes.contains(&"services.openssh.enable isn't one of this system's choices"), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("\"lightdm\" isn't one of the choices")), "{notes:?}");
    assert_eq!(h.program().chosen("calamares.loginScreen"), ["aizome"]);
}

#[test]
fn a_system_without_choices_offers_no_desktops_page() {
    let mut model = aizome_yukimi::fixture::model();
    model.setup.facts.choices.clear();
    let app = Yukimi::with(
        Source::Sample(Arc::new(model)),
        Runner::Mock(Arc::new(|_| Vec::new())),
        Clock::Fixed(aizome_yukimi::fixture::NOW, 21),
    );
    let h = Harness::new(app);
    let w = h.window();
    assert!(!has(&h, w, "Desktops"));
}

#[test]
fn comparisons_open_a_sheet_drill_into_inputs_and_filter_packages() {
    let (mut h, w, ran) = sample(WORKS);
    h.resize(w, aizome_kit::layout::Size::new(760.0, 520.0));
    h.send(Msg::Show(Page::Updates));
    h.scroll(w, Point::new(600.0, 400.0), Vec2::new(0.0, 400.0));
    assert!(h.click_text(w, "Changes"));
    settle(&mut h, |a| a.comparison.as_ref().is_some_and(|c| c.details.is_some()));
    let sheet = h.program().comparison.as_ref().unwrap().sheet;
    // Input reads can fail without taking away the direct link.
    assert!(has(&h, sheet, "Open commit diff"));
    h.send(Msg::ComparisonLoaded(sheet, Arc::new(aizome_yukimi::fixture::comparison())));
    assert!(has(&h, sheet, "aizome"));
    assert!(h.click_text(sheet, "Inspect"));
    assert_eq!(h.program().comparison.as_ref().unwrap().node().unwrap().name, "aizome");
    assert!(has(&h, sheet, "nixpkgs"));
    assert!(h.click_text(sheet, "Inspect"));
    assert!(h.texts(sheet).iter().any(|t| t.contains("No changed inputs")));
    assert!(h.click_text(sheet, "Back"));
    assert_eq!(h.program().comparison.as_ref().unwrap().node().unwrap().name, "aizome");
    assert!(h.click_text(sheet, "Package changes"));
    h.send(Msg::ComparisonQuery("firefox".into()));
    assert!(has(&h, sheet, "firefox"));
    assert!(!has(&h, sheet, "htop"));
    assert!(h.click_text(sheet, "Source changes"));
    assert_eq!(h.program().comparison.as_ref().unwrap().node().unwrap().name, "aizome");
    assert!(h.click_text(sheet, "Done"));
    assert!(h.program().comparison.is_none());
    // Closing/reopening cannot accept a late result from the old sheet.
    h.send(Msg::InspectChanges("sekka".into()));
    settle(&mut h, |a| a.comparison.as_ref().is_some_and(|c| c.details.is_some()));
    let new_sheet = h.program().comparison.as_ref().unwrap().sheet;
    assert_ne!(sheet, new_sheet);
    h.send(Msg::ComparisonLoaded(sheet, Arc::new(aizome_yukimi::fixture::comparison())));
    assert!(h.program().comparison.as_ref().unwrap().details.as_ref().unwrap().source_error.is_some());
    h.named(new_sheet, aizome_kit::sidestep_ui::NamedKey::Escape, aizome_kit::sidestep_ui::Modifiers::default());
    assert!(h.program().comparison.is_none());
    assert!(ran.lock().unwrap().is_empty(), "inspecting must not run update operations");
}

#[test]
fn authentication_failure_offers_an_explicit_retry_and_runs_it_once() {
    let (mut h, _, ran) = sample(WORKS);
    let operation = Operation::Rollback { generation: 26 };
    // A normal operation opens the sheet. Injecting the result below uses
    // that same sheet, avoiding any fixture-only widget behavior.
    h.send(Msg::Run(operation.clone()));
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    let id = h.program().job.as_ref().unwrap().id;
    h.send(Msg::Job(
        id,
        aizome_yukimi::job::JobEvent::AuthorizationFailed(aizome_yukimi::job::AuthorizationFailure::Unavailable),
    ));
    let sheet = h.program().job_sheet.unwrap();
    assert!(has(
        &h,
        sheet,
        "The administrator password dialog is unavailable. Nothing was changed. Try again when it is available."
    ));
    assert!(h.click_text(sheet, "Try Again"));
    settle(&mut h, |a| a.job.as_ref().is_some_and(|j| j.outcome.is_some()));
    assert_eq!(ran.lock().unwrap().as_slice(), [operation.clone(), operation]);
}
