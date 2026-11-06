// SPDX-License-Identifier: GPL-3.0-or-later
//! Every sentence the app shows, in one place, in Sekka's voice: plain,
//! sentence case, candid about what doesn't work yet, no exclamation marks.
use sekka_mac_contract::{
    Acknowledgement, DiskChanged, FileVault, HumanStep, InstallState, Issue, IssueCode, Refusal,
    Stage,
};

/// Bytes as Finder counts them: decimal units, one decimal place under 100.
pub fn bytes(n: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} bytes");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// Whole gigabytes, for the size field.
pub fn gigabytes(n: u64) -> u64 {
    (n + 500_000_000) / 1_000_000_000
}

/// "about 4 minutes", from a byte rate.
pub fn remaining(bytes_left: u64, bytes_per_second: f64) -> Option<String> {
    if bytes_per_second < 1.0 {
        return None;
    }
    let seconds = (bytes_left as f64 / bytes_per_second).round() as u64;
    Some(match seconds {
        0..=59 => "less than a minute left".into(),
        60..=119 => "about a minute left".into(),
        120..=3599 => format!("about {} minutes left", (seconds + 30) / 60),
        _ => format!("about {:.1} hours left", seconds as f64 / 3600.0),
    })
}

pub const APP_NAME: &str = "Sekka Installer";

/// The journey in the sidebar: the app's own steps, then what the owner does
/// after it.
pub const JOURNEY: [(&str, &str); 6] = [
    ("Check this Mac", "Model, macOS, power and disk"),
    ("Choose a size", "How much Sekka gets"),
    ("Download", "Sekka, checked byte for byte"),
    ("Install", "Partitions and the system"),
    ("Finish in Recovery", "On the startup screen"),
    ("Create your account", "When Sekka first starts"),
];

pub fn stage(stage: Stage) -> &'static str {
    stage.title()
}

/// What a stage does, for the installing screen.
pub fn stage_detail(stage: Stage) -> &'static str {
    match stage {
        Stage::Import => "Sekka Installer's helper is checking every downloaded byte against the release.",
        Stage::AppleInputs => "Apple's startup files for the new volume come from Apple and are checked against Apple's own signed list.",
        Stage::Shrink => "macOS is giving up the space for Sekka. Apple's tool does this; it can take several minutes.",
        Stage::StubPartition => "A small volume holds the startup software and Recovery for Sekka.",
        Stage::StubSystem => "The startup volume gets the parts of macOS it needs to start Sekka.",
        Stage::Partitions => "Sekka's boot partition and system partition are being created.",
        Stage::Firmware => "This Mac's own Wi-Fi, Bluetooth, keyboard and trackpad firmware is copied for Sekka.",
        Stage::Esp => "The boot files are written and read back.",
        Stage::Root => "Sekka is written to its partition and read back to check every byte.",
        Stage::Recovery => "The final step's app is placed on Sekka's startup volume.",
        Stage::StartupDisk => "Sekka's startup volume becomes the one your Mac starts from next.",
        Stage::Enrollment | Stage::StartupSelection => "This step happens in Recovery.",
    }
}

pub fn filevault(state: FileVault) -> &'static str {
    match state {
        FileVault::On => "FileVault is on. macOS stays encrypted; Sekka is installed beside it.",
        FileVault::Off => "FileVault is off.",
        FileVault::Converting => "FileVault is still encrypting or decrypting. Wait until it finishes, then check again.",
        FileVault::Unknown => "FileVault's state couldn't be read.",
    }
}

/// What to do about an issue, in a sentence.
pub fn issue(issue: &Issue) -> String {
    let advice = match issue.code {
        IssueCode::OnBattery => "Connect your charger and keep it connected until the installer finishes.",
        IssueCode::BatteryLow => "Charge to at least half before installing.",
        IssueCode::FileVaultConverting => "Wait for FileVault to finish, then check again.",
        IssueCode::NotEnoughSpace => "Free up space in macOS, then check again. Space in the Trash or in local snapshots doesn't count until it's gone.",
        IssueCode::LocalSnapshots => "Time Machine's local snapshots can stop macOS from shrinking. If resizing fails, delete them in Disk Utility and try again.",
        IssueCode::NotNormalBoot => "Start up from your Mac's internal disk, in macOS, and open Sekka Installer there.",
        IssueCode::MacosTooOld => "Update macOS in System Settings › General › Software Update.",
        IssueCode::ExistingSekka => "Sekka is already installed on this Mac.",
        IssueCode::PartialInstallation => "An earlier installation didn't finish. Its partitions are still on the disk.",
        IssueCode::UnrecognizedLayout | IssueCode::AmbiguousDisk => "Sekka Installer won't change a disk it can't describe with certainty.",
        IssueCode::ResizeLimitsUnavailable => "macOS couldn't say how far it can shrink. Restart your Mac and check again.",
        _ => "",
    };
    let detail = issue.detail.as_str();
    if advice.is_empty() || detail.contains(advice) {
        detail.to_string()
    } else {
        format!("{detail} {advice}")
    }
}

pub fn refusal(refusal: &Refusal) -> String {
    match refusal {
        Refusal::UnknownMachine { product_type, .. } => format!(
            "This Mac is a {product_type}. Sekka has only been brought up on the 14-inch MacBook Pro with an M3 Pro (Mac15,6) so far. Each Mac needs its own work on how it starts and its drivers."
        ),
        Refusal::ChipMismatch { .. } => "This Mac's chip isn't the M3 Pro that Sekka was brought up on.".into(),
        Refusal::UntestedSystemFirmware { found, tested } => format!(
            "This Mac's system firmware is {found}. Sekka's startup software has been tested with {} only. macOS updates change the system firmware; Sekka will support newer versions once they have been tested.",
            tested.join(", ")
        ),
        Refusal::MacosTooOld { minimum, found } => {
            format!("This Mac runs macOS {found}; Sekka Installer needs macOS {minimum} or later.")
        }
        Refusal::KnownIssue { summary, .. } => summary.clone(),
    }
}

pub fn acknowledgement(a: Acknowledgement) -> &'static str {
    match a {
        Acknowledgement::DevelopmentRelease => "I understand this is a development build of Sekka, not yet qualified on this Mac.",
        Acknowledgement::KnownMacosIssue => "I understand the known problem with this version of macOS described above.",
        Acknowledgement::UntestedSystemFirmware => "I understand this Mac's system firmware hasn't been tested with Sekka.",
    }
}

pub const BACKUP: &str = "I have a current backup of this Mac.";

pub fn human_step(step: HumanStep, volume: &str, owner: &str) -> String {
    match step {
        HumanStep::ShutDown => "Shut down your Mac. Wait ten seconds after the screen goes dark.".into(),
        HumanStep::HoldPowerButton => "Press and hold the power button until “Loading startup options” appears.".into(),
        HumanStep::ChooseVolume => format!("Choose {volume}, then click Continue."),
        HumanStep::AuthorizeInRecovery => {
            if owner.is_empty() {
                "Sign in, then follow Finish Installing Sekka.".into()
            } else {
                format!("Sign in as {owner}, then follow Finish Installing Sekka.")
            }
        }
        HumanStep::CreateAccount => "Sekka starts and asks you to create your account.".into(),
    }
}

pub fn disk_changed(changed: DiskChanged) -> &'static str {
    match changed {
        DiskChanged::No => "Nothing on your Mac's disk changed.",
        DiskChanged::Yes => "Your Mac's disk has changed: some of Sekka's partitions are there, but the installation isn't complete.",
        DiskChanged::Unknown => "It isn't known whether your Mac's disk changed. Sekka Installer won't try again on its own.",
    }
}

/// The headline and explanation for an installation that stopped.
pub fn stopped(state: &InstallState) -> (String, String) {
    match state {
        InstallState::Failed { stage, disk_changed: changed } => (
            format!("“{}” didn't finish", stage.title()),
            disk_changed(*changed).into(),
        ),
        InstallState::MutationStarted { stage } => (
            "The installation stopped part way".into(),
            format!(
                "It stopped during or after “{}”. Your Mac's disk has changed. Copy the diagnostics and ask for help; don't start another installation until this one is looked at.",
                stage.title()
            ),
        ),
        InstallState::StartupDiskFailed => (
            "Setting the startup disk didn't work".into(),
            "Everything else is in place and was checked. Try setting the startup disk again with your password; nothing else will be repeated.".into(),
        ),
        InstallState::EnrollmentUncertain => (
            "The Recovery step didn't finish".into(),
            "Start up in Recovery again (hold the power button, choose Sekka) and Finish Installing Sekka picks up where it stopped.".into(),
        ),
        other => ("The installation stopped".into(), format!("{other:?}")),
    }
}

/// Plain-English names for the bars on the disk picture.
pub const MACOS: &str = "macOS";
pub const SEKKA: &str = "Sekka";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_as_finder_shows_them() {
        assert_eq!(bytes(999), "999 bytes");
        assert_eq!(bytes(12_400_000_000), "12.4 GB");
        assert_eq!(bytes(494_384_795_648), "494 GB");
        assert_eq!(gigabytes(107_374_182_400), 107);
    }

    #[test]
    fn time_left_is_rounded_kindly() {
        assert_eq!(remaining(10, 100.0).unwrap(), "less than a minute left");
        assert_eq!(remaining(240_000_000, 1_000_000.0).unwrap(), "about 4 minutes left");
        assert!(remaining(10, 0.0).is_none());
    }
}
