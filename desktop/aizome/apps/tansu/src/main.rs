//! `aizome-tansu [IMAGE…]`: the window (offering to attach an image given
//! on the command line). `--sample` shows sample disks in memory instead
//! of this computer's, and changes nothing.

use std::path::PathBuf;
use std::rc::Rc;

use tansu::app::Tansu;
use tansu::instance;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-tansu [--sample] [--show=DEVICE] [IMAGE…]\n\n\
             Disks: partitions, filesystems, encryption, images and SMART, through UDisks2.\n\
             \x20 IMAGE    offer to attach a disk image (.iso, .img)\n\
             \x20 --show=DEVICE  show the disk holding DEVICE first (a /dev node or a UDisks object)\n\
             \x20 --sample show sample disks held in memory (nothing on this computer changes)"
        );
        return;
    }
    let sample = args.iter().any(|a| a == "--sample");
    args.retain(|a| a != "--sample");
    // `--show=DEVICE` (in one argument: the kit hands the program every
    // argument not starting with `-` as a file to open).
    let show = args.iter().find_map(|a| a.strip_prefix("--show=").map(str::to_owned));
    args.retain(|a| !a.starts_with("--show="));
    let paths: Vec<PathBuf> = args.iter().map(|a| absolute(a)).collect();
    let mut app = if sample { Tansu::new(Rc::new(tansu::mock::Mock::default())) } else { Tansu::live() };
    app = app.with_paths(paths.clone()).with_show(show);
    if !sample {
        let handed: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        match instance::claim(&handed) {
            instance::Claim::Handed => return,
            instance::Claim::First(l) => app.listener = Some(l),
            instance::Claim::Alone => {}
        }
    }
    aizome_kit::run(app);
}

fn absolute(a: &str) -> PathBuf {
    let p = PathBuf::from(a);
    if p.is_absolute() { p } else { std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p) }
}
