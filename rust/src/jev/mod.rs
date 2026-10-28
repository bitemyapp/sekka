//! Experimental vision-assisted VM testing. Never a publication authority.
mod api;
mod benchmark;
mod machine;
mod scenario;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 1024 * 128];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn save(path: &Path, value: &Value) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}

pub fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn private_dir(path: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let resolved = parent
        .canonicalize()?
        .join(path.file_name().context("Directory needs a file name")?);
    ensure!(
        !resolved.to_string_lossy().contains([',', '\n', '\r']),
        "Unsafe directory path"
    );
    fs::DirBuilder::new().mode(0o700).create(&resolved)?;
    Ok(resolved)
}

pub fn event(dir: &Path, value: Value) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join("events.jsonl"))?;
    writeln!(file, "{}", json!({"time_ms":now(),"event":value}))?;
    file.sync_all()?;
    Ok(())
}

fn arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .with_context(|| format!("Missing {name}; use --help"))
}

pub fn main(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("Use sekka-jev --help")
    };
    match command {
        "--help" | "-h" => println!(
            "\
Experimental Jev/QEMU harness (no release certification).
  sekka-jev probe IMAGE.png QUESTIONS.json NEW-OUTPUT-DIR [ENV-FILE]
  sekka-jev benchmark CORPUS.json NEW-OUTPUT-DIR [ENV-FILE]
  sekka-jev start NEW-RUN-DIR VM-CONFIG.json
  sekka-jev boot-disk RUN-DIR
  sekka-jev capture RUN-DIR
  sekka-jev run RUN-DIR SCENARIO.json [ENV-FILE]
  sekka-jev shutdown RUN-DIR
  sekka-jev status RUN-DIR
  sekka-jev manual RUN-DIR key|type|click VALUE [Y]
JEV_API_KEY is read from the environment or the explicitly supplied dotenv file.
Only the official https://api.typesafe.ai endpoint receives it.
See tests/jev and DEVELOPERS.md for configuration and evidence limits."
        ),
        "benchmark" => {
            ensure!(
                (3..=4).contains(&args.len()),
                "benchmark needs corpus, new output directory, optional env file"
            );
            benchmark::run(
                Path::new(&args[1]),
                Path::new(&args[2]),
                args.get(3).map(Path::new),
            )?;
        }
        "probe" => {
            ensure!(
                (4..=5).contains(&args.len()),
                "probe needs image, questions, new output directory, optional env file"
            );
            let image = Path::new(arg(&args, 1, "image")?).canonicalize()?;
            let questions = read(Path::new(arg(&args, 2, "questions")?))?;
            api::validate_questions(&questions)?;
            let output = private_dir(Path::new(arg(&args, 3, "output")?))?;
            fs::copy(&image, output.join("input.png"))?;
            let mut client = api::Client::new(&output, args.get(4).map(Path::new), 10, 655360)?;
            let response = client.ask(&output.join("input.png"), questions)?;
            save(&output.join("summary.json"), &client.summary())?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        "start" => {
            ensure!(
                args.len() == 3,
                "start needs a new run directory and VM configuration"
            );
            machine::start(Path::new(&args[1]), Path::new(&args[2]))?;
        }
        "boot-disk" | "capture" | "shutdown" | "status" => {
            ensure!(args.len() == 2, "{command} needs run directory");
            let run = Path::new(&args[1]).canonicalize()?;
            match command {
                "boot-disk" => machine::boot_disk(&run)?,
                "capture" => println!("{}", machine::capture(&run)?.display()),
                "shutdown" => machine::shutdown(&run)?,
                "status" => println!("{}", machine::status(&run)?),
                _ => unreachable!(),
            }
        }
        "manual" => {
            ensure!(
                (4..=5).contains(&args.len()),
                "manual needs run, key|type|click and values"
            );
            machine::manual(Path::new(&args[1]), &args[2..])?;
        }
        "run" => {
            ensure!(
                (3..=4).contains(&args.len()),
                "run needs run directory, scenario and optional env file"
            );
            scenario::run(
                Path::new(&args[1]),
                Path::new(&args[2]),
                args.get(3).map(Path::new),
            )?;
        }
        _ => bail!("Unknown command; use --help"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_alias_cannot_hide_qemu_delimiter() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("unsafe,parent")).unwrap();
        std::os::unix::fs::symlink(root.path().join("unsafe,parent"), root.path().join("alias"))
            .unwrap();
        assert!(private_dir(&root.path().join("alias/run")).is_err());
        assert!(!root.path().join("unsafe,parent/run").exists());
    }
}
