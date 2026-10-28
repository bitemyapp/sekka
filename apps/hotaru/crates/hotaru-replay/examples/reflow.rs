//! JSON operation runner for comparison with the actual libghostty-vt library.
use hotaru_core::{Color, Rgb, Terminal};
use serde_json::{Value, json};
use std::io::{self, Read};

fn color(value: Color) -> Value {
    match value {
        Color::Default => json!({"default":true}),
        Color::Indexed(index) => json!({"index":index}),
        Color::Rgb(Rgb(r, g, b)) => json!({"rgb":[r,g,b]}),
    }
}
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&input).unwrap();
    let mut results = Vec::new();
    for case in cases {
        let mut terminal = Terminal::new(
            case["cols"].as_u64().unwrap() as usize,
            case["rows"].as_u64().unwrap() as usize,
            case["history"].as_u64().unwrap_or(10_000) as usize,
        );
        let mut steps = Vec::new();
        for op in case["ops"].as_array().unwrap() {
            if let Some(text) = op["write"].as_str() {
                terminal.feed(text.as_bytes());
            }
            if let Some(size) = op["resize"].as_array() {
                terminal.resize(
                    size[0].as_u64().unwrap() as usize,
                    size[1].as_u64().unwrap() as usize,
                );
            }
            let snapshot = terminal.snapshot_all();
            let lines: Vec<_> = snapshot.lines.iter().map(|line| {
                let cells: Vec<_> = line.cells().iter().enumerate().map(|(col, &cell)| {
                    let style = line.style(cell);
                    json!({"text":line.grapheme(col).map(str::to_owned).unwrap_or_else(||cell.character().to_string()),
                        "width":cell.width(),"foreground":color(style.foreground),"background":color(style.background),
                        "underline_color":color(style.underline_color),"attrs":style.attrs,"protected":cell.is_protected()})
                }).collect();
                json!({"wrapped":line.wrapped(),"cells":cells})
            }).collect();
            steps.push(json!({"cols":snapshot.cols,"rows":terminal.snapshot().rows,"history":terminal.scrollback_len(),
                "cursor":[snapshot.cursor.col,snapshot.cursor.row], "lines":lines,"responses":terminal.take_responses()}));
        }
        results.push(json!({"name":case["name"],"steps":steps}));
    }
    println!("{}", serde_json::to_string(&results).unwrap());
}
