//! The messages read, and what the filter makes of them: the rows that
//! show, the counts the sidebar and the status line give, and the
//! timeline's histogram. Made again in `update` when the filter or the
//! messages change; messages that arrive live are only added.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::entry::{Entry, Priority, QuickMap, address};
use crate::filter::{Filter, Scope};

/// The messages read, oldest first. Each has a `seq` in the same order,
/// which stays its own while it's kept.
#[derive(Default)]
pub struct Store {
    pub entries: Vec<Rc<Entry>>,
    lowest: u64,
    highest: u64,
    /// Nothing older is left to read.
    pub complete: bool,
}

/// seq of the first entry read: older ones count down from it, newer up.
const MIDDLE: u64 = 1 << 62;

impl Store {
    pub fn clear(&mut self) {
        *self = Store::default();
    }

    /// Older messages, newest first (as journalctl reads backwards).
    pub fn prepend(&mut self, newest_first: Vec<Entry>) {
        if newest_first.is_empty() {
            return;
        }
        if self.entries.is_empty() {
            self.lowest = MIDDLE + 1;
            self.highest = MIDDLE;
        }
        let mut older: Vec<Rc<Entry>> = newest_first
            .into_iter()
            .map(|mut e| {
                self.lowest -= 1;
                e.seq = self.lowest;
                Rc::new(e)
            })
            .collect();
        older.reverse();
        older.append(&mut self.entries);
        self.entries = older;
    }

    /// Newer messages, oldest first.
    pub fn append(&mut self, oldest_first: Vec<Entry>) {
        if self.entries.is_empty() {
            self.lowest = MIDDLE;
            self.highest = MIDDLE - 1;
        }
        for mut e in oldest_first {
            self.highest += 1;
            e.seq = self.highest;
            self.entries.push(Rc::new(e));
        }
    }

    pub fn index_of(&self, seq: u64) -> Option<usize> {
        self.entries.binary_search_by_key(&seq, |e| e.seq).ok()
    }

    pub fn get(&self, seq: u64) -> Option<&Entry> {
        self.index_of(seq).map(|i| &*self.entries[i])
    }

    pub fn first_time(&self) -> Option<i64> {
        self.entries.iter().map(|e| e.time).find(|t| *t > 0)
    }

    pub fn last_time(&self) -> Option<i64> {
        self.entries.iter().rev().map(|e| e.time).find(|t| *t > 0)
    }
}

/// A source's messages, as the sidebar counts them.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceCount {
    pub name: Arc<str>,
    pub count: usize,
    pub problems: usize,
    pub errors: usize,
}

/// How many messages over time, in buckets: errors, warnings, the rest.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct Histogram {
    pub from: i64,
    pub to: i64,
    pub buckets: Vec<[u32; 3]>,
}

pub const BUCKETS: usize = 240;

impl Histogram {
    pub fn bucket_of(&self, t: i64) -> usize {
        if self.to <= self.from {
            return 0;
        }
        let f = (t - self.from) as f64 / (self.to - self.from) as f64;
        ((f * BUCKETS as f64) as usize).min(BUCKETS - 1)
    }

    /// The time at a fraction of the way along.
    pub fn time_at(&self, f: f64) -> i64 {
        self.from + ((self.to - self.from) as f64 * f.clamp(0.0, 1.0)) as i64
    }

    pub fn fraction(&self, t: i64) -> f64 {
        if self.to <= self.from { 0.0 } else { ((t - self.from) as f64 / (self.to - self.from) as f64).clamp(0.0, 1.0) }
    }

    pub fn tallest(&self) -> u32 {
        self.buckets.iter().map(|b| b[0] + b[1] + b[2]).max().unwrap_or(0)
    }
}

/// The messages that show, in chunks shared with the table: a message
/// arriving copies only the last chunk the table still holds, not all.
#[derive(Clone, Default)]
pub struct Shown {
    chunks: Vec<Rc<Vec<Rc<Entry>>>>,
    len: usize,
}

const CHUNK: usize = 4096;

impl Shown {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, e: Rc<Entry>) {
        match self.chunks.last_mut() {
            Some(last) if last.len() < CHUNK => Rc::make_mut(last).push(e),
            _ => {
                let mut v = Vec::with_capacity(CHUNK);
                v.push(e);
                self.chunks.push(Rc::new(v));
            }
        }
        self.len += 1;
    }

    pub fn get(&self, i: usize) -> Option<&Rc<Entry>> {
        self.chunks.get(i / CHUNK).and_then(|c| c.get(i % CHUNK))
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Rc<Entry>> + '_ {
        self.chunks.iter().flat_map(|c| c.iter())
    }
}

impl std::ops::Index<usize> for Shown {
    type Output = Entry;

    fn index(&self, i: usize) -> &Entry {
        &self.chunks[i / CHUNK][i % CHUNK]
    }
}

/// What the filter makes of the messages.
#[derive(Default)]
pub struct View {
    /// Indices of the messages that show, oldest first.
    pub rows: Rc<Vec<u32>>,
    /// Those messages, shared with the table.
    pub shown: Shown,
    pub errors: usize,
    pub warnings: usize,
    /// Each journal scope's messages, unfiltered.
    pub scopes: [usize; 6],
    /// What each source wrote, with the filter but for sources and time.
    pub sources: Rc<Vec<SourceCount>>,
    pub histogram: Arc<Histogram>,
    /// Messages the timeline charts (the filter but for its time): when,
    /// and which of the histogram's kinds.
    charted: Vec<(i64, u8)>,
    /// Each source's place in `counted`: by its address, and by its name
    /// (where two copies of one name meet).
    source_at: QuickMap<usize, usize>,
    source_named: HashMap<Arc<str>, usize>,
    counted: Vec<SourceCount>,
    /// How many entries this was made from.
    pub seen: usize,
    /// What the overview's suggestions are made of, tallied over the rows.
    pub notable: Notable,
}

/// What stands out among the rows: tallied as they're taken, so new
/// messages only add to it.
#[derive(Clone, Debug, Default)]
pub struct Notable {
    pub errors_by_source: HashMap<Arc<str>, usize>,
    /// Units that failed, in the order they did.
    pub failed: Vec<Arc<str>>,
    pub crashes: usize,
    /// What crashed, in the order it did.
    pub crashed: Vec<String>,
    /// The kernel's out-of-memory kills, and the last one's entry index.
    pub oom: usize,
    pub last_oom: Option<u32>,
}

impl View {
    /// The view of all of the store.
    pub fn make(store: &Store, filter: &Filter, uid: u32) -> View {
        let mut v = View::default();
        let (mut rows, mut shown) = (Vec::new(), Shown::default());
        v.take(store, filter, uid, &mut rows, &mut shown);
        v.rows = Rc::new(rows);
        v.shown = shown;
        v.finish(store, filter);
        v
    }

    /// Take the messages appended since the view was made.
    pub fn extend(&mut self, store: &Store, filter: &Filter, uid: u32) {
        if store.entries.len() <= self.seen {
            return;
        }
        let mut rows = std::mem::take(&mut self.rows);
        let mut shown = std::mem::take(&mut self.shown);
        self.take(store, filter, uid, Rc::make_mut(&mut rows), &mut shown);
        self.rows = rows;
        self.shown = shown;
        self.finish(store, filter);
    }

    fn take(&mut self, store: &Store, filter: &Filter, uid: u32, rows: &mut Vec<u32>, shown: &mut Shown) {
        let range = filter.range();
        let mut names = crate::search::Names::default();
        for (i, e) in store.entries.iter().enumerate().skip(self.seen) {
            for (k, s) in Scope::JOURNAL.iter().enumerate() {
                if s.matches(e, uid) {
                    self.scopes[k] += 1;
                }
            }
            if !filter.matches_but_sources_in(e, uid, &mut names) {
                continue;
            }
            let in_range = range.is_none_or(|(a, b)| e.time >= a && e.time <= b);
            if in_range {
                let at = match self.source_at.get(&address(&e.source)) {
                    Some(&at) => at,
                    None => {
                        let at = match self.source_named.get(&e.source) {
                            Some(&at) => at,
                            None => {
                                self.counted.push(SourceCount {
                                    name: e.source.clone(),
                                    count: 0,
                                    problems: 0,
                                    errors: 0,
                                });
                                self.source_named.insert(e.source.clone(), self.counted.len() - 1);
                                self.counted.len() - 1
                            }
                        };
                        self.source_at.insert(address(&e.source), at);
                        at
                    }
                };
                self.counted[at].count += 1;
                if e.priority.is_problem() {
                    self.counted[at].problems += 1;
                }
                if e.priority.is_error() {
                    self.counted[at].errors += 1;
                }
            }
            if !filter.sources_match(e) {
                continue;
            }
            let kind = if e.priority <= Priority::Error {
                0
            } else if e.priority == Priority::Warning {
                1
            } else {
                2
            };
            self.charted.push((e.time, kind));
            if in_range {
                rows.push(i as u32);
                shown.push(e.clone());
                if e.priority <= Priority::Error {
                    self.errors += 1;
                    *self.notable.errors_by_source.entry(e.source.clone()).or_default() += 1;
                } else if e.priority == Priority::Warning {
                    self.warnings += 1;
                }
                self.note(e, i);
            }
        }
        self.seen = store.entries.len();
    }

    fn note(&mut self, e: &Entry, i: usize) {
        let n = &mut self.notable;
        if e.message_id.as_deref() == Some(crate::filter::UNIT_FAILED)
            && let Some(u) = &e.about
            && !n.failed.contains(u)
        {
            n.failed.push(u.clone());
        }
        if e.flags & crate::entry::CRASH != 0 {
            n.crashes += 1;
            if let Some(name) = crate::filter::crashed(e)
                && !n.crashed.contains(&name)
            {
                n.crashed.push(name);
            }
        }
        if e.transport == crate::entry::Transport::Kernel && e.message.starts_with("Out of memory: Killed process") {
            n.oom += 1;
            n.last_oom = Some(i as u32);
        }
    }

    fn finish(&mut self, store: &Store, _filter: &Filter) {
        let mut sources = self.counted.clone();
        sources.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        self.sources = Rc::new(sources);
        let (Some(from), Some(to)) = (store.first_time(), store.last_time()) else {
            self.histogram = Arc::new(Histogram::default());
            return;
        };
        let mut h = Histogram { from, to: to.max(from + 1_000_000), buckets: vec![[0; 3]; BUCKETS] };
        for &(t, k) in &self.charted {
            if t > 0 {
                let b = h.bucket_of(t);
                h.buckets[b][usize::from(k)] += 1;
            }
        }
        self.histogram = Arc::new(h);
    }

    /// The row a message shows in.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        self.rows.binary_search(&(index as u32)).ok()
    }

    pub fn scope_count(&self, s: &Scope) -> Option<usize> {
        Scope::JOURNAL.iter().position(|x| x == s).map(|k| self.scopes[k])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Chip;
    use crate::search::Search;

    /// What arrives live, taken in, is what reading it all at once gives.
    #[test]
    fn appending_is_reading_again() {
        let all = crate::fixture::journal();
        let clock = crate::fixture::clock();
        let filters = [
            Filter::default(),
            Filter { scope: Scope::Problems, ..Filter::default() },
            Filter { chips: vec![Chip::Source { name: "sshd".into(), not: false }], ..Filter::default() },
            Filter { search: Search::parse("wlp14s0 -dhcp", &clock), ..Filter::default() },
        ];
        for filter in filters {
            let (first, rest) = all.split_at(400);
            let mut live = Store::default();
            live.prepend(first.iter().rev().cloned().collect());
            let mut view = View::make(&live, &filter, crate::fixture::UID);
            for chunk in rest.chunks(37) {
                live.append(chunk.to_vec());
                view.extend(&live, &filter, crate::fixture::UID);
            }
            let mut whole = Store::default();
            whole.append(all.clone());
            let again = View::make(&whole, &filter, crate::fixture::UID);
            assert_eq!(view.rows, again.rows);
            assert_eq!((view.errors, view.warnings, view.scopes), (again.errors, again.warnings, again.scopes));
            assert_eq!(view.sources, again.sources);
            assert_eq!(view.histogram, again.histogram);
            // Seqs keep the order the messages are in.
            assert!(live.entries.windows(2).all(|w| w[0].seq < w[1].seq));
            assert_eq!(live.index_of(live.entries[123].seq), Some(123));
        }
    }
}
