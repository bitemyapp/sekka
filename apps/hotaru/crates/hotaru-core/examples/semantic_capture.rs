//! Development-only exact-state adapter for semantic metadata qualification.
use hotaru_core::{GraphemeWidthMethod, HistoryLimits, Terminal};
use serde_json::json;
use std::io::{Cursor, Read};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut cols: usize = args[2].parse().unwrap();
    let mut rows: usize = args[3].parse().unwrap();
    let chunk: usize = args[4].parse().unwrap();
    let method = if args[5] == "unicode" {
        GraphemeWidthMethod::Unicode
    } else {
        GraphemeWidthMethod::Legacy
    };
    let mut terminal = Terminal::with_history_limits(
        cols,
        rows,
        HistoryLimits {
            rows: 256,
            bytes: None,
        },
    );
    terminal.set_default_grapheme_width(method);
    let mut data = Cursor::new(std::fs::read(&args[1]).unwrap());
    let mut tag = [0];
    while data.read_exact(&mut tag).is_ok() {
        match tag[0] {
            b'F' => {
                let mut size = [0; 4];
                data.read_exact(&mut size).unwrap();
                let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
                data.read_exact(&mut bytes).unwrap();
                for bytes in bytes.chunks(chunk) {
                    terminal.feed(bytes);
                }
            }
            b'R' => {
                let mut size = [0; 4];
                data.read_exact(&mut size).unwrap();
                cols = u16::from_le_bytes(size[..2].try_into().unwrap()) as usize;
                rows = u16::from_le_bytes(size[2..].try_into().unwrap()) as usize;
                terminal.resize(cols, rows);
            }
            b'C' => while terminal.compress_history_step() {},
            _ => panic!("unknown fixture operation"),
        }
    }
    let snapshot = terminal.snapshot_all();
    let cells: Vec<_> = snapshot
        .lines
        .iter()
        .map(|row| {
            row.cells()
                .iter()
                .map(|cell| {
                    [
                        cell.character() as u32,
                        cell.semantic_content() as u32,
                        cell.width() as u32,
                        u32::from(cell.is_protected()),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect();
    println!(
        "{}",
        json!({"cols":cols,"rows":rows,"cursor":[snapshot.cursor.col,snapshot.cursor.row],"cells":cells,"row_prompts":snapshot.lines.iter().map(|row| row.semantic_prompt() as u8).collect::<Vec<_>>()})
    );
}
