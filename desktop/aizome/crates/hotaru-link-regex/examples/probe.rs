use hotaru_link_regex::{DEFAULT_REGEX, Limits, Matcher};
use std::{
    io::{self, BufRead},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn main() {
    if std::env::args().any(|a| a == "--regex") {
        print!("{DEFAULT_REGEX}");
        return;
    }
    let mut matcher = Matcher::new().unwrap();
    for line in io::stdin().lock().lines() {
        let text: String = serde_json::from_str(&line.unwrap()).unwrap();
        let limit = Limits {
            deadline: Instant::now() + Duration::from_secs(10),
            max_searches: 256,
            retry_allowance: 25_600_000,
            ..Default::default()
        };
        let result = match matcher.ranges(&text, &limit, &AtomicBool::new(false)) {
            Ok(r) => {
                serde_json::json!({"ranges":r.into_iter().map(|r|[r.start,r.end]).collect::<Vec<_>>()})
            }
            Err(e) => serde_json::json!({"error":format!("{e:?}")}),
        };
        println!("{result}");
    }
}
