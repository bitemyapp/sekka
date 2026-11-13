//! Criterion benchmarks over every implemented machine:
//!
//! - `pointer-stream`: the synthetic pointer stream (1M events);
//! - `wrap-80`, `wrap-200`: ~10M printed cells with controls, at 80 and
//!   200 columns;
//! - `replay-<target>`: every conformance trace, each on a fresh machine
//!   (throughput in events).
//!
//! `SME_SMOKE=1` shrinks the workloads and the measurement times.
//! Filter as usual: `cargo bench -p bench --bench machines -- 'wrap-80/statig'`.

use std::time::Duration;

use bench::{drive, impls, smoke};
use criterion::{BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main, measurement::WallTime};
use spec::{install, lock, pointer, polkit, transfer, wrap};

fn tune(g: &mut BenchmarkGroup<'_, WallTime>, heavy: bool) {
    if smoke() {
        g.sample_size(10).warm_up_time(Duration::from_millis(100)).measurement_time(Duration::from_millis(300));
    } else if heavy {
        g.sample_size(20).measurement_time(Duration::from_secs(10));
    }
}

macro_rules! stream_benches {
    ($($name:literal => $krate:ident),* $(,)?) => {
        fn pointer_stream(c: &mut Criterion) {
            let events = bench::pointer_stream();
            let config = pointer::Config::default();
            let mut g = c.benchmark_group("pointer-stream");
            tune(&mut g, false);
            g.throughput(Throughput::Elements(events.len() as u64));
            $(
                if bench::$krate::pointer::IMPLEMENTED {
                    g.bench_function($name, |b| b.iter(|| drive::pointer::run::<bench::$krate::pointer::Pointer>(config, &events)));
                }
            )*
            g.finish();
        }

        fn wrap_streams(c: &mut Criterion) {
            for cols in [80u16, 200] {
                let (config, events) = bench::wrap_stream(cols);
                let mut g = c.benchmark_group(format!("wrap-{cols}"));
                tune(&mut g, true);
                g.throughput(Throughput::Elements(events.len() as u64));
                $(
                    if bench::$krate::wrap::IMPLEMENTED {
                        g.bench_function($name, |b| b.iter(|| drive::wrap::run::<bench::$krate::wrap::Wrap>(config, &events)));
                    }
                )*
                g.finish();
            }
        }

        fn replays(c: &mut Criterion) {
            macro_rules! replay {
                ($t:ident, $Type:ident) => {{
                    let set = bench::replay::<$t::traces::Spec>();
                    let mut g = c.benchmark_group(concat!("replay-", stringify!($t)));
                    tune(&mut g, false);
                    g.throughput(Throughput::Elements(bench::replay_len(&set)));
                    $(
                        if bench::$krate::$t::IMPLEMENTED {
                            g.bench_function($name, |b| b.iter(|| drive::$t::replay::<bench::$krate::$t::$Type>(&set)));
                        }
                    )*
                    g.finish();
                }};
            }
            bench::targets!(replay);
        }
    };
}

impls!(stream_benches);

criterion_group!(benches, pointer_stream, wrap_streams, replays);
criterion_main!(benches);
