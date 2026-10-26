use aizome_network_agent::{
    Agent, AgentError, Engine, NM, PATH, Result,
    keyring::Store,
    model::{self, Profile, Prompt, Settings, text_value},
    process::{Cancel, run},
    prompt::Prompter,
    vpn::Plugins,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use zbus::zvariant::{OwnedObjectPath, Value};

const UUID: &str = "11111111-2222-3333-4444-555555555555";
#[derive(Default)]
struct Memory {
    values: Mutex<HashMap<(String, String, String), String>>,
    lookups: AtomicUsize,
}
impl Store for Memory {
    fn lookup(&self, uuid: &str, setting: &str, _: bool, _: &Cancel) -> Result<HashMap<String, String>> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .values
            .lock()
            .unwrap()
            .iter()
            .filter(|((u, s, _), _)| u == uuid && s == setting)
            .map(|((_, _, k), v)| (k.clone(), v.clone()))
            .collect())
    }
    fn saved(&self, uuid: &str, _: &Cancel) -> Result<Vec<(String, String)>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .keys()
            .filter(|(u, _, _)| u == uuid)
            .map(|(_, s, k)| (s.clone(), k.clone()))
            .collect())
    }
    fn store(&self, uuid: &str, setting: &str, key: &str, value: &str, _: &str, _: &Cancel) -> Result<()> {
        self.values.lock().unwrap().insert((uuid.into(), setting.into(), key.into()), value.into());
        Ok(())
    }
    fn forget(&self, uuid: &str, setting: &str, key: &str, _: &Cancel) -> Result<()> {
        self.values.lock().unwrap().remove(&(uuid.into(), setting.into(), key.into()));
        Ok(())
    }
    fn delete(&self, uuid: &str, _: &Cancel) -> Result<()> {
        self.values.lock().unwrap().retain(|(u, _, _), _| u != uuid);
        Ok(())
    }
}
impl Memory {
    fn get(&self, setting: &str, key: &str) -> Option<String> {
        self.values.lock().unwrap().get(&(UUID.into(), setting.into(), key.into())).cloned()
    }
}
struct Ui {
    asked: Mutex<Vec<Prompt>>,
    answer: Option<String>,
    wait: bool,
}
impl Prompter for Ui {
    fn ask(&self, prompt: Prompt, c: &Cancel) -> Result<HashMap<String, String>> {
        self.asked.lock().unwrap().push(prompt.clone());
        if self.wait {
            loop {
                c.check()?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let value = self.answer.as_ref().ok_or_else(|| AgentError::UserCanceled("Cancelled".into()))?;
        Ok(prompt.fields.iter().map(|f| (f.key.clone(), value.clone())).collect())
    }
}
fn profile(flags: u32) -> Settings {
    HashMap::from([
        (
            "connection".into(),
            HashMap::from([
                ("uuid".into(), text_value(UUID.into())),
                ("id".into(), text_value("Test AP".into())),
                ("type".into(), text_value("802-11-wireless".into())),
            ]),
        ),
        (
            "802-11-wireless-security".into(),
            HashMap::from([("key-mgmt".into(), text_value("wpa-psk".into())), ("psk-flags".into(), flags.into())]),
        ),
    ])
}
fn fixture(answer: Option<&str>, wait: bool) -> (Arc<Memory>, Arc<Ui>, Arc<Engine>) {
    let store = Arc::new(Memory::default());
    let ui = Arc::new(Ui { asked: Mutex::default(), answer: answer.map(str::to_owned), wait });
    let engine = Arc::new(Engine { store: store.clone(), ui: ui.clone(), plugins: Plugins { dirs: vec![] } });
    (store, ui, engine)
}

#[test]
fn storage_policy_and_noninteractive_requests() {
    for flags in [0, 1, 2, 3, 4, 5] {
        let (store, ui, engine) = fixture(Some("entered-secret"), false);
        let result = engine.get(
            Profile::new(profile(flags)).unwrap(),
            "802-11-wireless-security",
            &[],
            model::ALLOW_INTERACTION,
            &Cancel::default(),
        );
        if flags & model::NOT_REQUIRED != 0 {
            assert!(matches!(result, Err(AgentError::NoSecrets(_))));
        } else {
            assert_eq!(model::string(&result.unwrap(), "802-11-wireless-security", "psk"), "entered-secret");
        }
        assert_eq!(store.values.lock().unwrap().len(), usize::from(flags == 1), "flags {flags}");
        assert_eq!(ui.asked.lock().unwrap().len(), usize::from(flags & 4 == 0));
    }
    let (_, ui, engine) = fixture(Some("must-not-prompt"), false);
    assert!(matches!(
        engine.get(Profile::new(profile(2)).unwrap(), "802-11-wireless-security", &[], 0, &Cancel::default()),
        Err(AgentError::NoSecrets(_))
    ));
    assert!(ui.asked.lock().unwrap().is_empty());
}

#[test]
fn existing_keyring_and_request_new_and_system_owned_passwords() {
    let (store, ui, engine) = fixture(Some("replacement"), false);
    store.store(UUID, "802-11-wireless-security", "psk", "existing", "", &Cancel::default()).unwrap();
    let result =
        engine.get(Profile::new(profile(1)).unwrap(), "802-11-wireless-security", &[], 0, &Cancel::default()).unwrap();
    assert_eq!(model::string(&result, "802-11-wireless-security", "psk"), "existing");
    assert!(ui.asked.lock().unwrap().is_empty());
    let result =
        engine.get(Profile::new(profile(1)).unwrap(), "802-11-wireless-security", &[], 3, &Cancel::default()).unwrap();
    assert_eq!(model::string(&result, "802-11-wireless-security", "psk"), "replacement");
    let mut p = profile(0);
    p.get_mut("802-11-wireless-security").unwrap().insert("psk".into(), text_value("system-owned".into()));
    let reads = store.lookups.load(Ordering::SeqCst);
    let result = engine.get(Profile::new(p).unwrap(), "802-11-wireless-security", &[], 0, &Cancel::default()).unwrap();
    assert_eq!(model::string(&result, "802-11-wireless-security", "psk"), "system-owned");
    assert_eq!(store.lookups.load(Ordering::SeqCst), reads);
    assert_eq!(ui.asked.lock().unwrap().len(), 1);
}

#[test]
fn saved_and_deleted_credentials_respect_flags() {
    let (store, _, engine) = fixture(None, false);
    let mut p = profile(1);
    p.get_mut("802-11-wireless-security").unwrap().insert("psk".into(), text_value("saved".into()));
    p.insert(
        "802-1x".into(),
        HashMap::from([("password".into(), text_value("one-time".into())), ("password-flags".into(), 2u32.into())]),
    );
    engine.save(Profile::new(p).unwrap(), &Cancel::default()).unwrap();
    assert_eq!(store.values.lock().unwrap().len(), 1);
    store.delete(UUID, &Cancel::default()).unwrap();
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn user_cancellation_never_saves() {
    let (store, _, engine) = fixture(None, false);
    assert!(matches!(
        engine.get(Profile::new(profile(1)).unwrap(), "802-11-wireless-security", &[], 1, &Cancel::default()),
        Err(AgentError::UserCanceled(_))
    ));
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn enterprise_hints_and_wep_flags() {
    let mut p = profile(0);
    p.insert(
        "802-1x".into(),
        HashMap::from([
            ("eap".into(), Value::from(vec!["tls"]).try_to_owned().unwrap()),
            ("private-key-password-flags".into(), 2u32.into()),
        ]),
    );
    let p = Profile::new(p).unwrap();
    assert_eq!(p.fields("802-1x", &[]).unwrap()[0].key, "private-key-password");
    assert_eq!(p.fields("802-1x", &["pin".into()]).unwrap()[0].key, "pin");
    let mut p = profile(0);
    let s = p.get_mut("802-11-wireless-security").unwrap();
    s.insert("key-mgmt".into(), text_value("none".into()));
    s.insert("wep-tx-keyidx".into(), 2u32.into());
    s.insert("wep-key-flags".into(), 1u32.into());
    let p = Profile::new(p).unwrap();
    assert_eq!(p.fields("802-11-wireless-security", &[]).unwrap()[0].key, "wep-key2");
    assert_eq!(p.flags("802-11-wireless-security", "wep-key2"), 1);
}

#[test]
fn only_networkmanager_can_call_and_cancellation_interrupts_pending_prompt() {
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon is required");
    let mut address = String::new();
    BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let connect = || zbus::blocking::connection::Builder::address(address.trim()).unwrap().build().unwrap();
    let nm = connect();
    nm.request_name(NM).unwrap();
    let (store, ui, engine) = fixture(Some("secret"), true);
    // Listening once built: a call that came first would be lost.
    let connection = zbus::blocking::connection::Builder::address(address.trim())
        .unwrap()
        .serve_at(PATH, Agent::new(engine))
        .unwrap()
        .build()
        .unwrap();
    let dest = connection.unique_name().unwrap().as_str().to_owned();
    let args = || {
        (
            profile(1),
            OwnedObjectPath::try_from("/test/connection").unwrap(),
            "802-11-wireless-security",
            Vec::<String>::new(),
            1u32,
        )
    };
    let attacker = connect();
    let proxy =
        zbus::blocking::Proxy::new(&attacker, dest.as_str(), PATH, "org.freedesktop.NetworkManager.SecretAgent")
            .unwrap();
    let denied = proxy.call::<_, _, Settings>("GetSecrets", &args()).unwrap_err().to_string();
    assert!(denied.contains("NotAuthorized"), "{denied}");
    assert!(ui.asked.lock().unwrap().is_empty());
    assert_eq!(store.lookups.load(Ordering::SeqCst), 0);
    for method in ["SaveSecrets", "DeleteSecrets"] {
        let denied = proxy
            .call::<_, _, ()>(method, &(profile(1), OwnedObjectPath::try_from("/test/connection").unwrap()))
            .unwrap_err()
            .to_string();
        assert!(denied.contains("NotAuthorized"), "{denied}");
    }
    let denied = proxy
        .call::<_, _, ()>(
            "CancelGetSecrets",
            &(OwnedObjectPath::try_from("/test/connection").unwrap(), "802-11-wireless-security"),
        )
        .unwrap_err()
        .to_string();
    assert!(denied.contains("NotAuthorized"), "{denied}");
    let owned = nm.clone();
    let destination = dest.clone();
    let pending = std::thread::spawn(move || {
        let p = zbus::blocking::Proxy::new(&owned, destination, PATH, "org.freedesktop.NetworkManager.SecretAgent")
            .unwrap();
        p.call::<_, _, Settings>("GetSecrets", &args())
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui.asked.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Another caller's request for the same setting neither displaces nor
    // cancels NetworkManager's.
    let denied = proxy.call::<_, _, Settings>("GetSecrets", &args()).unwrap_err().to_string();
    assert!(denied.contains("NotAuthorized"), "{denied}");
    let target = (OwnedObjectPath::try_from("/test/connection").unwrap(), "802-11-wireless-security");
    assert!(proxy.call::<_, _, ()>("CancelGetSecrets", &target).is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert!(!pending.is_finished());
    assert_eq!(ui.asked.lock().unwrap().len(), 1);
    let proxy =
        zbus::blocking::Proxy::new(&nm, dest.as_str(), PATH, "org.freedesktop.NetworkManager.SecretAgent").unwrap();
    proxy
        .call::<_, _, ()>(
            "CancelGetSecrets",
            &(OwnedObjectPath::try_from("/test/connection").unwrap(), "802-11-wireless-security"),
        )
        .unwrap();
    let error = pending.join().unwrap().unwrap_err().to_string();
    assert!(error.contains("AgentCanceled"), "{error}");
    assert!(store.values.lock().unwrap().is_empty());
    daemon.kill().unwrap();
    daemon.wait().unwrap();
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.NetworkManager.AgentManager")]
enum ManagerError {
    Failed(String),
    PermissionDenied(String),
}
/// NetworkManager's agent manager, as far as registering goes: `answers`
/// are its replies in turn, then success.
struct AgentManager {
    calls: Arc<AtomicUsize>,
    answers: Vec<ManagerError>,
}
#[zbus::interface(name = "org.freedesktop.NetworkManager.AgentManager")]
impl AgentManager {
    fn register_with_capabilities(
        &mut self,
        identifier: String,
        _capabilities: u32,
    ) -> std::result::Result<(), ManagerError> {
        assert_eq!(identifier, "org.aizome.NetworkAgent");
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.answers.is_empty() { Ok(()) } else { Err(self.answers.remove(0)) }
    }
}
fn network_manager(
    connect: &dyn Fn() -> zbus::blocking::Connection,
    answers: Vec<ManagerError>,
) -> (zbus::blocking::Connection, Arc<AtomicUsize>) {
    let nm = connect();
    let calls = Arc::new(AtomicUsize::new(0));
    nm.object_server()
        .at("/org/freedesktop/NetworkManager/AgentManager", AgentManager { calls: calls.clone(), answers })
        .unwrap();
    nm.request_name(NM).unwrap();
    (nm, calls)
}
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn registers_once_per_networkmanager_and_ends_with_the_bus() {
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon is required");
    let mut address = String::new();
    BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let connect = || zbus::blocking::connection::Builder::address(address.trim()).unwrap().build().unwrap();
    // The first reply is lost, though NetworkManager registered the agent:
    // its second answer is that the agent is registered already.
    let (nm, calls) = network_manager(
        &connect,
        vec![
            ManagerError::Failed("Timed out".into()),
            ManagerError::PermissionDenied("An agent with this ID is already registered for this user.".into()),
        ],
    );
    let (_, ui, engine) = fixture(Some("secret"), true);
    let agent = zbus::blocking::connection::Builder::address(address.trim())
        .unwrap()
        .serve_at(PATH, Agent::new(engine))
        .unwrap()
        .build()
        .unwrap();
    let dest = agent.unique_name().unwrap().to_string();
    let served = std::thread::spawn(move || aizome_network_agent::run(agent).map_err(|e| e.to_string()));
    eventually("the agent asks again after a failure", || calls.load(Ordering::SeqCst) == 2);
    // A request now stays until NetworkManager goes.
    let owned = nm.clone();
    let pending = std::thread::spawn(move || {
        zbus::blocking::Proxy::new(&owned, dest, PATH, "org.freedesktop.NetworkManager.SecretAgent")
            .unwrap()
            .call::<_, _, Settings>(
                "GetSecrets",
                &(
                    profile(1),
                    OwnedObjectPath::try_from("/test/connection").unwrap(),
                    "802-11-wireless-security",
                    Vec::<String>::new(),
                    1u32,
                ),
            )
    });
    eventually("the dialog opens", || !ui.asked.lock().unwrap().is_empty());
    std::thread::sleep(aizome_network_agent::REGISTER_RETRY + Duration::from_millis(500));
    assert_eq!(calls.load(Ordering::SeqCst), 2, "the agent registered again");
    assert!(!pending.is_finished());
    // NetworkManager restarts: its request is cancelled, and the agent
    // registers with the new one.
    nm.release_name(NM).unwrap();
    let error = pending.join().unwrap().unwrap_err().to_string();
    assert!(error.contains("AgentCanceled"), "{error}");
    let (_restarted, calls) = network_manager(&connect, vec![]);
    eventually("the agent registers with the new NetworkManager", || calls.load(Ordering::SeqCst) == 1);
    // Without its bus the agent ends, for systemd to start it again.
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    eventually("the agent ends with the bus", || served.is_finished());
    assert!(served.join().unwrap().is_err());
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "aizome-agent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn script(&self, name: &str, source: &str) -> std::path::PathBuf {
        use std::io::Write;
        let path = self.0.join(name);
        // Written by another process: while this one has the file open to
        // write, a child another test forks inherits it, and running the
        // script fails as busy (ETXTBSY) until that child execs.
        let mut writer = Command::new("sh")
            .args(["-c", "cat > \"$0\" && chmod 700 \"$0\""])
            .arg(&path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        writer.stdin.take().unwrap().write_all(format!("#!/bin/sh\n{source}").as_bytes()).unwrap();
        assert!(writer.wait().unwrap().success());
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn vpn_profile() -> Profile {
    let mut p = profile(0);
    p.insert(
        "vpn".into(),
        HashMap::from([
            ("service-type".into(), text_value("org.example.test-vpn".into())),
            (
                "data".into(),
                Value::from(HashMap::from([("password-flags", "2"), ("username", "fixture-user")]))
                    .try_to_owned()
                    .unwrap(),
            ),
        ]),
    );
    Profile::new(p).unwrap()
}

#[test]
fn vpn_plugins_get_hints_and_data_and_external_dialogs_honor_interaction() {
    let dir = Directory::new();
    let script = dir.script(
        "authenticate",
        r#"
case "$*" in *"--external-ui-mode"*) ;; *) exit 1 ;; esac
case "$*" in *"-t challenge:otp"*) ;; *) exit 1 ;; esac
input=$(cat)
case "$input" in *"DATA_VAL=fixture-user"*) ;; *) exit 1 ;; esac
cat <<'EOF'
[VPN Plugin UI]
Version=2
Title=Authentication
Description=Enter the token\nfrom your device.
[password]
Label=One-time password
IsSecret=true
ShouldAsk=true
Value=
[opaque-state]
Label=State
IsSecret=true
ShouldAsk=false
Value=plugin-state
EOF
"#,
    );
    std::fs::write(dir.0.join("test.name"), format!(
        "[VPN Connection]\nservice=org.example.test-vpn\n[GNOME]\nauth-dialog={}\nsupports-external-ui-mode=true\nsupports-hints=true\n",
        script.display())).unwrap();
    let plugins = Plugins { dirs: vec![dir.0.clone()] };
    let (_, ui, _) = fixture(Some("otp-response"), false);
    assert!(matches!(
        plugins.secrets(&vpn_profile(), &["challenge:otp".into()], 0, HashMap::new(), ui.as_ref(), &Cancel::default()),
        Err(AgentError::NoSecrets(_))
    ));
    assert!(ui.asked.lock().unwrap().is_empty());
    let values = plugins
        .secrets(&vpn_profile(), &["challenge:otp".into()], 3, HashMap::new(), ui.as_ref(), &Cancel::default())
        .unwrap();
    assert_eq!(values["password"], "otp-response");
    assert_eq!(values["opaque-state"], "plugin-state");
    assert_eq!(ui.asked.lock().unwrap()[0].message, "Enter the token\nfrom your device.");
}

#[test]
fn legacy_vpn_plugin_can_supply_its_own_ui_and_unknown_plugins_fail_closed() {
    let dir = Directory::new();
    let helper = dir.script("authenticate", "cat >/dev/null\nprintf 'password\\nplugin-password\\n\\n'\n");
    std::fs::write(
        dir.0.join("test.name"),
        format!("[VPN Connection]\nservice=org.example.test-vpn\n[GNOME]\nauth-dialog={}\n", helper.display()),
    )
    .unwrap();
    let plugins = Plugins { dirs: vec![dir.0.clone()] };
    let (_, ui, _) = fixture(None, false);
    let result = plugins.secrets(&vpn_profile(), &[], 1, HashMap::new(), ui.as_ref(), &Cancel::default()).unwrap();
    assert_eq!(result["password"], "plugin-password");
    assert!(ui.asked.lock().unwrap().is_empty());
    let mut p = vpn_profile();
    p.settings.get_mut("vpn").unwrap().insert("service-type".into(), text_value("/bin/sh".into()));
    assert!(matches!(
        plugins.secrets(&p, &[], 1, HashMap::new(), ui.as_ref(), &Cancel::default()),
        Err(AgentError::NoSecrets(_))
    ));
}

/// Whether a process has ended. One killed may stay a zombie until
/// something reaps it.
fn gone(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_or(true, |stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| rest.starts_with('Z')))
}

fn child_of(dir: &Directory) -> String {
    let file = dir.0.join("child");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(pid) = std::fs::read_to_string(&file).ok().filter(|s| s.ends_with('\n')) {
            return pid.trim().into();
        }
        assert!(Instant::now() < deadline, "the helper started no child");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn failed() -> AgentError {
    AgentError::Failed("Worker failed".into())
}

#[test]
fn cancellation_terminates_blocked_helpers_and_their_children() {
    let dir = Directory::new();
    let helper = dir.script("blocked", &format!("sleep 60 &\necho $! > {}\nwait\n", dir.0.join("child").display()));
    let cancel = Cancel::default();
    let token = cancel.clone();
    let worker = std::thread::spawn(move || run(&mut Command::new(helper), Vec::new(), &token, failed()));
    let child = child_of(&dir);
    let start = Instant::now();
    cancel.cancel();
    assert!(matches!(worker.join().unwrap(), Err(AgentError::AgentCanceled(_))));
    assert!(start.elapsed() < Duration::from_secs(2));
    while !gone(&child) {
        assert!(start.elapsed() < Duration::from_secs(2), "the helper's child survived cancellation");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_finished_helper_returns_at_once_and_leaves_what_it_started() {
    let dir = Directory::new();
    // The child holds the helper's output open, as a browser opened for
    // single sign-on would.
    let helper = dir.script(
        "sso",
        &format!("sleep 60 &\necho $! > {}\nprintf 'password\\nvalue\\n\\n'\n", dir.0.join("child").display()),
    );
    let start = Instant::now();
    let output = run(&mut Command::new(helper), Vec::new(), &Cancel::default(), failed()).unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(output, b"password\nvalue\n\n");
    let child = child_of(&dir);
    assert!(!gone(&child), "the helper's child was killed");
    unsafe {
        libc::kill(child.parse().unwrap(), libc::SIGKILL);
    }
}

#[test]
fn input_and_output_larger_than_a_pipe_pass_through() {
    let dir = Directory::new();
    let echo = dir.script("echo", "exec cat\n");
    let input: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    assert_eq!(run(&mut Command::new(echo), input.clone(), &Cancel::default(), failed()).unwrap(), input);
}

#[test]
fn only_a_vpn_helper_exiting_non_zero_counts_as_cancelled() {
    let dir = Directory::new();
    let failing = dir.script("failing", "cat >/dev/null\nexit 1\n");
    // Aizome's own dialog and keyring workers: another agent may still ask.
    assert!(matches!(
        run(&mut Command::new(&failing), Vec::new(), &Cancel::default(), failed()),
        Err(AgentError::Failed(_))
    ));
    std::fs::write(
        dir.0.join("test.name"),
        format!("[VPN Connection]\nservice=org.example.test-vpn\n[GNOME]\nauth-dialog={}\n", failing.display()),
    )
    .unwrap();
    let plugins = Plugins { dirs: vec![dir.0.clone()] };
    let (_, ui, _) = fixture(None, false);
    assert!(matches!(
        plugins.secrets(&vpn_profile(), &[], 1, HashMap::new(), ui.as_ref(), &Cancel::default()),
        Err(AgentError::UserCanceled(_))
    ));
}

#[test]
fn one_deadline_covers_every_step_of_a_request() {
    let dir = Directory::new();
    let slow = dir.script("slow", "exec sleep 60\n");
    let quick = dir.script("quick", "exit 0\n");
    let cancel = Cancel::within(Duration::from_millis(300));
    let start = Instant::now();
    assert!(matches!(run(&mut Command::new(slow), Vec::new(), &cancel, failed()), Err(AgentError::AgentCanceled(_))));
    assert!(start.elapsed() < Duration::from_secs(2));
    // The next step has no time of its own left.
    assert!(matches!(run(&mut Command::new(quick), Vec::new(), &cancel, failed()), Err(AgentError::AgentCanceled(_))));
}

#[test]
fn changing_to_ask_every_time_clears_previously_saved_secrets() {
    let (store, _, engine) = fixture(None, false);
    store.store(UUID, "802-11-wireless-security", "psk", "old-password", "", &Cancel::default()).unwrap();
    engine.save(Profile::new(profile(2)).unwrap(), &Cancel::default()).unwrap();
    assert!(store.values.lock().unwrap().is_empty());
}

/// A Wi-Fi profile with an enterprise password too, as NetworkManager
/// sends it with SaveSecrets after an update that carried no secrets, for
/// a connection it hasn't activated since it started: flags, no secrets.
fn enterprise_profile(psk_flags: u32, password_flags: u32) -> Settings {
    let mut p = profile(psk_flags);
    p.insert("802-1x".into(), HashMap::from([("password-flags".into(), password_flags.into())]));
    p
}

fn saved_twice(store: &Memory) {
    let c = Cancel::default();
    // Perhaps by another desktop.
    store.store(UUID, "802-11-wireless-security", "psk", "saved-psk", "", &c).unwrap();
    store.store(UUID, "802-1x", "password", "saved-password", "", &c).unwrap();
}

#[test]
fn saving_a_profile_update_without_secrets_keeps_the_saved_ones() {
    let (store, _, engine) = fixture(None, false);
    saved_twice(&store);
    // Settei turning "Connect automatically" off, say.
    engine.save(Profile::new(enterprise_profile(1, 1)).unwrap(), &Cancel::default()).unwrap();
    assert_eq!(store.get("802-11-wireless-security", "psk").as_deref(), Some("saved-psk"));
    assert_eq!(store.get("802-1x", "password").as_deref(), Some("saved-password"));
    // A secret the update carries replaces only its own entry.
    let mut p = enterprise_profile(1, 1);
    p.get_mut("802-11-wireless-security").unwrap().insert("psk".into(), text_value("new-psk".into()));
    engine.save(Profile::new(p).unwrap(), &Cancel::default()).unwrap();
    assert_eq!(store.get("802-11-wireless-security", "psk").as_deref(), Some("new-psk"));
    assert_eq!(store.get("802-1x", "password").as_deref(), Some("saved-password"));
}

#[test]
fn saving_forgets_only_the_secrets_a_profile_stops_keeping() {
    // Agent-owned; then system-owned, ask every time (alone and with
    // agent-owned) and not required.
    for (flags, kept) in [(1, true), (0, false), (2, false), (3, false), (4, false)] {
        let (store, _, engine) = fixture(None, false);
        saved_twice(&store);
        engine.save(Profile::new(enterprise_profile(flags, 1)).unwrap(), &Cancel::default()).unwrap();
        assert_eq!(store.get("802-11-wireless-security", "psk").is_some(), kept, "flags {flags}");
        assert_eq!(store.get("802-1x", "password").as_deref(), Some("saved-password"), "flags {flags}");
    }
    // NetworkManager leaves out flags at their default, system-owned, and
    // a setting the profile no longer has keeps nothing.
    let (store, _, engine) = fixture(None, false);
    saved_twice(&store);
    let mut p = profile(1);
    p.get_mut("802-11-wireless-security").unwrap().remove("psk-flags");
    engine.save(Profile::new(p).unwrap(), &Cancel::default()).unwrap();
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn wireguard_peer_secrets_use_nm_keys_and_nested_reply() {
    let public = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let key = format!("peers.{public}.preshared-key");
    let mut p = profile(0);
    p.insert(
        "wireguard".into(),
        HashMap::from([
            ("private-key-flags".into(), 1u32.into()),
            (
                "peers".into(),
                Value::from(vec![HashMap::from([
                    ("public-key", text_value(public.into())),
                    ("preshared-key-flags", 2u32.into()),
                ])])
                .try_to_owned()
                .unwrap(),
            ),
        ]),
    );
    let p = Profile::new(p).unwrap();
    assert_eq!(p.fields("wireguard", &[]).unwrap().len(), 2);
    assert_eq!(p.fields("wireguard", std::slice::from_ref(&key)).unwrap()[0].key, key);
    assert_eq!(p.flags("wireguard", &key), 2);
    let (_, _, engine) = fixture(Some("credential"), false);
    let reply = engine.get(p, "wireguard", &[], 1, &Cancel::default()).unwrap();
    assert_eq!(model::string(&reply, "wireguard", "private-key"), "credential");
    let peers: Vec<HashMap<String, zbus::zvariant::OwnedValue>> =
        reply["wireguard"]["peers"].try_clone().unwrap().try_into().unwrap();
    assert_eq!(<&str>::try_from(&peers[0]["public-key"]).unwrap(), public);
    assert_eq!(<&str>::try_from(&peers[0]["preshared-key"]).unwrap(), "credential");
}

struct UnavailableKeyring;
impl Store for UnavailableKeyring {
    fn lookup(&self, _: &str, _: &str, _: bool, _: &Cancel) -> Result<HashMap<String, String>> {
        Err(AgentError::Failed("Keyring unavailable".into()))
    }
    fn saved(&self, _: &str, _: &Cancel) -> Result<Vec<(String, String)>> {
        Err(AgentError::Failed("Keyring unavailable".into()))
    }
    fn store(&self, _: &str, _: &str, _: &str, _: &str, _: &str, _: &Cancel) -> Result<()> {
        Err(AgentError::Failed("Keyring unavailable".into()))
    }
    fn forget(&self, _: &str, _: &str, _: &str, _: &Cancel) -> Result<()> {
        Ok(())
    }
    fn delete(&self, _: &str, _: &Cancel) -> Result<()> {
        Ok(())
    }
}

#[test]
fn unavailable_keyring_does_not_prevent_interactive_authentication() {
    let (_, ui, _) = fixture(Some("usable-password"), false);
    let engine = Engine { store: Arc::new(UnavailableKeyring), ui: ui.clone(), plugins: Plugins { dirs: vec![] } };
    assert!(matches!(
        engine.get(Profile::new(profile(1)).unwrap(), "802-11-wireless-security", &[], 0, &Cancel::default()),
        Err(AgentError::NoSecrets(_))
    ));
    assert!(ui.asked.lock().unwrap().is_empty());
    let reply =
        engine.get(Profile::new(profile(1)).unwrap(), "802-11-wireless-security", &[], 1, &Cancel::default()).unwrap();
    assert_eq!(model::string(&reply, "802-11-wireless-security", "psk"), "usable-password");
}
