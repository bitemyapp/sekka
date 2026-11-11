//! Privileged radio only. Capture, discovery, pairing and media stay in the
//! unprivileged AirPlay app. Owly journals every network change for recovery.
mod diagnostics;
mod observe;
mod profiles;
use owly_radio::injection::Injection;
use owly_radio::radio::{Backend, LinkState};
use owly_radio::rundir::RunDir;
use owly_radio::sys::Ops;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::time::Duration;

const RUN: &str = "/run/sekka-airplay-radio";

fn emit(value: Value) {
    // A closed UI pipe must not skip radio teardown.
    let _ = writeln!(std::io::stdout().lock(), "{value}");
}

fn seconds(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|v| (1..=14_400).contains(v))
        .ok_or_else(|| "Session duration must be 1–14400 seconds".into())
}

#[tokio::main(worker_threads = 2)]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            emit(json!({"state":"error", "error":e}));
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let run = RunDir::new(RUN);
    let observe = args.first().is_some_and(|a| a == "observe");
    if args == ["profiles"] {
        emit(json!({"profiles":profiles::NAMES}));
        return Ok(());
    }
    let profile = if args.first().is_some_and(|a| a == "trial") {
        Some(profiles::get(
            args.get(2).ok_or("A trial profile is required")?,
        )?)
    } else {
        None
    };
    let monitor_only = profile.map_or_else(
        || args.first().is_some_and(|a| a == "monitor"),
        |p| p.monitor,
    );
    if args == ["probe"] {
        emit(serde_json::to_value(owly_radio::probe::from_config(run)).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let (duration, channel) = match args.as_slice() {
        [command, value, _] if command == "trial" => {
            let duration = seconds(value)?;
            if duration > 600 { return Err("Radio trials are limited to 600 seconds".into()); }
            (Some(duration), profile.map(|p| p.channel))
        }
        [command, value] if matches!(command.as_str(), "session" | "observe" | "monitor") => (Some(seconds(value)?), None),
        [command, value, channel]
            if matches!(command.as_str(), "session" | "monitor") && ["6", "44", "149"].contains(&channel.as_str()) =>
        {
            (Some(seconds(value)?), channel.parse::<u8>().ok())
        }
        [command] if command == "recover" => (None, None),
        _ => return Err("Usage: sekka-airplay-radio probe | session SECONDS [6|44|149] | observe SECONDS | trial SECONDS PROFILE | profiles | recover".into()),
    };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return Err("The Wi-Fi radio helper requires administrator authentication.".into());
    }
    let mut config =
        owly_radio::config::Config::load(std::path::Path::new(owly_radio::config::CONFIG))?;
    if let Some(channel) = channel {
        // The monitor diagnostic only uses the cell to prepare the radio.
        // Intel rejects a 5-GHz cell; remove a channel-6 cell before tuning
        // the actual monitor, whose channel remains regulated by the kernel.
        config.channels = Some(vec![if monitor_only { 6 } else { channel }]);
    }
    if let Some(p) = profile {
        config.tx_rate = Some(p.rate.into());
        config.tx_amsdu = p.amsdu;
        config.aggregation = Some(false); // no Block Ack promise on passive monitors
    }
    if monitor_only && config.tx_rate.is_none() {
        // Limit this to the diagnostic monitor path. Owly uses HT only when
        // the peer advertises the selected MCS, retaining basic OFDM for
        // action/multicast frames and peers without that capability.
        config.tx_rate = Some("mcs7".into());
    }
    let settings = config.settings(run)?;
    if option_env!("SEKKA_AIRPLAY_GO_TRIAL") == Some("1") && (monitor_only || observe) {
        return Err("The GO trial requires the ibss6-mcs7 profile".into());
    }
    if observe && duration.is_some_and(|v| v > 60) {
        return Err("Receive-only diagnosis is limited to 60 seconds".into());
    }
    let start: owly_radio::injection::StartAwdl = std::sync::Arc::new(
        move |monitor, tap, mut cfg, peers| {
            if monitor_only {
                let ops = owly_radio::sys::System;
                // iwlmld diagnosis: remove the cell that suppresses remote
                // AWDL reception. Its journal entry still owns recovery.
                // This mode cannot promise hardware ACKs or media throughput.
                if !ops.exists(owly_radio::injection::IBSS) {
                    return Err(std::io::Error::other(
                        "Monitor-only diagnosis requires a separate ad-hoc interface",
                    ));
                }
                if channel.is_some_and(|c| c != 6) {
                    // Learn the AP country from normal beacons, without
                    // overriding the country's transmit restrictions.
                    refresh_regulatory_scan()?;
                }
                ops.del_iface(owly_radio::injection::IBSS)?;
                if let Some(channel) = channel {
                    cfg.radio.channel.0 = channel;
                }
                let frequency = owly_radio::probe::social_freq(cfg.radio.channel.0)
                    .ok_or_else(|| std::io::Error::other("Invalid AWDL channel"))?;
                // cfg80211 requires at least one running monitor here.
                ops.set_freq(monitor, frequency).map_err(|e| {
                    std::io::Error::other(format!("Tuning the monitor to {frequency} MHz: {e}"))
                })?;
                let permitted = ops
                    .phy(monitor)
                    .and_then(|phy| ops.wiphy(&phy))
                    .and_then(|w| w.channel(frequency).map(|c| c.may_transmit()))
                    .unwrap_or(false);
                emit(
                    json!({"state":"radio_mode", "mode":"monitor_without_hardware_ack", "channel":cfg.radio.channel.0, "transmitAllowed":permitted, "dataRate":cfg.rate.to_string()}),
                );
                if !permitted {
                    return Err(std::io::Error::other(format!(
                        "This Wi-Fi radio currently permits receiving but not transmitting on channel {}. The channel was not enabled.",
                        cfg.radio.channel.0
                    )));
                }
                emit(
                    json!({"state":"radio_preflight", "message":"Checking the monitor before AWDL transmission", "channel":cfg.radio.channel.0}),
                );
                std::thread::sleep(Duration::from_secs(3));
                if !ops.exists(monitor) {
                    return Err(std::io::Error::other(
                        "The Wi-Fi driver lost its monitor before any AWDL frames were transmitted.",
                    ));
                }
            }
            // AirDrop's -65 dBm proximity filter can hide a reachable TV. The
            // picker still requires explicit selection before any pairing.
            cfg.rssi_threshold = None;
            if let Some(p) = profile {
                cfg.no_ack = p.no_ack;
                cfg.neighbour_echo = p.echo;
                cfg.rssi_threshold = p.nearby_only.then_some(-65);
                emit(json!({"state":"trial_settings", "monitor":p.monitor,
                    "channel":p.channel, "rate":p.rate, "noAck":p.no_ack,
                    "amsdu":p.amsdu, "rssiThreshold":cfg.rssi_threshold,
                    "neighbourEcho":p.echo, "mtu":p.mtu}));
            }
            let native = owly_radio::native::Native::start(monitor, tap, cfg, peers)?;
            if let Some(p) = profile {
                owly_radio::rtnl::Rtnl::open()?.set_mtu(native.index, p.mtu)?;
            }
            Ok(Box::new(native))
        },
    );
    let mut backend = Injection::with_awdl(
        std::sync::Arc::new(owly_radio::sys::System),
        settings.clone(),
        start,
    );
    backend.recover().await.map_err(|e| e.to_string())?;
    let Some(duration) = duration else {
        emit(json!({"state":"idle"}));
        return Ok(());
    };
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let mut int = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| e.to_string())?;
    let (closed, mut ui_closed) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut byte = [0];
        // Any byte, EOF, or read error releases the radio. No root command
        // interpreter or caller-selected paths pass through this pipe.
        let _ = std::io::stdin().read(&mut byte);
        let _ = closed.send(());
    });
    let stop = async {
        tokio::select! { _ = &mut ui_closed => {}, _ = term.recv() => {}, _ = int.recv() => {} }
    };
    if observe {
        drop(backend);
        observe::scan(settings, duration, stop).await
    } else {
        session(&mut backend, duration, stop).await
    }
}

fn refresh_regulatory_scan() -> std::io::Result<()> {
    let iw = option_env!("SEKKA_AIRPLAY_IW")
        .ok_or_else(|| std::io::Error::other("The packaged Wi-Fi scan tool is unavailable"))?;
    emit(
        json!({"state":"regulatory_scan", "message":"Reading nearby access points' regulatory information"}),
    );
    let mut scan = std::process::Command::new(iw)
        .args(["dev", owly_radio::injection::IBSS, "scan", "passive"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = scan.try_wait()? {
            emit(
                json!({"state":"regulatory_scan", "finished":true, "success":status.success(), "exitCode":status.code()}),
            );
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            let _ = scan.kill();
            let _ = scan.wait();
            emit(
                json!({"state":"regulatory_scan", "finished":true, "success":false, "error":"Wi-Fi scan timed out"}),
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

async fn session(
    backend: &mut impl Backend,
    duration: u64,
    stop: impl std::future::Future<Output = ()>,
) -> Result<(), String> {
    tokio::pin!(stop);
    emit(json!({"state":"starting"}));
    // Don't cancel up halfway through a journaled change. Stop requests
    // remain pending and are handled as soon as setup completes.
    let outcome = match backend.up().await {
        Ok(mut link) => {
            let mut monitor = diagnostics::Monitor::open(link.mac.0).ok();
            let mut sample = tokio::time::interval(Duration::from_secs(2));
            emit(
                json!({"state":"up", "interface":link.name, "index":link.index,
                "address":format!("{}%{}", link.addr, link.name), "mac":link.mac.to_string()}),
            );
            let deadline = tokio::time::sleep(Duration::from_secs(duration));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = sample.tick() => if let Some(monitor) = &mut monitor { emit(monitor.sample()); },
                    _ = &mut stop => break Ok(()),
                    _ = &mut deadline => break Ok(()),
                    changed = link.state.changed() => {
                        if changed.is_err() { break Err("Radio status connection closed".into()); }
                        if let LinkState::Lost(reason) = &*link.state.borrow() { break Err(reason.clone()); }
                    }
                    changed = link.peers.changed() => {
                        if changed.is_err() { break Err("Radio peer connection closed".into()); }
                        let peers: Vec<_> = link.peers.borrow().iter().map(|p|
                            json!({"mac":p.mac.to_string(), "name":p.name, "address":format!("{}%{}",p.mac.link_local(),link.name), "rssi":p.rssi})).collect();
                        emit(json!({"state":"peers", "peers":peers}));
                    }
                }
            }
        }
        Err(e) => Err(e.to_string()),
    };
    emit(json!({"state":"stopping"}));
    backend
        .down()
        .await
        .map_err(|e| format!("Wi-Fi recovery failed: {e}"))?;
    emit(json!({"state":"idle"}));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_are_bounded() {
        for v in ["0", "14401", "-1", "never"] {
            assert!(seconds(v).is_err());
        }
        assert_eq!(seconds("60").unwrap(), 60);
    }
    #[tokio::test]
    async fn closed_client_restores_the_radio() {
        let root = tempfile::tempdir().unwrap();
        let mut backend = owly_radio::fake::Fake::new(root.path());
        let _ = session(&mut backend, 60, async {}).await;
        assert!(!root.path().join("sys/class/net/awdl0").exists());
        assert!(
            std::fs::read_to_string(root.path().join("owly-fake/calls"))
                .unwrap()
                .ends_with("down\n")
        );
    }
    #[tokio::test]
    async fn failed_setup_still_runs_recovery() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("owly-fake")).unwrap();
        std::fs::write(root.path().join("owly-fake/up.fail"), "4 injected failure").unwrap();
        let mut backend = owly_radio::fake::Fake::new(root.path());
        assert!(
            session(&mut backend, 60, std::future::pending())
                .await
                .unwrap_err()
                .contains("injected failure")
        );
        assert!(
            std::fs::read_to_string(root.path().join("owly-fake/calls"))
                .unwrap()
                .ends_with("down\n")
        );
    }
}
