//! QMP input for the fixed 1280x800 disposable test display, not the host desktop.
use crate::{support::*, vm::Rpc};
use anyhow::{Context, Result, bail, ensure};
use serde_json::json;
use std::{fs, path::Path, time::Duration};

pub fn key(qmp: &mut Rpc, names: &[&str]) -> Result<()> {
    qmp.call("send-key", json!({"keys": names.iter().map(|n| json!({"type":"qcode","data":n})).collect::<Vec<_>>(), "hold-time":30}))?;
    pause(Duration::from_millis(80))
}
fn codes(c: char) -> Result<Vec<String>> {
    let (name, shift) = match c {
        'a'..='z' | '0'..='9' => (c.to_string(), false),
        'A'..='Z' => (c.to_ascii_lowercase().to_string(), true),
        ' ' => ("spc".into(), false),
        '-' => ("minus".into(), false),
        '_' => ("minus".into(), true),
        '/' => ("slash".into(), false),
        '.' => ("dot".into(), false),
        '!' => ("1".into(), true),
        '$' => ("4".into(), true),
        '{' => ("bracket_left".into(), true),
        '}' => ("bracket_right".into(), true),
        ':' => ("semicolon".into(), true),
        '@' => ("2".into(), true),
        '#' => ("3".into(), true),
        '%' => ("5".into(), true),
        '^' => ("6".into(), true),
        '&' => ("7".into(), true),
        '*' => ("8".into(), true),
        '(' => ("9".into(), true),
        ')' => ("0".into(), true),
        '=' => ("equal".into(), false),
        '+' => ("equal".into(), true),
        '[' => ("bracket_left".into(), false),
        ']' => ("bracket_right".into(), false),
        '\\' => ("backslash".into(), false),
        '|' => ("backslash".into(), true),
        ';' => ("semicolon".into(), false),
        '\'' => ("apostrophe".into(), false),
        '"' => ("apostrophe".into(), true),
        ',' => ("comma".into(), false),
        '<' => ("comma".into(), true),
        '>' => ("dot".into(), true),
        '?' => ("slash".into(), true),
        '`' => ("grave_accent".into(), false),
        '~' => ("grave_accent".into(), true),
        _ => bail!("Unsupported US-layout test character"),
    };
    Ok(if shift {
        vec!["shift".into(), name]
    } else {
        vec![name]
    })
}
pub fn type_text(qmp: &mut Rpc, text: &str) -> Result<()> {
    // Validate before sending any input.
    let all = text.chars().map(codes).collect::<Result<Vec<_>>>()?;
    for names in all {
        key(qmp, &names.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    Ok(())
}
fn click(qmp: &mut Rpc, x: u32, y: u32) -> Result<()> {
    ensure!(x < 1280 && y < 800, "Outside the fixed test display");
    qmp.call(
        "input-send-event",
        json!({"events":[
            {"type":"abs","data":{"axis":"x","value":x*32767/1279}},
            {"type":"abs","data":{"axis":"y","value":y*32767/799}},
            {"type":"btn","data":{"down":true,"button":"left"}}
        ]}),
    )?;
    qmp.call(
        "input-send-event",
        json!({"events":[{"type":"btn","data":{"down":false,"button":"left"}}]}),
    )?;
    pause(Duration::from_millis(200))
}
pub fn main(args: Vec<String>) -> Result<()> {
    ensure!(
        args.len() >= 2,
        "Usage: qmp_input.rs RUN-NAME screenshot|key|type|click [VALUES]"
    );
    let name = &args[0];
    ensure!(
        Path::new(name).file_name().and_then(|s| s.to_str()) == Some(name)
            && ![".", ".."].contains(&name.as_str()),
        "Supply a run name, not a path"
    );
    let repo = repo()?;
    let work = repo.join(".work").join(name).canonicalize()?;
    ensure!(
        work.parent() == Some(repo.join(".work").as_path()),
        "Not a local test run"
    );
    regular(&work.join("target.raw"))?;
    ensure!(
        [40 * 1024u64.pow(3), 80 * 1024u64.pow(3)]
            .contains(&fs::metadata(work.join("target.raw"))?.len()),
        "Not a 40 or 80 GiB test image"
    );
    let mut qmp = Rpc::connect(&work.join("qmp.sock"), true, 20)?;
    match args[1].as_str() {
        "screenshot" if args.len() == 2 => {
            let path = repo
                .join("artifacts")
                .join(name)
                .join(format!("gui-{}.png", stamp()));
            qmp.call("screendump", json!({"filename":path,"format":"png"}))?;
            println!("{}", path.display());
        }
        "key" if args.len() == 3 => key(&mut qmp, &args[2].split('+').collect::<Vec<_>>())?,
        "type" if args.len() == 3 => type_text(&mut qmp, &args[2])?,
        "click" if args.len() == 4 => click(
            &mut qmp,
            args[2].parse().context("x coordinate")?,
            args[3].parse().context("y coordinate")?,
        )?,
        _ => bail!("Invalid input operation"),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_us_test_keys() {
        assert_eq!(codes('!').unwrap(), ["shift", "1"]);
        assert_eq!(codes('A').unwrap(), ["shift", "a"]);
        assert_eq!(codes('{').unwrap(), ["shift", "bracket_left"]);
        assert_eq!(codes('~').unwrap(), ["shift", "grave_accent"]);
        assert_eq!(codes('\'').unwrap(), ["apostrophe"]);
        // Every printable ASCII character can be typed.
        assert!((' '..='~').all(|c| codes(c).is_ok()));
        assert!(codes('\n').is_err());
    }
}
