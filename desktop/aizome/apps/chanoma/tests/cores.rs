//! Chanoma's frontend with real libretro cores (`CHANOMA_TEST_CORES`: a
//! folder of `*_libretro.so`, such as nixpkgs' `libretro.*` gathered):
//! each of the systems' cores loads and says what it plays, and the Game
//! Boy cores run a game made here, whose screen goes from white to black
//! while a direction is held, through a whole session: the game read, its
//! frames converted, the controller passed in, its state kept and carried
//! on from.
//!
//! `cargo test -p aizome-chanoma --test cores -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use aizome_chanoma::library::SYSTEMS;
use aizome_chanoma::pad::{Button, Shared};
use aizome_chanoma::retro::{CoreSource, Event, Launch, Session};

fn cores() -> Option<PathBuf> {
    std::env::var_os("CHANOMA_TEST_CORES").map(PathBuf::from)
}

/// A 32 KiB Game Boy game: the background palette white, or black while
/// any direction is held.
fn game_boy_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    // Entry: NOP; JP $0150.
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xc3, 0x50, 0x01]);
    // The logo the boot ROM compares (emulators that skip it may still look).
    const LOGO: [u8; 48] = [
        0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d, 0x00, 0x08,
        0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e, 0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99, 0xbb, 0xbb, 0x67, 0x63,
        0x6e, 0x0e, 0xec, 0xcc, 0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e,
    ];
    rom[0x104..0x134].copy_from_slice(&LOGO);
    rom[0x134..0x134 + 12].copy_from_slice(b"CHANOMA TEST");
    // ROM only, 32 KiB, no RAM.
    rom[0x147] = 0x00;
    rom[0x148] = 0x00;
    rom[0x149] = 0x00;
    let mut x: u8 = 0;
    for b in &rom[0x134..=0x14c] {
        x = x.wrapping_sub(*b).wrapping_sub(1);
    }
    rom[0x14d] = x;
    #[rustfmt::skip]
    let code = [
        0xf3,             // 0150 DI
        0x3e, 0xe4,       // 0151 LD A,$E4
        0xe0, 0x47,       // 0153 LDH (BGP),A
        0x3e, 0x20,       // 0155 loop: LD A,$20 (the directions)
        0xe0, 0x00,       // 0157 LDH (P1),A
        0xf0, 0x00,       // 0159 LDH A,(P1)
        0xf0, 0x00,       // 015B LDH A,(P1)
        0x2f,             // 015D CPL
        0xe6, 0x0f,       // 015E AND $0F
        0x28, 0x06,       // 0160 JR Z,no
        0x3e, 0x1b,       // 0162 LD A,$1B (colour 0 black)
        0xe0, 0x47,       // 0164 LDH (BGP),A
        0x18, 0xed,       // 0166 JR loop
        0x3e, 0xe4,       // 0168 no: LD A,$E4 (colour 0 white)
        0xe0, 0x47,       // 016A LDH (BGP),A
        0x18, 0xe7,       // 016C JR loop
    ];
    rom[0x150..0x150 + code.len()].copy_from_slice(&code);
    rom
}

/// The mean brightness (0–255) of a PNG.
fn brightness(png: &Path) -> f64 {
    let img = image::open(png).unwrap_or_else(|e| panic!("{}: {e}", png.display())).to_luma8();
    img.pixels().map(|p| f64::from(p.0[0])).sum::<f64>() / f64::from(img.width() * img.height())
}

/// Play `rom` with `core` for a moment with `held` pressed, keep where it
/// was left, and say how bright its last frame was.
fn play(core: &Path, rom: &Path, dir: &Path, resume: bool, held: u32) -> (f64, bool) {
    let pads = Arc::new(Shared::default());
    pads.set(0, true, held, [0.0; 6]);
    let launch = Launch {
        core: CoreSource::Library(core.to_owned()),
        game: rom.to_owned(),
        save_ram: dir.join("game.srm"),
        state: dir.join("game.state"),
        picture: dir.join("game.png"),
        resume,
        system_dir: dir.join("system"),
        bios: Vec::new(),
        found_bios: Vec::new(),
        save_dir: dir.join("saves"),
        players: 1,
        prefer: Vec::new(),
        screen_lines: 1080,
        silent: true,
        unpack_dir: dir.join("unzipped"),
        extensions: &[],
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::start(launch, None, pads, None, move |e| {
        let _ = tx.send(e);
    });
    let resumed = match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Event::Started { resumed, .. }) => resumed,
        other => panic!("{}: {other:?}", core.display()),
    };
    // Long enough for a core that plays the console's own start-up
    // first (SameBoy's logo scrolls down for about two seconds).
    std::thread::sleep(Duration::from_millis(3500));
    session.stop(true);
    (brightness(&dir.join("game.png")), resumed)
}

#[test]
#[ignore = "needs CHANOMA_TEST_CORES"]
fn every_systems_cores_load_and_say_what_they_play() {
    let Some(dir) = cores() else { return };
    let mut found = 0;
    for sys in SYSTEMS {
        for stem in sys.cores {
            let path = dir.join(format!("{stem}_libretro.so"));
            if !path.is_file() {
                continue;
            }
            // SAFETY: the folder holds libretro cores.
            let lib = unsafe { libloading::Library::new(&path) }.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let api = unsafe { aizome_chanoma::retro::ffi::Api::of(&lib) }.unwrap();
            let mut info: aizome_chanoma::retro::ffi::retro_system_info = unsafe { std::mem::zeroed() };
            unsafe { (api.get_system_info)(&mut info) };
            assert_eq!(unsafe { (api.api_version)() }, 1);
            let name = unsafe { std::ffi::CStr::from_ptr(info.library_name) }.to_string_lossy();
            let exts =
                unsafe { std::ffi::CStr::from_ptr(info.valid_extensions) }.to_string_lossy().to_ascii_lowercase();
            println!("{:12} {stem:24} {name}: {exts}", sys.id);
            // Each extension Chanoma sends it is one the core takes.
            for ext in sys.extensions {
                if !exts.split('|').any(|e| e == *ext) {
                    println!("    (it doesn't list .{ext})");
                }
            }
            found += 1;
        }
    }
    assert!(found > 0, "no cores in {}", dir.display());
}

#[test]
#[ignore = "needs CHANOMA_TEST_CORES"]
fn every_core_given_a_file_that_isnt_a_game_goes_on() {
    // Each loads it as a game of its system's, through a whole session:
    // it says it can't, or starts on it, and Chanoma goes on (Flycast once
    // logged as it loaded, through a log it hadn't been given, and took
    // Chanoma down).
    let Some(dir) = cores() else { return };
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Debug).try_init();
    let tmp = std::env::temp_dir().join(format!("chanoma-not-a-game-{}", std::process::id()));
    let mut tried = 0;
    for sys in SYSTEMS {
        let Some(ext) = sys.extensions.first() else { continue };
        for stem in sys.cores {
            let core = dir.join(format!("{stem}_libretro.so"));
            if !core.is_file() {
                continue;
            }
            let here = tmp.join(stem);
            std::fs::create_dir_all(&here).unwrap();
            let game = here.join(format!("Not a Game.{ext}"));
            // A sheet names a track that isn't there (a copy left
            // unfinished); anything else is bytes that are no disc.
            let bytes = match *ext {
                "cue" => b"FILE \"Not a Game.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n".to_vec(),
                "gdi" => b"1\n1 0 4 2352 \"Not a Game.bin\" 0\n".to_vec(),
                "m3u" => b"Not a Game.cue\n".to_vec(),
                _ => vec![0x5a; 64 * 1024],
            };
            std::fs::write(&game, bytes).unwrap();
            let launch = Launch {
                core: CoreSource::Library(core.clone()),
                game,
                save_ram: here.join("game.srm"),
                state: here.join("game.state"),
                picture: here.join("game.png"),
                resume: false,
                system_dir: here.join("system"),
                bios: Vec::new(),
                found_bios: Vec::new(),
                save_dir: here.join("saves"),
                players: 1,
                prefer: Vec::new(),
                screen_lines: 1080,
                silent: true,
                unpack_dir: here.join("unzipped"),
                extensions: &[],
            };
            let (tx, rx) = mpsc::channel();
            let session = Session::start(launch, None, Arc::new(Shared::default()), None, move |e| {
                let _ = tx.send(e);
            });
            match rx.recv_timeout(Duration::from_secs(30)) {
                // A core that throws once it runs ends its game (`guard`).
                Ok(Event::Started { .. }) => match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(Event::Stopped(why)) => println!("{:12} {stem}: started on it, then {why}", sys.id),
                    _ => println!("{:12} {stem}: started on it", sys.id),
                },
                Ok(Event::Failed(why)) => println!("{:12} {stem}: {why}", sys.id),
                other => panic!("{stem}: {other:?}"),
            }
            session.stop(false);
            tried += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(tried > 0, "no cores in {}", dir.display());
}

#[test]
#[ignore = "needs CHANOMA_TEST_CORES"]
fn game_boy_cores_play_hear_the_controller_and_carry_on() {
    let Some(dir) = cores() else { return };
    let work = std::env::temp_dir().join(format!("chanoma-cores-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let rom = work.join("test.gb");
    std::fs::write(&rom, game_boy_rom()).unwrap();
    let mut played = 0;
    for stem in ["gambatte", "sameboy", "mgba"] {
        let core = dir.join(format!("{stem}_libretro.so"));
        if !core.is_file() {
            continue;
        }
        let here = work.join(stem);
        std::fs::create_dir_all(&here).unwrap();
        let (idle, resumed) = play(&core, &rom, &here, false, 0);
        assert!(!resumed);
        let (held, resumed) = play(&core, &rom, &here, true, Button::Right.bit());
        println!("{stem}: {idle:.0} untouched, {held:.0} with Right held, carried on: {resumed}");
        assert!(idle > 150.0, "{stem}: the screen should be light, is {idle:.0}");
        assert!(held < 80.0, "{stem}: Right should darken it, is {held:.0}");
        assert!(resumed, "{stem}: it should carry on from where it was left");
        played += 1;
    }
    assert!(played > 0, "no Game Boy cores in {}", dir.display());
}

/// A list of a household's own games, each played through `core` as
/// Chanoma plays a game of `system`'s (its players, its settings, the
/// BIOS images by the games under `folders`, offered as Chanoma offers
/// them), in stages eight seconds long, each carrying on from the last's
/// state as Chanoma carries a game on: untouched for two (its logos, its
/// film, its title), then Start and the confirm button pressed through the
/// rest. Each stage's picture is kept in `out` and its brightness printed.
/// The games that didn't start come back.
fn play_listed(
    core: &Path,
    system: &str,
    list: &Path,
    system_dir: &Path,
    folders: &[PathBuf],
    out: &Path,
    stages: u32,
) -> Vec<String> {
    let system = aizome_chanoma::library::system(system).unwrap();
    let games = std::fs::read_to_string(list).unwrap();
    let mut failed = Vec::new();
    for game in games.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let game = Path::new(game);
        let name = game.file_stem().unwrap().to_string_lossy().into_owned();
        let here = out.join(&name);
        std::fs::create_dir_all(&here).unwrap();
        let found_bios = aizome_chanoma::retro::bios::wanted(&aizome_chanoma::retro::bios::near(game, folders));
        let mut started = true;
        let mut seen = Vec::new();
        for stage in 1..=stages {
            let pads = Arc::new(Shared::default());
            let launch = Launch {
                core: CoreSource::Library(core.to_owned()),
                game: game.to_owned(),
                save_ram: here.join("game.srm"),
                state: here.join("game.state"),
                picture: here.join("game.png"),
                resume: stage > 1,
                system_dir: system_dir.to_owned(),
                bios: Vec::new(),
                found_bios: found_bios.clone(),
                save_dir: here.join("saves"),
                players: system.players,
                prefer: system.prefer.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
                screen_lines: 1080,
                silent: true,
                unpack_dir: here.join("unzipped"),
                extensions: &[],
            };
            let (tx, rx) = mpsc::channel();
            let session = Session::start(launch, None, pads.clone(), None, move |e| {
                let _ = tx.send(e);
            });
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(Event::Started { resumed, .. }) => {
                    if stage > 1 && !resumed {
                        println!("{name}: stage {stage} didn't carry on from the last");
                    }
                }
                other => {
                    println!("{name}: didn't start: {other:?}");
                    failed.push(name.clone());
                    started = false;
                    session.stop(false);
                    break;
                }
            }
            if stage <= 2 {
                std::thread::sleep(Duration::from_secs(8));
            } else {
                for b in [Button::Start, Button::South, Button::South, Button::Start] {
                    pads.set(0, true, b.bit(), [0.0; 6]);
                    std::thread::sleep(Duration::from_millis(150));
                    pads.set(0, true, 0, [0.0; 6]);
                    std::thread::sleep(Duration::from_millis(1850));
                }
            }
            session.stop(true);
            let shot = out.join(format!("{name} {stage}.png"));
            let _ = std::fs::copy(here.join("game.png"), &shot);
            seen.push(if shot.is_file() { format!("{:.0}", brightness(&shot)) } else { "-".into() });
        }
        if started {
            println!("{name}: played; brightness {}", seen.join(" "));
        }
    }
    failed
}

fn stages(var: &str) -> u32 {
    std::env::var(var).ok().and_then(|s| s.parse().ok()).unwrap_or(6)
}

/// Games played through PCSX-ReARMed with a system folder (OpenBIOS,
/// another BIOS, or none for its own stand-in), a picture kept every
/// eight seconds and its brightness said, Start and the confirm button
/// pressed from the third on. For looking at, not judged here: a game
/// that doesn't start fails.
///
/// `CHANOMA_TEST_PSX_GAMES` (a file of games, one path a line),
/// `CHANOMA_TEST_PSX_SYSTEM` (the system folder), `CHANOMA_TEST_PSX_OUT`
/// (where the pictures go), and `CHANOMA_TEST_PSX_STAGES` (how many
/// pictures, six if not said).
#[test]
#[ignore = "needs CHANOMA_TEST_CORES and CHANOMA_TEST_PSX_*"]
fn playstation_games_start_with_a_bios() {
    let (Some(dir), Some(list), Some(system), Some(out)) = (
        cores(),
        std::env::var_os("CHANOMA_TEST_PSX_GAMES").map(PathBuf::from),
        std::env::var_os("CHANOMA_TEST_PSX_SYSTEM").map(PathBuf::from),
        std::env::var_os("CHANOMA_TEST_PSX_OUT").map(PathBuf::from),
    ) else {
        return;
    };
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();
    let core = dir.join("pcsx_rearmed_libretro.so");
    let failed = play_listed(&core, "psx", &list, &system, &[], &out, stages("CHANOMA_TEST_PSX_STAGES"));
    assert!(failed.is_empty(), "these didn't start: {failed:?}");
}

/// Dreamcast games through Flycast, drawing with OpenGL (its frames read
/// back, with no compositor), with the BIOS a `bios` folder by the games
/// holds, offered as Chanoma offers it (`dc/dc_boot.bin`), as the
/// PlayStation's are played above. For looking at, not judged here: a
/// game that doesn't start fails, and one that crashes takes the test
/// down.
///
/// `CHANOMA_TEST_DC_GAMES` (a file of games, one path a line),
/// `CHANOMA_TEST_DC_ROOT` (the games folder, its `bios` beside it),
/// `CHANOMA_TEST_DC_OUT` (where the pictures go) and
/// `CHANOMA_TEST_DC_STAGES` (how many, six if not said).
#[test]
#[ignore = "needs CHANOMA_TEST_CORES and CHANOMA_TEST_DC_*"]
fn dreamcast_games_play_and_carry_on() {
    let (Some(dir), Some(list), Some(root), Some(out)) = (
        cores(),
        std::env::var_os("CHANOMA_TEST_DC_GAMES").map(PathBuf::from),
        std::env::var_os("CHANOMA_TEST_DC_ROOT").map(PathBuf::from),
        std::env::var_os("CHANOMA_TEST_DC_OUT").map(PathBuf::from),
    ) else {
        return;
    };
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();
    let core = dir.join("flycast_libretro.so");
    let system = out.join("system");
    let failed = play_listed(&core, "dreamcast", &list, &system, &[root], &out, stages("CHANOMA_TEST_DC_STAGES"));
    assert!(failed.is_empty(), "these didn't start: {failed:?}");
}
