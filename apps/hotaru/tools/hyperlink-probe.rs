//! Development-only metadata probe. Copy into an isolated core examples/
//! directory before building; this is never linked into the Hotaru application.
use hotaru_core::Terminal;
use hotaru_core::formatter::{CopyFormat,FormatOptions,format_copy};
use serde_json::json;
use std::sync::atomic::AtomicBool;

fn main() {
    let args:Vec<_>=std::env::args().collect();
    assert_eq!(args.len(),4,"hyperlink-probe DATA ROWS COLS");
    let rows:usize=args[2].parse().unwrap();let cols:usize=args[3].parse().unwrap();
    assert!(rows>0&&rows<=4096&&cols>0&&cols<=8192);
    let data=std::fs::read(&args[1]).unwrap();
    let mut terminal=Terminal::new(cols,rows,0);
    for chunk in data.chunks(65536){terminal.feed(chunk);}
    let snapshot=terminal.snapshot();
    let links:Vec<Vec<Vec<u8>>>=snapshot.lines.iter().map(|line|(0..cols).map(|col|line.hyperlink(col).map_or_else(Vec::new,|link|link.uri.to_vec())).collect()).collect();
    let copy=format_copy(&snapshot.into(),&FormatOptions{format:CopyFormat::Vt,..FormatOptions::default()},&AtomicBool::new(false)).unwrap();
    println!("{}",json!({"links":links,"rows":rows,"cols":cols,"vt":copy.plain.unwrap()}));
}
