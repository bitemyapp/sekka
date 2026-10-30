#[allow(dead_code)]
#[path = "../src/automatic_links/mod.rs"]
mod automatic_links;
use automatic_links::mapping;
use hotaru_link_regex::{CellPoint, Limits, Matcher};
use std::{
    io::{self, BufRead},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn main() {
    let mut matcher = Matcher::new().unwrap();
    for line in io::stdin().lock().lines() {
        let data: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut terminal = hotaru_core::Terminal::new(
            data["cols"].as_u64().unwrap() as usize,
            data["rows"].as_u64().unwrap() as usize,
            0,
        );
        if data["unicode"].as_bool().unwrap_or(true) {
            terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        }
        terminal.feed(data["data"].as_str().unwrap().as_bytes());
        let mut snapshot = terminal.snapshot();
        snapshot.position.first_row = data["first_row"].as_u64().unwrap_or(0) as usize;
        let at = CellPoint { row: data["row"].as_u64().unwrap(), col: data["col"].as_u64().unwrap() as u16 };
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(10);
        let out = match mapping::extract(&snapshot, at, &cancel, deadline) {
            Ok(mapped) => {
                let found = matcher.find_at(&mapped.source, at, &Limits { deadline, ..Default::default() }, &cancel);
                let found = match found {
                    Ok(Some(found)) => {
                        serde_json::json!({"text":found.text(),"bytes":[found.bytes().start,found.bytes().end],"start":[found.start().row,found.start().col],"end":[found.end().row,found.end().col]})
                    }
                    Ok(None) => serde_json::Value::Null,
                    Err(error) => serde_json::json!({"error":format!("{error:?}")}),
                };
                serde_json::json!({"text":mapped.source.text(),"start":[mapped.start.row,mapped.start.col],"end":[mapped.end.row,mapped.end.col],"found":found})
            }
            Err(error) => serde_json::json!({"error":format!("{error:?}")}),
        };
        println!("{out}");
    }
}
