//! A quick pass over every workload with every implemented machine: one
//! timed run each (no statistics), and the digest of its effects compared
//! with impl-enum's, so a benchmark can't silently measure a machine that
//! does something else. impl-implicit keeps the current code's confirmed
//! bugs, so its digests may differ on workloads that hit them.
//!
//! `cargo run --release -p bench --bin smoke` (add `SME_SMOKE=1` for small
//! workloads).

use std::time::Instant;

use bench::{drive, impls};
use spec::{install, lock, pointer, polkit, transfer, wrap};

struct Row {
    workload: String,
    implementation: &'static str,
    events: u64,
    ns: f64,
    digest: u64,
}

fn time(f: impl Fn() -> u64) -> (f64, u64) {
    let d = f();
    let start = Instant::now();
    let d2 = f();
    assert_eq!(d, d2, "a machine's digest changed between runs");
    (start.elapsed().as_nanos() as f64, d)
}

macro_rules! smoke {
    ($($name:literal => $krate:ident),* $(,)?) => {
        fn rows() -> Vec<Row> {
            let mut rows = Vec::new();
            let events = bench::pointer_stream();
            $(
                if bench::$krate::pointer::IMPLEMENTED {
                    let (ns, digest) = time(|| drive::pointer::run::<bench::$krate::pointer::Pointer>(pointer::Config::default(), &events));
                    rows.push(Row { workload: "pointer-stream".into(), implementation: $name, events: events.len() as u64, ns, digest });
                }
            )*
            for cols in [80u16, 200] {
                let (config, events) = bench::wrap_stream(cols);
                $(
                    if bench::$krate::wrap::IMPLEMENTED {
                        let (ns, digest) = time(|| drive::wrap::run::<bench::$krate::wrap::Wrap>(config, &events));
                        rows.push(Row { workload: format!("wrap-{cols}"), implementation: $name, events: events.len() as u64, ns, digest });
                    }
                )*
            }
            macro_rules! replay {
                ($t:ident, $Type:ident) => {{
                    let set = bench::replay::<$t::traces::Spec>();
                    let n = bench::replay_len(&set);
                    $(
                        if bench::$krate::$t::IMPLEMENTED {
                            let (ns, digest) = time(|| drive::$t::replay::<bench::$krate::$t::$Type>(&set));
                            rows.push(Row { workload: concat!("replay-", stringify!($t)).into(), implementation: $name, events: n, ns, digest });
                        }
                    )*
                }};
            }
            bench::targets!(replay);
            rows
        }
    };
}

impls!(smoke);

fn main() {
    let rows = rows();
    println!("{:<18} {:<15} {:>10} {:>10}  digest", "workload", "implementation", "events", "ns/event");
    let mut problems = 0;
    for r in &rows {
        let reference = rows.iter().find(|x| x.workload == r.workload && x.implementation == "enum").map(|x| x.digest);
        let check = match reference {
            Some(d) if d == r.digest => "same as enum",
            Some(_) if r.implementation == "implicit" => "differs (bugs kept)",
            Some(_) => {
                problems += 1;
                "DIFFERS FROM ENUM"
            }
            None => "?",
        };
        println!(
            "{:<18} {:<15} {:>10} {:>10.2}  {check}",
            r.workload,
            r.implementation,
            r.events,
            r.ns / r.events as f64
        );
    }
    if problems > 0 {
        eprintln!("{problems} workload(s) where an implementation's effects differ from impl-enum's");
        std::process::exit(1);
    }
}
