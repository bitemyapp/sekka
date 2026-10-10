pub mod builder;
pub mod config;
pub mod gui_input;
pub mod live_test;
pub mod support;
pub mod vm;
pub mod wifi_test;

pub fn dispatch(command: &str, args: Vec<String>) -> anyhow::Result<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        let usage = match command {
            "build-rootless" => "BOOTSTRAP_ISO [--sha256 HEX] [--rebuild-iso]",
            "qemu-test" => {
                "ISO [--firmware bios|uefi] [--install] [--gui] [--disk-bus virtio|nvme|nvme4k] [--filesystem ext4|btrfs|xfs] [--previous-filesystem blank|ext4|btrfs|xfs] [--applications all|none|ID,ID]"
            }
            "live-test" => {
                "ISO [--firmware bios|uefi] [--profile default|compatibility|lts] [--input usb|ps2]"
            }
            "qmp-input" => "RUN-NAME screenshot|key|type|click [VALUES]",
            "guest-exec" => "RUN-NAME COMMAND",
            "wifi-test" => "ISO [--profile default|compatibility|lts]",
            "boot-installed" => "RUN-NAME",
            "check-config" => "template|settings FILE FILE",
            "check-selections" => "(inside the /workspace build environment)",
            _ => "(run only inside the disposable guest)",
        };
        println!("Usage: {command} {usage}");
        return Ok(());
    }
    support::init_signals()?;
    match command {
        "build-rootless" => builder::main(args),
        "prepare-builder" => builder::prepare(),
        "build-iso" => {
            anyhow::ensure!(
                args.is_empty() || args == ["--rebuild"],
                "Usage: build-iso [--rebuild]"
            );
            builder::build_iso(!args.is_empty())
        }
        "qemu-test" => vm::main(args),
        "live-test" => live_test::main(args),
        "wifi-test" => wifi_test::main(args),
        "guest-wifi-test" => wifi_test::guest(),
        "qmp-input" => gui_input::main(args),
        "guest-exec" => vm::guest_exec(args),
        "boot-installed" => vm::boot_installed(args),
        "check-config" => config::main(args),
        "check-selections" => {
            anyhow::ensure!(args.is_empty(), "Usage: check-selections");
            builder::check_selections()
        }
        _ => anyhow::bail!("Unknown command: {command}"),
    }
}
