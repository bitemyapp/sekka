use hotaru_core::{Rgb, Terminal};
use std::io::{self, Read, Write};

fn main() {
    if let Err(error) = run() {
        eprintln!("hotaru-replay: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = 80usize;
    let mut cols = 120usize;
    let mut format = "plain".to_owned();
    let mut data = None;
    let mut bench = false;
    let mut bench_json = false;
    let mut iterations = 10usize;
    let mut history = 0usize;
    let mut chunk = 65536usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = Some(args.next().ok_or("--data requires a path")?),
            "--rows" => rows = args.next().ok_or("--rows requires a number")?.parse()?,
            "--cols" => cols = args.next().ok_or("--cols requires a number")?.parse()?,
            "--scrollback" => {
                history = args
                    .next()
                    .ok_or("--scrollback requires a number")?
                    .parse()?
            }
            "--chunk" => chunk = args.next().ok_or("--chunk requires a number")?.parse()?,
            "--format" => format = args.next().ok_or("--format requires plain or json")?,
            "--bench" => bench = true,
            "--bench-json" => bench_json = true,
            "--iterations" => {
                iterations = args
                    .next()
                    .ok_or("--iterations requires a number")?
                    .parse()?
            }
            "--help" | "-h" => {
                println!(
                    "hotaru-replay [--data FILE] [--rows 80] [--cols 120] [--format plain|json] [--scrollback 0] [--chunk 65536] [--bench | --bench-json --iterations 10]\nReads stdin when --data is omitted. --bench suppresses serialization. --bench-json measures feed separately from setup/teardown with zero history."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    if rows == 0
        || cols == 0
        || rows > hotaru_core::MAX_ROWS
        || cols > hotaru_core::MAX_COLS
        || chunk == 0
    {
        return Err("dimensions/chunk outside supported bounds".into());
    }
    if !matches!(format.as_str(), "plain" | "json") {
        return Err("format must be plain or json (VT formatter port is pending)".into());
    }
    if bench_json && (history != 0 || bench || iterations == 0 || iterations > 100_000) {
        return Err(
            "--bench-json requires zero scrollback, 1..100000 iterations and no --bench".into(),
        );
    }
    let mut bytes = Vec::new();
    if let Some(path) = data {
        bytes = std::fs::read(path)?;
    } else {
        io::stdin().read_to_end(&mut bytes)?;
    }
    if bench_json {
        use std::time::Instant;
        let mut setup = Vec::with_capacity(iterations);
        let mut feed = Vec::with_capacity(iterations);
        let mut teardown = Vec::with_capacity(iterations);
        let mut response_bytes = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let mut terminal = Terminal::new(cols, rows, 0);
            setup.push(start.elapsed().as_nanos() as u64);
            let start = Instant::now();
            for part in bytes.chunks(chunk) {
                terminal.feed(part);
            }
            feed.push(start.elapsed().as_nanos() as u64);
            response_bytes.push(terminal.pending_response_bytes());
            std::hint::black_box(&terminal);
            let start = Instant::now();
            drop(terminal);
            teardown.push(start.elapsed().as_nanos() as u64);
        }
        serde_json::to_writer(
            io::stdout().lock(),
            &serde_json::json!({"schema":1,"engine":"hotaru",
            "timed_scope":"feed-only","input_bytes":bytes.len(),"rows":rows,"cols":cols,
            "scrollback":0,"chunk_bytes":chunk,"samples_ns":feed,"setup_ns":setup,"teardown_ns":teardown,
            "response_bytes":response_bytes}),
        )?;
        return Ok(());
    }
    let mut terminal = Terminal::new(cols, rows, history);
    let mut responses = Vec::new();
    for part in bytes.chunks(chunk) {
        terminal.feed(part);
        let reply = terminal.take_responses();
        if bench {
            std::hint::black_box(reply);
        } else {
            responses.extend(reply);
        }
        terminal.take_events();
    }
    if bench {
        std::hint::black_box(&terminal);
        return Ok(());
    }
    let snapshot = terminal.snapshot();
    let mut output = io::BufWriter::new(io::stdout().lock());
    if format == "plain" {
        output.write_all(snapshot.plain_text().as_bytes())?;
    } else {
        let lines:Vec<_>=snapshot.lines.iter().map(|line|{
            let mut text=String::new();line.append_text(&mut text,0,cols,true);
            let cells:Vec<_>=line.cells().iter().enumerate().map(|(col,&cell)|{
                let style=line.style(cell);
                serde_json::json!({"text":line.grapheme(col).map(str::to_owned).unwrap_or_else(||cell.character().to_string()),"width":cell.width(),"foreground":color(style.foreground),"background":color(style.background),"underline_color":color(style.underline_color),"attrs":style.attrs})
            }).collect();
            serde_json::json!({"text":text,"wrapped":line.wrapped(),"cells":cells})
        }).collect();
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({
                "cols":cols,"rows":rows,
                "cursor":{"row":snapshot.cursor.row,"col":snapshot.cursor.col,"visible":snapshot.cursor.visible},
                "lines":lines,"responses":responses,
                "modes":{
                    "application_cursor":snapshot.modes.application_cursor,
                    "bracketed_paste":snapshot.modes.bracketed_paste,
                    "focus_events":snapshot.modes.focus_events,
                    "alternate_screen":snapshot.modes.alternate_screen
                },
                "palette":snapshot.palette.iter().map(|Rgb(r,g,b)| [*r,*g,*b]).collect::<Vec<_>>(),
                "default_foreground":[snapshot.default_foreground.0,snapshot.default_foreground.1,snapshot.default_foreground.2],
                "default_background":[snapshot.default_background.0,snapshot.default_background.1,snapshot.default_background.2]
            }),
        )?;
    }
    output.flush()?;
    Ok(())
}

fn color(color: hotaru_core::Color) -> serde_json::Value {
    match color {
        hotaru_core::Color::Default => serde_json::json!({"default":true}),
        hotaru_core::Color::Indexed(index) => serde_json::json!({"index":index}),
        hotaru_core::Color::Rgb(hotaru_core::Rgb(r, g, b)) => serde_json::json!({"rgb":[r,g,b]}),
    }
}
