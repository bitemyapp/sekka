//! A Fusen that never launched leaves the stickies alone (SAP-06): a second
//! launch hands its request to the running Fusen and exits, and mustn't
//! save its own (empty) stickies over the running one's, or delete their
//! login item.
//!
//! End to end: the running Fusen is the real program in the headless
//! harness, serving the instance socket; the second launch is the real
//! `aizome-fusen` binary with the same private HOME and XDG dirs.
use aizome_fusen::app::{Fusen, Msg, Places, Request};
use aizome_kit::testing::Harness;
use aizome_sumi::instance::{self, Claim};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const ENTRY: &str = "org.aizome.Fusen.Stickies.desktop";

struct Env {
    home: PathBuf,
    /// A runtime dir of its own, short enough for the socket's path.
    runtime: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fusen-second-launch").join(name);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Notes")).unwrap();
        let runtime = std::env::temp_dir().join(format!("fusen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&runtime);
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(home.join("Notes/Groceries.md"), "Groceries\n- [ ] miso\n").unwrap();
        Env { home, runtime }
    }

    /// What `Places::user()` gives under this HOME and these XDG dirs.
    fn places(&self) -> Places {
        Places {
            notes: self.home.join("Notes"),
            state: Some(self.home.join(".local/state/aizome/fusen")),
            trash: Some(self.home.join(".local/share/Trash")),
            autostart: Some(self.home.join(".config/autostart")),
            compositor: false,
        }
    }

    fn app(&self, request: Request) -> Fusen {
        let mut f = Fusen::new(self.places(), request);
        f.open_links = false;
        f
    }

    fn stickies_json(&self) -> String {
        std::fs::read_to_string(self.home.join(".local/state/aizome/fusen/stickies.json")).unwrap_or_default()
    }

    fn login_item(&self) -> bool {
        self.home.join(".config/autostart").join(ENTRY).exists()
    }

    /// Keep "Groceries" as a sticky, as a run of Fusen that quit did.
    fn keep_a_sticky(&self) {
        let mut h = Harness::new(self.app(Request::default()));
        let g = h.program().notes.iter().find(|n| n.title == "Groceries").unwrap().id;
        h.send(Msg::Sticky(Some(g)));
        assert!(self.stickies_json().contains("Groceries.md"), "{}", self.stickies_json());
        assert!(self.login_item(), "a login item brings the sticky back");
    }

    fn assert_kept(&self, after: &str) {
        let (json, entry) = (self.stickies_json(), self.login_item());
        assert!(
            json.contains("Groceries.md") && entry,
            "after {after}: stickies.json = {json:?}, login item present = {entry}"
        );
    }

    fn launch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aizome-fusen"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("AIZOME_SOCKET")
            .env_remove("AIZOME_SHELL_SOCKET")
            .output()
            .expect("run aizome-fusen")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.runtime);
    }
}

#[test]
fn a_second_launch_keeps_the_stickies_and_their_login_item() {
    let env = Env::new("handed");
    // The running Fusen, with one sticky, serving the socket the binary
    // looks for.
    let Claim::First(listener) = instance::claim_at(env.runtime.join("aizome-app.fusen.sock"), &[]) else {
        panic!("the running Fusen claims the socket")
    };
    let mut f = env.app(Request::default());
    f.instance = Some(listener);
    let mut h = Harness::new(f);
    let g = h.program().notes.iter().find(|n| n.title == "Groceries").unwrap().id;
    h.send(Msg::Sticky(Some(g)));
    env.assert_kept("a sticky was opened");

    // The second launch, as the shell's Services "New Note" runs it.
    let out = env.launch(&["--new", "From Services\nselected text"]);
    assert!(out.status.success(), "{out:?}");
    let until = Instant::now() + Duration::from_secs(10);
    while !env.home.join("Notes/From Services.md").exists() {
        assert!(Instant::now() < until, "the running Fusen never got the request");
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::Nothing);
    }
    env.assert_kept("a second launch handed its request over");

    // Killed before its next save (no Drop), then the next login.
    std::mem::forget(h);
    let next = Harness::new(env.app(Request { stickies_only: true, ..Request::default() }));
    assert_eq!(next.program().stickies.len(), 1, "the sticky comes back at the next login");
}

#[test]
fn a_fusen_that_never_launched_saves_nothing() {
    let env = Env::new("unlaunched");
    env.keep_a_sticky();
    drop(env.app(Request::default()));
    env.assert_kept("a Fusen was built and dropped");
}
