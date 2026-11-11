mod app;
mod backend;
mod battery;
mod bluetooth;
mod peer;
mod tracking;

fn main() {
    if std::env::args().any(|v| v == "--help" || v == "-h") {
        println!(
            "sekka-airplay — mirror a screen and laptop audio to an AirPlay TV.\nSelect a TV, enter its passcode, then choose a screen in the desktop's sharing dialog.\nClosing the window stops sharing and forgets pairing credentials.\n\nOptions:\n  --discovery-details       Open raw discovery details\n  --discover-bluetooth      Print Bluetooth discovery for 20 seconds\n  --discover-peer           Test direct Wi-Fi for 60 seconds\n  --discover-peer-149       Test direct Wi-Fi on channel 149\n  --discover-peer-monitor   Test monitor-only direct Wi-Fi for 60 seconds\n  --monitor-mode            Use monitor-only Wi-Fi in the GUI\n  --monitor-149             Use channel 149 in the monitor GUI\n  --peer-trial              Limit GUI radio sessions to two minutes\n  --diagnose-peer-airdrop   Test the radio with an unlocked Everyone-mode iPhone\n\nRadio tests require administrator authentication and may interrupt Wi-Fi.\nThe AirDrop diagnostic only wakes peers; it never offers files."
        );
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--battery-relay") {
        let result = args
            .get(2)
            .filter(|_| args.len() == 3)
            .ok_or_else(|| std::io::Error::other("A test relay path is required"))
            .and_then(|path| battery::relay(std::path::Path::new(path)));
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    if std::env::args().any(|v| {
        matches!(
            v.as_str(),
            "--discover-peer" | "--discover-peer-149" | "--diagnose-peer-airdrop" | "--discover-peer-monitor"
        )
    }) {
        let (send, done) = std::sync::mpsc::channel();
        let channel = std::env::args().any(|v| v == "--discover-peer-149").then_some(149);
        let report = move |event| match event {
            peer::Event::Status(report) | peer::Event::Probe(report) => println!("{report}"),
            peer::Event::Devices(devices) => println!("{}", serde_json::json!({"receivers":devices})),
            peer::Event::Stopped => {
                let _ = send.send(());
            }
        };
        let _session = if std::env::args().any(|v| v == "--diagnose-peer-airdrop") {
            peer::Session::diagnose_phone(report)
        } else if std::env::args().any(|v| v == "--discover-peer-monitor") {
            peer::Session::diagnose_monitor(report)
        } else {
            peer::Session::for_channel(60, channel, report)
        };
        let _ = done.recv();
        return;
    }
    if std::env::args().any(|v| v == "--discover-bluetooth") {
        let _scan = bluetooth::Discovery::start(|snapshot| {
            println!("{}", serde_json::to_string(&snapshot).unwrap_or_default());
        });
        std::thread::sleep(std::time::Duration::from_secs(20));
        return;
    }
    aizome_kit::run(app::Kagami::new().with_discovery_details(std::env::args().any(|a| a == "--discovery-details")));
}
