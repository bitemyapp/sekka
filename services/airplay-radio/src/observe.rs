//! Receive-only, bounded channel diagnosis. Uses the same recovery journal
//! as the active radio, but never starts AWDL or injects a packet.
use owly_radio::injection::{MONITOR, Settings};
use owly_radio::journal::{Journal, Step};
use owly_radio::nl80211::Kind;
use owly_radio::sys::{Ops, System};
use serde_json::json;
use std::time::Duration;

pub async fn scan(
    settings: Settings,
    duration: u64,
    stop: impl std::future::Future<Output = ()>,
) -> Result<(), String> {
    let ops = System;
    let mut journal = Journal::open(&settings.run_dir).map_err(|e| e.to_string())?;
    journal.undo(&ops)?;
    let station = owly_radio::probe::find_station(&ops, &settings).map_err(|e| e.say)?;
    if ops.exists(MONITOR) {
        return Err("Another monitor interface already exists; leave its session alone.".into());
    }
    let mac = ops.mac(&station.name).ok_or("No station address")?;
    tokio::pin!(stop);
    let outcome: Result<(), String> = async {
        crate::emit(json!({"state":"starting", "mode":"receive_only"}));
        journal
            .record(Step::Station {
                station: station.name.clone(),
                connection: ops.nm_connection(&station.name),
            })
            .map_err(|e| e.to_string())?;
        ops.set_managed(&station.name, false);
        ops.set_up(&station.name, false)
            .map_err(|e| e.to_string())?;
        journal
            .record(Step::Interface {
                name: MONITOR.into(),
            })
            .map_err(|e| e.to_string())?;
        ops.add_iface(&station.phy, MONITOR, Kind::Monitor { active: false })
            .map_err(|e| e.to_string())?;
        ops.set_managed(MONITOR, false);
        ops.set_up(MONITOR, true).map_err(|e| e.to_string())?;
        for (channel, frequency) in owly_radio::probe::SOCIAL {
            if let Err(error) = ops.set_freq(MONITOR, frequency) {
                crate::emit(
                    json!({"state":"observe_error", "channel":channel, "error":error.to_string()}),
                );
                continue;
            }
            let mut monitor =
                crate::diagnostics::Monitor::open(mac.0).map_err(|e| e.to_string())?;
            crate::emit(json!({"state":"observing", "channel":channel, "frequency":frequency}));
            let deadline = tokio::time::sleep(Duration::from_millis(duration * 1000 / 3));
            tokio::pin!(deadline);
            let mut sample = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stop => return Ok(()),
                    _ = &mut deadline => break,
                    _ = sample.tick() => {
                        let mut report = monitor.sample();
                        report["channel"] = json!(channel);
                        crate::emit(report);
                    }
                }
            }
            let mut report = monitor.sample();
            report["channel"] = json!(channel);
            report["state"] = json!("observed_channel");
            crate::emit(report);
        }
        Ok(())
    }
    .await;
    crate::emit(json!({"state":"stopping"}));
    journal.undo(&ops)?;
    crate::emit(json!({"state":"idle"}));
    outcome
}
