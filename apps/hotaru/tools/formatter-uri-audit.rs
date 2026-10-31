use hotaru_core::{
    Terminal,
    formatter::{CopyFormat, FormatOptions, format_copy},
};
use std::{
    io::{self, BufRead},
    sync::atomic::AtomicBool,
};
fn main() {
    for line in io::stdin().lock().lines() {
        let uri: Vec<u8> = serde_json::from_str(&line.unwrap()).unwrap();
        let mut t = Terminal::new(1, 1, 0);
        let mut input = b"\x1b]8;;".to_vec();
        input.extend_from_slice(&uri);
        input.extend_from_slice(b"\x1b\\X");
        t.feed(&input);
        let output = format_copy(
            &t.content_snapshot(),
            &FormatOptions {
                format: CopyFormat::Html,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        println!(
            "{}",
            serde_json::to_string(&String::from_utf8(output.html.unwrap()).unwrap()).unwrap()
        );
    }
}
