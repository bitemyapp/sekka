//! Read-only checks against this machine's real daemons. They change
//! nothing; run them with `cargo test -p aizome-sys --test live -- --ignored
//! --nocapture` on a machine that has the services.

mod common;

use aizome_sys::Wake;
use common::eventually;

#[test]
#[ignore = "reads the real system bus"]
fn network_reads_networkmanager() {
    let network = aizome_sys::network::Network::new(Wake::none());
    assert!(eventually(|| network.available()), "NetworkManager is not running");
    let state = network.snapshot();
    println!("NetworkManager {} state {:?} connectivity {:?}", state.version, state.state, state.connectivity);
    for device in &state.devices {
        println!("  device {} {:?} {:?} {}", device.interface, device.kind, device.state, device.hw_address);
    }
    for network in &state.networks {
        println!(
            "  network {:?} {}% {:?} {:?} known={} connected={}",
            network.ssid, network.strength, network.security, network.bands, network.known, network.connected
        );
    }
    for connection in &state.connections {
        println!(
            "  saved {:?} {:?} active={} autoconnect={}",
            connection.id, connection.kind, connection.active, connection.autoconnect
        );
    }
    if let Some(details) = state.primary_details() {
        println!("  primary: {details:?}");
        assert!(!details.interface.is_empty());
    }
    println!("  radios: {:?} airplane={}", state.radios, state.airplane_mode);
    assert!(!state.devices.is_empty());
}

#[test]
#[ignore = "reads the real system bus"]
fn bluetooth_reads_bluez() {
    let bt = aizome_sys::bluetooth::Bluetooth::new(Wake::none());
    assert!(eventually(|| bt.available()), "BlueZ is not running");
    let state = bt.snapshot();
    for adapter in &state.adapters {
        println!(
            "adapter {} {} powered={} ({}) discoverable={} discovering={}",
            adapter.path,
            adapter.alias,
            adapter.powered,
            adapter.power_state,
            adapter.discoverable,
            adapter.discovering
        );
    }
    for device in &state.devices {
        println!(
            "  device {} {:?} {:?} paired={} connected={} battery={:?}",
            device.address, device.name, device.kind, device.paired, device.connected, device.battery
        );
    }
    assert!(!state.adapters.is_empty());
}

#[test]
#[ignore = "reads the real sound server"]
fn audio_reads_the_sound_server() {
    let audio = aizome_sys::audio::Audio::new(Wake::none());
    assert!(eventually(|| audio.available() && !audio.snapshot().sinks.is_empty()), "no sound server");
    let state = audio.snapshot();
    println!("server: {}", state.server);
    for sink in &state.sinks {
        println!(
            "  output {} {:?} {:.0}% muted={} default={} icon={} ports={:?}",
            sink.index,
            sink.description,
            sink.volume * 100.0,
            sink.muted,
            sink.is_default,
            sink.icon_name,
            sink.ports.iter().map(|p| (&p.name, p.available)).collect::<Vec<_>>()
        );
    }
    for source in &state.sources {
        println!(
            "  input {} {:?} {:.0}% muted={} default={}",
            source.index,
            source.description,
            source.volume * 100.0,
            source.muted,
            source.is_default
        );
    }
    for stream in state.playback.iter().chain(&state.recording) {
        println!(
            "  stream {} {:?} ({}) {:.0}% on {}",
            stream.index,
            stream.application,
            stream.app_id,
            stream.volume * 100.0,
            stream.device
        );
    }
    for card in &state.cards {
        println!(
            "  card {} {:?} active {:?} profiles {:?}",
            card.index,
            card.description,
            card.active_profile,
            card.profiles.iter().filter(|p| p.available).map(|p| &p.name).collect::<Vec<_>>()
        );
    }
    assert!(state.default_output().is_some());
}

#[test]
#[ignore = "reads the real system bus"]
fn power_reads_upower_profiles_logind_and_backlight() {
    use aizome_sys::power::*;
    let upower = Upower::new(Wake::none());
    assert!(eventually(|| upower.available()), "UPower is not running");
    let state = upower.snapshot();
    println!("on battery {} lid present {} closed {}", state.on_battery, state.lid_present, state.lid_closed);
    if let Some(display) = &state.display {
        println!("  display: {:.0}% {:?} {}", display.percentage, display.state, display.icon_name);
    }
    for device in &state.devices {
        println!(
            "  {:?} {} {:.0}% {:?} rate={:.1}W health={:.0}% online={} empty={:?} full={:?}",
            device.kind,
            device.native_path,
            device.percentage,
            device.state,
            device.energy_rate,
            device.capacity,
            device.online,
            device.time_to_empty,
            device.time_to_full
        );
    }
    println!("  keyboard lights: {:?}", state.keyboard_backlights);

    let profiles = PowerProfiles::new(Wake::none());
    if eventually(|| profiles.available()) {
        let p = profiles.snapshot();
        println!(
            "profile {} of {:?}, holds {:?}",
            p.active,
            p.profiles.iter().map(|p| &p.name).collect::<Vec<_>>(),
            p.holds
        );
        assert!(p.has(&p.active));
    }

    let logind = Logind::new(Wake::none());
    assert!(eventually(|| logind.available()));
    let l = logind.snapshot();
    println!(
        "logind session {} ({}) active={} suspend={:?} hibernate={:?} power off={:?} lid={} idle={} delay={}",
        l.session_id,
        l.session_path,
        l.session_active,
        l.can_suspend,
        l.can_hibernate,
        l.can_power_off,
        l.lid_closed,
        l.idle_hint,
        l.delay_inhibited
    );
    for inhibitor in logind.inhibitors().wait().unwrap() {
        println!("  inhibitor {} by {} ({}) {}", inhibitor.what, inhibitor.who, inhibitor.why, inhibitor.mode);
    }

    let backlight = Backlight::new(Wake::none());
    println!(
        "backlights {:?} primary {:?}",
        backlight.snapshot().devices,
        backlight.snapshot().primary().map(|d| d.fraction())
    );
}

#[test]
#[ignore = "reads the real system bus"]
fn datetime_reads_timedated_localed_hostnamed() {
    use aizome_sys::datetime::*;
    let time = TimeDate::new(Wake::none());
    assert!(eventually(|| time.available()));
    println!("time: {:?}", time.snapshot());
    let locale = Locale::new(Wake::none());
    assert!(eventually(|| locale.available()));
    println!("locale: {:?}", locale.snapshot());
    let host = Hostname::new(Wake::none());
    assert!(eventually(|| host.available()));
    println!("host: {:?}", host.snapshot());
    let zones = zones().unwrap();
    println!("{} zones; {} regions", zones.len(), by_region(&zones).len());
    assert!(!time.snapshot().timezone.is_empty());
}

#[test]
#[ignore = "reads the real system bus"]
fn users_reads_accountsservice() {
    let users = aizome_sys::users::Users::new(Wake::none());
    assert!(eventually(|| users.available() && !users.snapshot().users.is_empty()), "AccountsService is not running");
    let state = users.snapshot();
    for user in &state.users {
        println!(
            "user {} {:?} {:?} {:?} icon={:?} last login {:?} logins {}",
            user.uid, user.name, user.real_name, user.account_type, user.icon_file, user.last_login, user.login_count
        );
    }
    println!("current {:?}, managed {}", state.current().map(|u| &u.name), state.users_managed);
}

#[test]
#[ignore = "looks at the real system"]
fn polkit_helper_is_found() {
    // Registering would take over this session's authentication dialogs,
    // so only the helper lookup is checked live.
    let helper = aizome_sys::polkit::Helper::find();
    println!("polkit helper: {helper:?}");
    assert!(helper.is_some());
}

#[test]
#[ignore = "reads the real system bus"]
fn disks_reads_udisks() {
    let disks = aizome_sys::disks::Disks::new(Wake::none());
    assert!(eventually(|| disks.available() && !disks.snapshot().drives.is_empty()), "UDisks is not running");
    let state = disks.snapshot();
    println!("UDisks {} can make {:?}", state.version, state.supported_filesystems);
    for drive in &state.drives {
        println!(
            "drive {} {} serial {} {:?} smart {:?}",
            drive.display_name(),
            aizome_sys::disks::format_size(drive.size),
            drive.serial,
            drive.rotation,
            drive.smart.as_ref().map(|s| (s.health(), s.temperature_celsius, s.power_on_hours))
        );
        if let Some(disk) = state.drive_block(&drive.path) {
            for part in state.partitions(&disk.path) {
                let p = part.partition.as_ref().unwrap();
                println!(
                    "  {} #{} {:?} {:?} {} {} {:?}",
                    part.device,
                    p.number,
                    p.type_name,
                    p.flag_names,
                    aizome_sys::disks::format_size(p.size),
                    part.content(),
                    part.filesystem.as_ref().map(|f| (&f.mount_points, f.usage.map(|u| u.used_fraction())))
                );
            }
            println!("  free: {:?}", state.free_space(&disk.path));
        }
    }
    println!("volumes for the desktop: {:?}", state.volumes());
}
