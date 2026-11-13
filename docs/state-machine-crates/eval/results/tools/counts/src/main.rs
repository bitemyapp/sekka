//! Prints, as TSV, the size of every benchmark workload built exactly as
//! `bench` builds it (same seeds and sizes): events, and for wrap the
//! printed cells, counted as the `Put` effects the reference machine
//! (impl-enum) emits (a `Print` is one, a `Repeat(n)` is `max(n, 1)` when
//! there is a previous scalar).

use spec::wrap::Machine as _;
use spec::{Sink, pointer, wrap};

#[derive(Default)]
struct Puts {
    puts: u64,
    effects: u64,
}

impl Sink<wrap::Effect> for Puts {
    fn push(&mut self, e: wrap::Effect) {
        self.effects += 1;
        if matches!(e, wrap::Effect::Put { .. }) {
            self.puts += 1;
        }
    }
}

fn main() {
    println!("suite\tworkload\tevents\tprint_events\trepeat_events\tputs\teffects");
    for (suite, n) in [("instructions", 20_000usize), ("criterion", 1_000_000)] {
        let events = pointer::workload::stream(0xC0FFEE, n);
        println!("{suite}\tpointer-stream\t{}\t-\t-\t-\t-", events.len());
    }
    for (suite, cells) in [("instructions", 200_000usize), ("criterion", 10_000_000)] {
        for cols in [80u16, 200] {
            let rows = if cols >= 200 { 50 } else { 24 };
            let events = wrap::workload::stream(0x7E57 + u64::from(cols), cols, rows, cells);
            let prints = events.iter().filter(|e| matches!(e, wrap::Event::Print { .. })).count();
            let repeats = events.iter().filter(|e| matches!(e, wrap::Event::Repeat(_))).count();
            let mut m = impl_enum::wrap::Wrap::new(wrap::Config { cols, rows });
            let mut sink = Puts::default();
            for e in &events {
                m.handle(*e, &mut sink);
            }
            println!("{suite}\twrap-{cols}\t{}\t{prints}\t{repeats}\t{}\t{}", events.len(), sink.puts, sink.effects);
        }
    }
}
