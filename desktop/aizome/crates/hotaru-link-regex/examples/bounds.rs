use hotaru_link_regex::{Limits, Matcher};
use std::{
    io::{self, Read},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let request: serde_json::Value = serde_json::from_str(&input).unwrap();
    let text = request["text"].as_str().unwrap();
    let started = Instant::now();
    let mut matcher = Matcher::new().unwrap();
    let init = started.elapsed();
    let millis = request["deadline_ms"].as_u64().unwrap_or(25).min(10_000);
    let limits = Limits {
        deadline: Instant::now() + Duration::from_millis(millis),
        ..Default::default()
    };
    let begin = Instant::now();
    let result = matcher.ranges(
        text,
        &limits,
        &AtomicBool::new(request["cancel"].as_bool().unwrap_or(false)),
    );
    let elapsed = begin.elapsed();
    let value = match result {
        Ok(r) => {
            serde_json::json!({"ranges":r.into_iter().map(|r|[r.start,r.end]).collect::<Vec<_>>()})
        }
        Err(e) => serde_json::json!({"error":format!("{e:?}")}),
    };
    println!(
        "{}",
        serde_json::json!({"bytes":text.len(),"init_ns":init.as_nanos(),"search_ns":elapsed.as_nanos(),"result":value})
    );
}
