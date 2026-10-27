//! The service against fake CUPS tools: shell scripts that record their
//! arguments and environment and print canned output, so every command
//! line is checked exactly and nothing touches the real CUPS. The `live_`
//! tests (ignored by default) only read this machine's CUPS.

use super::parse::ptype;
use super::parse::tests::{LPSTAT_A, LPSTAT_LO, LPSTAT_LP, LPSTAT_V, lpq_fixture};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// A directory of fake tools under the system's temporary directory,
/// removed when dropped.
struct Fake {
    dir: PathBuf,
}

/// What a fake tool does for one command line (`"$*"`): an exact match,
/// `prefix*`, or `*` for anything.
struct Case {
    args: String,
    stdout: Vec<u8>,
    stderr: String,
    status: i32,
}

fn ok(args: &str, stdout: impl AsRef<[u8]>) -> Case {
    Case { args: args.to_owned(), stdout: stdout.as_ref().to_vec(), stderr: String::new(), status: 0 }
}

fn fails(args: &str, stderr: &str, status: i32) -> Case {
    Case { args: args.to_owned(), stdout: Vec::new(), stderr: stderr.to_owned(), status }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

impl Fake {
    fn new() -> Fake {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aizome-printers-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(dir.join("bin")).expect("a test directory");
        std::fs::create_dir_all(dir.join("home/.cups")).expect("a test directory");
        Fake { dir }
    }

    fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    fn tools(&self) -> Tools {
        Tools { user_lpoptions: Some(self.dir.join("home/.cups/lpoptions")), ..Tools::in_dir(self.bin()) }
    }

    /// A script `name` that logs its call and answers `cases`.
    fn tool(&self, name: &str, cases: Vec<Case>) {
        let bin = self.bin();
        let file = |suffix: &str| bin.join(format!("{name}.{suffix}")).display().to_string();
        let mut script = format!(
            "#!/bin/sh\n\
             {{ printf '%s\\037' \"$@\"; printf '\\n'; }} >> {calls}\n\
             printf 'LC_ALL=%s LANG=%s stdin=%s sid=%s pid=%s\\n' \"$LC_ALL\" \"$LANG\" \
             \"$(readlink /proc/self/fd/0)\" \"$(cut -d' ' -f6 /proc/$$/stat)\" \"$$\" >> {env}\n\
             case \"$*\" in\n",
            calls = quote(&file("calls")),
            env = quote(&file("env")),
        );
        for (index, case) in cases.iter().enumerate() {
            let pattern = match case.args.as_str() {
                "*" => "*".to_owned(),
                args => match args.strip_suffix('*') {
                    Some(prefix) => format!("{}*", quote(prefix)),
                    None => quote(args),
                },
            };
            self.set(name, index, case);
            script.push_str(&format!(
                "{pattern}) cat {out}; cat {err} >&2; exit \"$(cat {status})\";;\n",
                out = quote(&file(&format!("{index}.out"))),
                err = quote(&file(&format!("{index}.err"))),
                status = quote(&file(&format!("{index}.status"))),
            ));
        }
        script.push_str(&format!("*) printf 'fake {name}: unexpected arguments: %s\\n' \"$*\" >&2; exit 99;;\nesac\n"));
        self.script(name, &script);
    }

    /// Change what a tool answers for its `index`th case.
    fn set(&self, name: &str, index: usize, case: &Case) {
        let path = |suffix: &str| self.bin().join(format!("{name}.{index}.{suffix}"));
        std::fs::write(path("out"), &case.stdout).expect("a canned output");
        std::fs::write(path("err"), &case.stderr).expect("a canned output");
        std::fs::write(path("status"), case.status.to_string()).expect("a canned status");
    }

    /// An executable script `name` with `body`.
    fn script(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.bin().join(name);
        std::fs::write(&path, body).expect("a script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("an executable");
    }

    /// Each call's arguments.
    fn calls(&self, name: &str) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.bin().join(format!("{name}.calls"))).unwrap_or_default();
        text.lines()
            .map(|line| {
                let mut args: Vec<String> = line.split('\x1f').map(str::to_owned).collect();
                args.pop();
                args
            })
            .collect()
    }

    fn count(&self, name: &str, args: &[&str]) -> usize {
        self.calls(name).iter().filter(|call| call.as_slice() == args).count()
    }

    /// Each call's environment line.
    fn env(&self, name: &str) -> Vec<String> {
        let text = std::fs::read_to_string(self.bin().join(format!("{name}.env"))).unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

/// Finished jobs, as `lpstat -W completed -l -o` prints them.
const COMPLETED: &str = "\
Office-Laser-40         callen            1024   Sat Oct 17 14:00:00 2026
\tAlerts: job-completed-successfully
\tqueued for Office-Laser
Office-Laser-41         callen            2048   Sat Oct 17 14:05:00 2026
\tAlerts: job-canceled-by-user
\tqueued for Office-Laser
old_inkjet-39           bob               1024   Fri Oct 16 10:00:00 2026
\tStatus: The printer is not responding.
\tAlerts: aborted-by-system
\tqueued for old_inkjet
";

/// The lpstat cases [`cups`] sets up, by index, for tests that change them.
const LPSTAT_D: usize = 5;

/// A CUPS with five printers (see `parse`'s fixtures) and four jobs.
fn cups(fake: &Fake) {
    fake.tool(
        "lpstat",
        vec![
            ok("-r", "scheduler is running\n"),
            ok("-l -p", LPSTAT_LP),
            ok("-a", LPSTAT_A),
            ok("-v", LPSTAT_V),
            ok("-W not-completed -l -o", LPSTAT_LO),
            ok("-d", "system default destination: Office-Laser\n"),
            ok("-W completed -l -o", COMPLETED),
        ],
    );
    let base = 0x904c; // black, duplex, copies, collate, variable sizes
    fake.tool(
        "lpoptions",
        vec![
            ok(
                "-p Brother_HL_L2390DW",
                format!(
                    "copies=1 device-uri=dnssd://Brother%20HL-L2390DW._ipp._tcp.local/ \
                     printer-info='Brother HL-L2390DW' printer-is-accepting-jobs=true \
                     printer-is-shared=false printer-is-temporary=false \
                     printer-location='Study, 2nd floor' \
                     printer-make-and-model='Brother HL-L2390DW series - IPP Everywhere' \
                     printer-state=3 printer-state-reasons=none printer-type={}\n",
                    base | ptype::NOT_SHARED
                ),
            ),
            ok(
                "-p Office-Laser",
                format!(
                    "printer-info='Bürodrucker – 3. Stock 藍' printer-location \
                     printer-make-and-model='HP LaserJet Pro M404-M405, driverless, cups-filters 2.0.1' \
                     printer-is-shared=true printer-state=4 printer-type={}\n",
                    base | ptype::DEFAULT
                ),
            ),
            fails("-p old_inkjet", "lpoptions: Unknown printer or class.\n", 1),
            ok("-p Stopped_one", format!("printer-make-and-model='Local Raw Printer' printer-type={base}\n")),
            ok(
                "-p Front-Desk_2",
                format!("printer-info='Front desk' printer-type={}\n", base | ptype::REMOTE | ptype::NOT_SHARED),
            ),
            ok("-d *", ""),
        ],
    );
    fake.tool("lpq", vec![ok("-a", lpq_fixture())]);
}

/// A handle on `fake` that polls once at start (and then hourly), loaded.
fn handle(fake: &Fake) -> Printers {
    let printers = Printers::with_interval(fake.tools(), Wake::none(), Duration::from_secs(3600));
    wait_for(|| printers.snapshot().loaded);
    printers
}

fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_state_from_every_source() {
    let fake = Fake::new();
    cups(&fake);
    let state = cli::query(&fake.tools(), None).expect("a state");

    assert!(state.loaded && state.available);
    assert_eq!(state.error, None);
    let names: Vec<&str> = state.printers.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Brother_HL_L2390DW", "Office-Laser", "old_inkjet", "Stopped_one", "Front-Desk_2"]);

    let brother = state.printer("brother_hl_l2390dw").expect("found ignoring case");
    assert_eq!(brother.description, "Brother HL-L2390DW");
    assert_eq!(brother.location, "Study, 2nd floor");
    assert_eq!(brother.make_and_model.as_deref(), Some("Brother HL-L2390DW series - IPP Everywhere"));
    assert_eq!(
        brother.device_uri.as_deref(),
        Some("dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000-80ce-11db-8000-c0b5d7e5cc7d"),
        "lpstat -v wins over lpoptions"
    );
    assert_eq!(brother.state, PrinterState::Idle);
    assert!(brother.accepting && brother.enabled);
    assert_eq!(brother.shared, Some(false));
    assert_eq!(brother.driverless, Some(true));
    assert!(!brother.is_system_default && !brother.is_user_default);
    assert!(!brother.temporary && !brother.remote && !brother.is_class);

    let office = state.printer("Office-Laser").expect("a printer");
    assert_eq!(office.state, PrinterState::Printing);
    assert_eq!(office.current_job.as_deref(), Some("Office-Laser-42"));
    assert_eq!(office.state_message.as_deref(), Some("Rendering completed"));
    assert_eq!(office.state_reasons, ["toner-low-report", "media-empty-warning"]);
    assert_eq!(office.description, "Bürodrucker – 3. Stock 藍");
    assert_eq!(office.location, "");
    assert_eq!(office.shared, Some(true));
    assert_eq!(office.driverless, Some(true));
    assert!(office.is_system_default);

    let old = state.printer("old_inkjet").expect("a printer");
    assert_eq!(old.state, PrinterState::Stopped);
    assert!(old.is_paused() && !old.enabled && !old.accepting);
    assert_eq!(old.state_message.as_deref(), Some("Paused"));
    assert_eq!(old.make_and_model, None, "lpoptions failed for it");
    assert_eq!((old.shared, old.driverless), (None, None));

    let raw = state.printer("Stopped_one").expect("a printer");
    assert_eq!(raw.driverless, Some(false));
    assert_eq!(raw.display_name(), "Stopped_one");

    let front = state.printer("Front-Desk_2").expect("a printer");
    assert!(front.remote);
    assert_eq!(front.shared, Some(false), "from printer-type's not-shared bit");
    assert_eq!(front.state_reasons, ["hold-new-jobs"]);

    assert_eq!(state.system_default.as_deref(), Some("Office-Laser"));
    assert_eq!(state.user_default, None);
    assert_eq!(state.default_printer().map(|p| p.name.as_str()), Some("Office-Laser"));

    let jobs: Vec<(&str, JobState, Option<&str>)> =
        state.jobs.iter().map(|j| (j.id.as_str(), j.state, j.title.as_deref())).collect();
    assert_eq!(
        jobs,
        [
            ("Office-Laser-42", JobState::Processing, Some("Quarterly report.pdf")),
            ("Brother_HL_L2390DW-43", JobState::Held, Some("Aizome tests, chapter 藍染")),
            ("Front-Desk_2-44", JobState::Pending, None),
            ("Office-Laser-45", JobState::Pending, Some("untitled")),
        ]
    );
    assert_eq!(state.jobs_on("office-laser").count(), 2);
    assert!(state.jobs.iter().all(|job| job.state.is_active()));

    // Every query ran as asked, under LC_ALL=C, without a terminal.
    assert_eq!(fake.count("lpstat", &["-W", "not-completed", "-l", "-o"]), 1);
    assert_eq!(fake.calls("lpq"), [args(&["-a"])]);
    assert_eq!(fake.calls("lpoptions").len(), 5);
    for line in fake.env("lpstat") {
        assert!(line.starts_with("LC_ALL=C LANG=C stdin=/dev/null "), "{line}");
        let session = line.split_once("sid=").and_then(|(_, rest)| rest.split_once(" pid="));
        let (sid, pid) = session.expect("a session and a pid");
        assert_eq!(sid, pid, "each tool leads a session of its own");
    }
}

#[test]
fn the_users_own_default() {
    let fake = Fake::new();
    cups(&fake);
    std::fs::write(
        fake.dir.join("home/.cups/lpoptions"),
        "Dest Office-Laser media=A4\nDefault Brother_HL_L2390DW sides=two-sided-long-edge\n",
    )
    .expect("an lpoptions file");
    // lpstat -d then reports the user's choice.
    fake.set("lpstat", LPSTAT_D, &ok("-d", "system default destination: Brother_HL_L2390DW\n"));
    let state = cli::query(&fake.tools(), None).expect("a state");
    assert_eq!(state.user_default.as_deref(), Some("Brother_HL_L2390DW"));
    assert_eq!(state.system_default.as_deref(), Some("Office-Laser"), "from printer-type");
    assert!(state.printer("Brother_HL_L2390DW").is_some_and(|p| p.is_user_default && !p.is_system_default));
    assert!(state.printer("Office-Laser").is_some_and(|p| p.is_system_default && !p.is_user_default));
    assert_eq!(state.default_printer().map(|p| p.name.as_str()), Some("Brother_HL_L2390DW"));

    let tools = Tools { env_default: Some("Front-Desk_2".into()), ..fake.tools() };
    let state = cli::query(&tools, None).expect("a state");
    assert_eq!(state.user_default.as_deref(), Some("Front-Desk_2"), "LPDEST wins over the file");

    // A user default naming a printer that is gone falls back to the system's.
    std::fs::write(fake.dir.join("home/.cups/lpoptions"), "Default Gone\n").expect("an lpoptions file");
    let state = cli::query(&fake.tools(), None).expect("a state");
    assert_eq!(state.user_default.as_deref(), Some("Gone"));
    assert_eq!(state.default_printer().map(|p| p.name.as_str()), Some("Office-Laser"));
}

#[test]
fn no_printers_is_not_an_error() {
    let fake = Fake::new();
    let none = "lpstat: No destinations added.\n";
    fake.tool(
        "lpstat",
        vec![
            ok("-r", "scheduler is running\n"),
            fails("-l -p", none, 1),
            fails("-a", none, 1),
            fails("-v", none, 1),
            ok("-W not-completed -l -o", ""),
            ok("-d", "no system default destination\n"),
        ],
    );
    let state = cli::query(&fake.tools(), None).expect("a state");
    assert!(state.available && state.printers.is_empty() && state.jobs.is_empty());
    assert_eq!((state.system_default, state.user_default, state.error), (None, None, None));
    assert!(fake.calls("lpq").is_empty(), "no jobs, no titles to ask for");
}

#[test]
fn cups_not_there() {
    let fake = Fake::new();
    fake.tool("lpstat", vec![ok("-r", "scheduler is not running\n")]);
    assert!(matches!(cli::query(&fake.tools(), None), Err(Error::Unavailable(_))));

    let missing = Fake::new();
    assert!(matches!(cli::query(&missing.tools(), None), Err(Error::Unavailable(_))));

    let printers = Printers::with_interval(missing.tools(), Wake::none(), Duration::from_secs(3600));
    wait_for(|| printers.snapshot().loaded);
    let state = printers.snapshot();
    assert!(!printers.available() && state.printers.is_empty());
    assert!(matches!(state.error, Some(Error::Unavailable(_))));
}

#[test]
fn a_failed_refresh_keeps_the_last_lists() {
    let fake = Fake::new();
    cups(&fake);
    let printers = handle(&fake);
    assert_eq!(printers.snapshot().printers.len(), 5);

    fake.set("lpstat", 1, &fails("-l -p", "lpstat: Forbidden\n", 1));
    let result = printers.refresh().wait();
    assert_eq!(result, Err(Error::NotAuthorized("Forbidden".into())));
    let state = printers.snapshot();
    assert_eq!(state.error, Some(Error::NotAuthorized("Forbidden".into())));
    assert_eq!(state.printers.len(), 5);
    assert!(state.available);

    fake.set("lpstat", 0, &ok("-r", "scheduler is not running\n"));
    assert!(matches!(printers.refresh().wait(), Err(Error::Unavailable(_))));
    let state = printers.snapshot();
    assert!(!state.available && state.printers.is_empty() && state.jobs.is_empty());
}

#[test]
fn the_poller_publishes_only_changes_and_stops_with_the_handle() {
    let fake = Fake::new();
    cups(&fake);
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let wake = Wake::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let printers = Printers::with_interval(fake.tools(), wake, Duration::from_millis(50));
    wait_for(|| printers.snapshot().loaded);
    let generation = printers.generation();
    let woken = wakes.load(Ordering::SeqCst);
    let polls = fake.count("lpstat", &["-r"]);
    wait_for(|| fake.count("lpstat", &["-r"]) >= polls + 3);
    assert_eq!(printers.generation(), generation, "nothing changed, nothing published");
    assert_eq!(wakes.load(Ordering::SeqCst), woken);

    fake.set("lpstat", LPSTAT_D, &ok("-d", "system default destination: Brother_HL_L2390DW\n"));
    wait_for(|| printers.snapshot().system_default.as_deref() == Some("Brother_HL_L2390DW"));
    assert!(printers.generation() > generation);
    assert!(wakes.load(Ordering::SeqCst) > woken);

    drop(printers);
    std::thread::sleep(Duration::from_millis(500));
    let after = fake.count("lpstat", &["-r"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.count("lpstat", &["-r"]), after, "the poller ended with its handle");
}

#[test]
fn adding_printers() {
    let fake = Fake::new();
    cups(&fake);
    fake.tool("lpadmin", vec![ok("*", "")]);
    let printers = handle(&fake);
    let polls = fake.count("lpstat", &["-r"]);

    let new = NewPrinter {
        name: "Office-Laser_2".into(),
        device_uri: "ipp://10.0.0.5/ipp/print".into(),
        driver: DriverChoice::Everywhere,
        description: Some("Office laser – 3rd floor".into()),
        location: Some("Room 301".into()),
    };
    assert_eq!(printers.add_printer(new.clone()).wait(), Ok(()));
    assert_eq!(fake.count("lpstat", &["-r"]), polls + 1, "the action refreshed");

    let model = NewPrinter {
        driver: DriverChoice::Model("drv:///sample.drv/generic.ppd".into()),
        description: None,
        location: Some(String::new()),
        ..new.clone()
    };
    assert_eq!(printers.add_printer(model).wait(), Ok(()));

    let ppd = fake.dir.join("my printer.ppd");
    std::fs::write(&ppd, "*PPD-Adobe: \"4.3\"\n").expect("a PPD");
    let with_ppd =
        NewPrinter { driver: DriverChoice::Ppd(ppd.clone()), description: None, location: None, ..new.clone() };
    assert_eq!(printers.add_printer(with_ppd).wait(), Ok(()));

    let ppd = ppd.canonicalize().expect("the PPD").display().to_string();
    assert_eq!(
        fake.calls("lpadmin"),
        [
            args(&[
                "-p",
                "Office-Laser_2",
                "-E",
                "-v",
                "ipp://10.0.0.5/ipp/print",
                "-m",
                "everywhere",
                "-D",
                "Office laser – 3rd floor",
                "-L",
                "Room 301"
            ]),
            args(&[
                "-p",
                "Office-Laser_2",
                "-E",
                "-v",
                "ipp://10.0.0.5/ipp/print",
                "-m",
                "drv:///sample.drv/generic.ppd"
            ]),
            args(&["-p", "Office-Laser_2", "-E", "-v", "ipp://10.0.0.5/ipp/print", "-P", &ppd]),
        ]
    );

    // Refused before anything runs.
    let refused = [
        NewPrinter { name: "My Printer".into(), ..new.clone() },
        NewPrinter { name: "-x".into(), ..new.clone() },
        NewPrinter { name: "a/b".into(), ..new.clone() },
        NewPrinter { name: String::new(), ..new.clone() },
        NewPrinter { device_uri: "  ".into(), ..new.clone() },
        NewPrinter { device_uri: "-v".into(), ..new.clone() },
        NewPrinter { description: Some("two\nlines".into()), ..new.clone() },
        NewPrinter { location: Some("x".repeat(128)), ..new.clone() },
        NewPrinter { driver: DriverChoice::Model("--help".into()), ..new.clone() },
    ];
    for printer in refused {
        let result = printers.add_printer(printer.clone()).wait();
        assert!(matches!(result, Err(Error::InvalidArgument(_))), "{printer:?}: {result:?}");
    }
    let missing = NewPrinter { driver: DriverChoice::Ppd(fake.dir.join("nope.ppd")), ..new };
    assert!(matches!(printers.add_printer(missing).wait(), Err(Error::NotFound(_))));
    assert_eq!(fake.calls("lpadmin").len(), 3);
}

#[test]
fn printer_actions_run_the_right_commands() {
    let fake = Fake::new();
    cups(&fake);
    for tool in ["lpadmin", "cupsenable", "cupsdisable", "cupsaccept", "cupsreject", "cancel"] {
        fake.tool(tool, vec![ok("*", "")]);
    }
    let printers = handle(&fake);

    let results = [
        printers.remove_printer("old_inkjet").wait(),
        printers.set_system_default("Brother_HL_L2390DW").wait(),
        printers.set_user_default("Office-Laser").wait(),
        printers.pause("Office-Laser").wait(),
        printers.resume("Office-Laser").wait(),
        printers.set_accepting("Front-Desk_2", false).wait(),
        printers.set_accepting("Front-Desk_2", true).wait(),
        printers.set_shared("Brother_HL_L2390DW", true).wait(),
        printers.set_shared("Brother_HL_L2390DW", false).wait(),
        printers.cancel_job("Office-Laser-42").wait(),
        printers.cancel_job("43").wait(),
        printers.cancel_all_jobs("Office-Laser").wait(),
    ];
    assert!(results.iter().all(Result::is_ok), "{results:?}");

    assert_eq!(
        fake.calls("lpadmin"),
        [
            args(&["-x", "old_inkjet"]),
            args(&["-d", "Brother_HL_L2390DW"]),
            args(&["-p", "Brother_HL_L2390DW", "-o", "printer-is-shared=true"]),
            args(&["-p", "Brother_HL_L2390DW", "-o", "printer-is-shared=false"]),
        ]
    );
    assert_eq!(fake.count("lpoptions", &["-d", "Office-Laser"]), 1);
    assert_eq!(fake.calls("cupsdisable"), [args(&["Office-Laser"])]);
    assert_eq!(fake.calls("cupsenable"), [args(&["Office-Laser"])]);
    assert_eq!(fake.calls("cupsreject"), [args(&["Front-Desk_2"])]);
    assert_eq!(fake.calls("cupsaccept"), [args(&["Front-Desk_2"])]);
    assert_eq!(fake.calls("cancel"), [args(&["Office-Laser-42"]), args(&["43"]), args(&["-a", "Office-Laser"])]);
    // One refresh at start and one after each action.
    assert_eq!(fake.count("lpstat", &["-r"]), 1 + results.len());

    let refused = [
        printers.remove_printer("").wait(),
        printers.pause("-a").wait(),
        printers.set_shared("two words", true).wait(),
        printers.cancel_job("Office Laser-1").wait(),
        printers.cancel_job("-1").wait(),
        printers.cancel_job("Office-Laser").wait(),
        printers.cancel_all_jobs("x\ny").wait(),
    ];
    for result in refused {
        assert!(matches!(result, Err(Error::InvalidArgument(_))), "{result:?}");
    }
    assert_eq!(fake.calls("cancel").len(), 3);
}

#[test]
fn failures_say_what_went_wrong() {
    let fake = Fake::new();
    cups(&fake);
    fake.tool(
        "lpadmin",
        vec![
            fails("-x Office-Laser", "lpadmin: Forbidden\n", 1),
            fails("-x old_inkjet", "lpadmin: Unable to connect to server: Connection refused\n", 1),
            fails("-d Office-Laser", "lpadmin: The printer or class does not exist.\n", 1),
            fails("-d Stopped_one", "lpadmin: Bad device-uri scheme \"foo\".\n", 1),
            fails("-d Front-Desk_2", "", 3),
        ],
    );
    fake.tool(
        "cupsenable",
        vec![fails("*", "cupsenable: Operation failed: The printer or class does not exist.\n", 1)],
    );
    fake.tool("cancel", vec![fails("*", "cancel: cancel-job failed: Job #9 does not exist.\n", 1)]);
    fake.tool("lp", vec![fails("*", "lp: Error - unable to access \"x\" - No such file or directory\n", 1)]);
    let printers = handle(&fake);

    assert_eq!(printers.remove_printer("Office-Laser").wait(), Err(Error::NotAuthorized("Forbidden".into())));
    assert!(matches!(printers.remove_printer("old_inkjet").wait(), Err(Error::Unavailable(_))));
    assert!(matches!(printers.set_system_default("Office-Laser").wait(), Err(Error::NotFound(_))));
    assert_eq!(
        printers.set_system_default("Stopped_one").wait(),
        Err(Error::Command { program: "lpadmin".into(), message: "Bad device-uri scheme \"foo\".".into() })
    );
    assert_eq!(
        printers.set_system_default("Front-Desk_2").wait(),
        Err(Error::Command { program: "lpadmin".into(), message: "exited with status 3".into() })
    );
    assert!(matches!(printers.resume("Gone").wait(), Err(Error::NotFound(_))));
    assert!(matches!(printers.cancel_job("Office-Laser-9").wait(), Err(Error::NotFound(_))));
    let page = fake.dir.join("page.txt");
    std::fs::write(&page, "hello\n").expect("a file");
    assert!(matches!(printers.print_file("Office-Laser", &page, None).wait(), Err(Error::NotFound(_))));
    // cupsreject isn't there at all.
    assert!(matches!(printers.set_accepting("Office-Laser", false).wait(), Err(Error::Unavailable(_))));
}

#[test]
fn error_messages_are_sorted() {
    let sorted = |message: &str| cli::classify("lpadmin", message.to_owned());
    assert!(matches!(sorted("Forbidden"), Error::NotAuthorized(_)));
    assert!(matches!(sorted("client-error-not-authorized"), Error::NotAuthorized(_)));
    assert!(matches!(sorted("Unauthorized"), Error::NotAuthorized(_)));
    assert!(matches!(sorted("Unable to connect to server: No such file or directory"), Error::Unavailable(_)));
    assert!(matches!(sorted("Scheduler is not running."), Error::Unavailable(_)));
    assert!(matches!(sorted("No such file or directory"), Error::Unavailable(_)));
    assert!(matches!(sorted("Unknown destination \"x\"."), Error::NotFound(_)));
    assert!(matches!(sorted("Invalid destination name in list \"x\"."), Error::NotFound(_)));
    assert!(matches!(sorted("Unable to open PPD \"x\": No such file"), Error::NotFound(_)));
    assert!(matches!(sorted("Printer drivers are deprecated."), Error::Command { .. }));

    let output = cli::Output {
        stdout: Vec::new(),
        stderr: "lpadmin: Printer drivers are deprecated.\nlpadmin: Forbidden\n".into(),
        status: Some(1),
    };
    assert_eq!(
        cli::failure("lpadmin", &output),
        Error::NotAuthorized("Printer drivers are deprecated.\nForbidden".into())
    );
    let killed = cli::Output { status: None, ..cli::Output::default() };
    assert_eq!(
        cli::failure("lp", &killed),
        Error::Command { program: "lp".into(), message: "was killed by a signal".into() }
    );
}

#[test]
fn printing_the_test_page_and_files() {
    let fake = Fake::new();
    cups(&fake);
    let data = fake.dir.join("share/cups");
    std::fs::create_dir_all(data.join("data")).expect("a data directory");
    std::fs::write(data.join("data/testprint"), "#CUPS-BANNER\n").expect("a test page");
    fake.tool("cups-config", vec![ok("--datadir", format!("{}\n", data.display()))]);
    fake.tool(
        "lp",
        vec![ok("-d Office-Laser *", "request id is Office-Laser-46 (1 file(s))\n"), ok("-d Brother_HL_L2390DW *", "")],
    );
    let printers = handle(&fake);

    assert_eq!(printers.print_test_page("Office-Laser").wait(), Ok("Office-Laser-46".to_owned()));
    let page = data.join("data/testprint").display().to_string();
    assert_eq!(fake.calls("lp"), [args(&["-d", "Office-Laser", "-t", "Test page", &page])]);

    let file = fake.dir.join("Quarterly report.pdf");
    std::fs::write(&file, "%PDF-1.4\n").expect("a file");
    assert_eq!(printers.print_file("Office-Laser", &file, Some("Report")).wait(), Ok("Office-Laser-46".to_owned()));
    assert_eq!(printers.print_file("Office-Laser", &file, None).wait(), Ok("Office-Laser-46".to_owned()));
    let file_arg = file.display().to_string();
    assert_eq!(
        fake.calls("lp").get(1..),
        Some(&[args(&["-d", "Office-Laser", "-t", "Report", &file_arg]), args(&["-d", "Office-Laser", &file_arg])][..])
    );

    // lp said nothing about the job.
    assert!(matches!(printers.print_file("Brother_HL_L2390DW", &file, None).wait(), Err(Error::Failed(_))));
    // Refused before lp runs.
    assert!(matches!(printers.print_file("Office-Laser", fake.dir.join("nope"), None).wait(), Err(Error::NotFound(_))));
    assert!(matches!(printers.print_file("Office-Laser", &fake.dir, None).wait(), Err(Error::InvalidArgument(_))));
    assert_eq!(fake.calls("lp").len(), 4);
}

#[test]
fn finding_the_test_page() {
    let fake = Fake::new();
    // No cups-config: the page beside the lp binary, as in a store path.
    fake.tool("lp", vec![]);
    let data = fake.dir.join("share/cups/data");
    std::fs::create_dir_all(&data).expect("a data directory");
    std::fs::write(data.join("testprint"), "#CUPS-BANNER\n").expect("a test page");
    let tools = fake.tools();
    let found = tools.find_test_page().and_then(|page| page.canonicalize().ok());
    assert_eq!(found, data.join("testprint").canonicalize().ok());

    let given = fake.dir.join("mine");
    std::fs::write(&given, "x").expect("a page");
    let tools = Tools { test_page: Some(given.clone()), ..fake.tools() };
    assert_eq!(tools.find_test_page(), Some(given));
    let tools = Tools { test_page: Some(fake.dir.join("missing")), ..fake.tools() };
    assert_eq!(tools.find_test_page(), None);
}

#[test]
fn discovering_devices() {
    let fake = Fake::new();
    let lpinfo = "Device: uri = ipp\n        class = network\n        info = Internet Printing Protocol (ipp)\n        make-and-model = Unknown\n        device-id = \n        location = \n\
Device: uri = dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000\n        class = network\n        info = Brother HL-L2390DW\n        make-and-model = Brother HL-L2390DW\n        device-id = MFG:Brother;MDL:HL-L2390DW;CMD:PJL,HBP;\n        location = \n\
Device: uri = usb://Canon/iP4950%20series?serial=1234AB\n        class = direct\n        info = Canon iP4950 series\n        make-and-model = Canon iP4950 series\n        device-id = MFG:Canon;MDL:iP4950 series;CMD:BJL,BJRaster3;\n        location = \n\
Device: uri = dnssd://HP%20LaserJet%20M404._ipp._tcp.local/\n        class = network\n        info = HP LaserJet M404\n        make-and-model = HP LaserJet M404\n        device-id = MFG:HP;MDL:LaserJet M404;CMD:PCL,PWGRaster;\n        location = \n";
    let driverless = "\"driverless:ipp://Brother%20HL-L2390DW._ipp._tcp.local/\" en \"Brother\" \"Brother HL-L2390DW, driverless, cups-filters 2.0.1\" \"MFG:Brother;MDL:HL-L2390DW;CMD:PWGRaster,URF;\"\n\
\"driverless:ipps://Epson%20ET-2850._ipps._tcp.local/\" en \"Epson\" \"Epson ET-2850, driverless, cups-filters 2.0.1\" \"MFG:Epson;MDL:ET-2850;CMD:PWGRaster;\"\n";
    fake.tool("lpinfo", vec![ok("-l --timeout 30 -v", lpinfo)]);
    fake.tool("driverless", vec![ok("list", driverless)]);
    let printers = handle(&fake);

    let devices = printers.discover().wait().expect("devices");
    let summary: Vec<(&str, bool, bool)> =
        devices.iter().map(|d| (d.info.as_str(), d.driverless, d.is_manual())).collect();
    assert_eq!(
        summary,
        [
            ("Internet Printing Protocol (ipp)", false, true),
            ("Brother HL-L2390DW", true, false),
            ("Canon iP4950 series", false, false),
            ("HP LaserJet M404", true, false),
            ("Epson ET-2850", true, false),
        ]
    );
    assert_eq!(devices[2].class, DeviceClass::Direct);
    assert_eq!(devices[2].model_name(), "Canon iP4950 series");
    let bare = Device { make_and_model: None, ..devices[2].clone() };
    assert_eq!(bare.model_name(), "Canon iP4950 series", "from the device ID");
    let unknown = Device { make_and_model: None, device_id: None, ..devices[2].clone() };
    assert_eq!(unknown.model_name(), "Canon iP4950 series", "from the info");
    assert_eq!(devices[4].uri, "ipps://Epson%20ET-2850._ipps._tcp.local/");
    assert_eq!(fake.calls("lpinfo"), [args(&["-l", "--timeout", "30", "-v"])]);
    assert_eq!(fake.calls("driverless"), [args(&["list"])]);

    // lpinfo refused and no driverless: the refusal.
    let refused = Fake::new();
    refused.tool("lpinfo", vec![fails("*", "lpinfo: Forbidden\n", 1)]);
    assert!(matches!(cli::discover(&refused.tools()), Err(Error::NotAuthorized(_))));
    // No lpinfo, but driverless: what driverless found.
    let partial = Fake::new();
    partial.tool("driverless", vec![ok("list", driverless)]);
    let found = cli::discover(&partial.tools()).expect("devices");
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|d| d.driverless));
}

#[test]
fn listing_drivers_and_finished_jobs() {
    let fake = Fake::new();
    cups(&fake);
    let models = "Model:  name = drv:///sample.drv/zebra.ppd\n        natural_language = en\n        make-and-model = Zebra ZPL Label Printer\n        device-id = \n\
Model:  name = everywhere\n        natural_language = en\n        make-and-model = IPP Everywhere\n        device-id = \n";
    fake.tool(
        "lpinfo",
        vec![ok("-l --make-and-model Zebra -m", models), ok("-l --device-id MFG:Zebra;MDL:ZD420; -m", models)],
    );
    let printers = handle(&fake);

    let filter = DriverFilter { make_and_model: Some("Zebra".into()), ..DriverFilter::default() };
    let drivers = printers.drivers(filter).wait().expect("drivers");
    assert_eq!(drivers.len(), 2);
    assert_eq!(drivers[0].make_and_model, "Zebra ZPL Label Printer");
    assert!(drivers[1].is_everywhere());
    let filter = DriverFilter { device_id: Some("MFG:Zebra;MDL:ZD420;".into()), ..DriverFilter::default() };
    assert_eq!(printers.drivers(filter).wait().map(|d| d.len()), Ok(2));

    let finished = printers.completed_jobs().wait().expect("jobs");
    let states: Vec<(&str, JobState)> = finished.iter().map(|j| (j.id.as_str(), j.state)).collect();
    assert_eq!(
        states,
        [
            ("Office-Laser-40", JobState::Completed),
            ("Office-Laser-41", JobState::Canceled),
            ("old_inkjet-39", JobState::Aborted),
        ]
    );
    assert_eq!(finished[2].status.as_deref(), Some("The printer is not responding."));
}

#[test]
fn clearing_the_users_default() {
    let fake = Fake::new();
    cups(&fake);
    let file = fake.dir.join("home/.cups/lpoptions");
    std::fs::write(&file, "Dest Office-Laser media=A4\nDefault Brother_HL_L2390DW sides=two-sided-long-edge\n")
        .expect("an lpoptions file");
    let printers = handle(&fake);
    assert_eq!(printers.snapshot().user_default.as_deref(), Some("Brother_HL_L2390DW"));

    assert_eq!(printers.clear_user_default().wait(), Ok(()));
    assert_eq!(
        std::fs::read_to_string(&file).ok().as_deref(),
        Some("Dest Office-Laser media=A4\nDest Brother_HL_L2390DW sides=two-sided-long-edge\n")
    );
    assert_eq!(printers.snapshot().user_default, None);
    // Nothing to clear is fine, with or without a file.
    assert_eq!(printers.clear_user_default().wait(), Ok(()));
    std::fs::remove_file(&file).expect("removed");
    assert_eq!(printers.clear_user_default().wait(), Ok(()));
}

#[test]
fn commands_run_bounded_and_detached() {
    let fake = Fake::new();
    // Something it started in the background is killed with it.
    let pid_file = fake.dir.join("background.pid");
    fake.script(
        "slow",
        &format!("#!/bin/sh\nsleep 30 &\necho $! > {}\nsleep 30\n", quote(&pid_file.display().to_string())),
    );
    let started = Instant::now();
    let result = cli::run(&fake.bin().join("slow"), &["x"], Duration::from_millis(300), None);
    assert!(matches!(result, Err(Error::Timeout)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let background = std::fs::read_to_string(&pid_file).unwrap_or_default();
    let stat = format!("/proc/{}/stat", background.trim());
    wait_for(|| std::fs::read_to_string(&stat).map(|s| s.contains(") Z ")).unwrap_or(true));

    // A stop cancels it.
    let stop = Stop::default();
    let stopper = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stopper.stop();
    });
    let started = Instant::now();
    let result = cli::run(&fake.bin().join("slow"), &["x"], Duration::from_secs(30), Some(&stop));
    assert_eq!(result.map(|_| ()), Err(Error::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5));

    // Lots of output on both pipes doesn't block it.
    fake.script(
        "chatty",
        "#!/bin/sh\nhead -c 1000000 /dev/zero | tr '\\0' x\nhead -c 200000 /dev/zero | tr '\\0' y >&2\nexit 4\n",
    );
    let output = cli::run(&fake.bin().join("chatty"), &[] as &[&str], Duration::from_secs(10), None).expect("output");
    assert_eq!((output.stdout.len(), output.stderr.len(), output.status), (1_000_000, 200_000, Some(4)));

    // A program that isn't there.
    let missing = cli::run(&fake.bin().join("nothing"), &["x"], Duration::from_secs(1), None);
    assert!(matches!(missing, Err(Error::Unavailable(_))));
}

#[test]
fn printer_names() {
    assert_eq!(validate_name("Brother_HL_L2390DW"), Ok(()));
    assert_eq!(validate_name("Office-Laser.2:a"), Ok(()));
    assert_eq!(validate_name("Bürodrucker_藍"), Ok(()));
    assert_eq!(validate_name(&"x".repeat(127)), Ok(()));
    for bad in ["", "a b", "a\tb", "a/b", "a\\b", "a#b", "a?b", "a'b", "a\"b", "a@b", "-p", "a\u{7f}"] {
        assert!(matches!(validate_name(bad), Err(Error::InvalidArgument(_))), "{bad:?}");
    }
    assert!(validate_name(&"x".repeat(128)).is_err());
    assert_eq!(validate_name("a b"), Err(Error::InvalidArgument("A printer name can't contain spaces".into())));

    assert_eq!(suggest_name("Brother HL-L2390DW series"), "Brother_HL_L2390DW");
    assert_eq!(suggest_name("Brother HL-L2390DW series - IPP Everywhere"), "Brother_HL_L2390DW");
    assert_eq!(suggest_name("HP LaserJet Pro M404-M405, driverless, cups-filters 2.0.1"), "HP_LaserJet_Pro_M404_M405");
    assert_eq!(suggest_name("  Canon iP4950 (USB) "), "Canon_iP4950_USB");
    assert_eq!(suggest_name("Bürodrucker 藍"), "B_rodrucker");
    assert_eq!(suggest_name("藍 – 藍"), "Printer");
    assert_eq!(suggest_name(""), "Printer");
    assert_eq!(suggest_name(&"ab ".repeat(100)).len(), 127);
    for model in ["Brother HL-L2390DW", "x y", "!!!", &"long name ".repeat(40)] {
        assert_eq!(validate_name(&suggest_name(model)), Ok(()), "{model}");
    }

    let state = PrintersState {
        printers: ["Brother_HL_L2390DW", "brother_hl_l2390dw_2"]
            .map(|name| Printer { name: name.into(), ..Printer::default() })
            .to_vec(),
        ..PrintersState::default()
    };
    assert_eq!(state.suggest_name("Brother HL-L2390DW series"), "Brother_HL_L2390DW_3");
    assert_eq!(state.suggest_name("Epson ET-2850"), "Epson_ET_2850");
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

#[test]
#[ignore = "reads this machine's CUPS"]
fn live_state() {
    let printers = Printers::new(Wake::none());
    let refreshed = printers.refresh().wait();
    let state = printers.snapshot();
    eprintln!("refresh: {refreshed:?}");
    eprintln!("available: {}, error: {:?}", printers.available(), state.error);
    eprintln!("system default: {:?}, user default: {:?}", state.system_default, state.user_default);
    for printer in &state.printers {
        eprintln!("{printer:#?}");
    }
    for job in &state.jobs {
        eprintln!("{job:#?}");
    }
    assert!(state.loaded);
    if on_path("lpstat") {
        assert_eq!(refreshed, Ok(()));
        assert!(printers.available(), "lpstat is installed but CUPS isn't answering");
    }
    let finished = printers.completed_jobs().wait();
    eprintln!("completed jobs: {:?}", finished.as_ref().map(Vec::len));
    assert!(finished.is_ok() || !on_path("lpstat"));
}

#[test]
#[ignore = "reads this machine's CUPS, and the network through it"]
fn live_discover() {
    let printers = Printers::new(Wake::none());
    let started = Instant::now();
    let found = printers.discover().wait_timeout(Duration::from_secs(150));
    eprintln!("discovery took {:?}", started.elapsed());
    match found {
        Some(Ok(devices)) => {
            for device in &devices {
                eprintln!(
                    "{:?} {} — {} (model {:?}, driverless {}, manual {}) → {}",
                    device.class,
                    device.uri,
                    device.info,
                    device.model_name(),
                    device.driverless,
                    device.is_manual(),
                    printers.snapshot().suggest_name(&device.model_name()),
                );
            }
            assert!(!devices.is_empty(), "lpinfo lists its backends even without printers");
        }
        Some(Err(error)) => {
            eprintln!("discovery failed: {error}");
            assert!(!matches!(error, Error::Timeout), "discovery should give up by itself");
        }
        None => panic!("discovery hung"),
    }
}

#[test]
#[ignore = "reads this machine's CUPS"]
fn live_drivers() {
    let printers = Printers::new(Wake::none());
    let filter = DriverFilter { make_and_model: Some("Generic".into()), ..DriverFilter::default() };
    let drivers = printers.drivers(filter).wait();
    match &drivers {
        Ok(drivers) => {
            for driver in drivers.iter().take(20) {
                eprintln!("{} — {} ({:?})", driver.name, driver.make_and_model, driver.device_id);
            }
            eprintln!("{} drivers", drivers.len());
            assert!(drivers.iter().any(Driver::is_everywhere), "CUPS always offers IPP Everywhere");
        }
        Err(error) => {
            eprintln!("lpinfo -m failed: {error}");
            assert!(!on_path("lpinfo"));
        }
    }
}

#[test]
#[ignore = "reads this machine's CUPS installation"]
fn live_test_page() {
    let page = Tools::default().find_test_page();
    eprintln!("test page: {page:?}");
    if let Some(page) = page {
        assert!(page.is_file());
        let start = std::fs::read(&page).map(|bytes| bytes.get(..12).map(<[u8]>::to_vec)).ok().flatten();
        eprintln!("starts with {:?}", start.map(|s| String::from_utf8_lossy(&s).into_owned()));
    } else {
        assert!(!on_path("lp"), "lp is installed but its test page wasn't found");
    }
}
