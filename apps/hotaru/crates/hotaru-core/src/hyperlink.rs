//! OSC 8 identities and sparse, row-owned hyperlink ranges.
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkIdentity {
    Explicit(Box<[u8]>),
    Implicit(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    pub id: LinkIdentity,
    pub uri: Box<[u8]>,
}

pub type LinkRef = triomphe::Arc<Hyperlink>;

#[derive(Clone, Copy, Debug)]
struct Run {
    start: u16,
    end: u16,
    link: u16,
}

#[derive(Debug, Default)]
pub(crate) struct RowLinks {
    runs: Vec<Run>,
    links: Vec<LinkRef>,
    payload_bytes: usize,
}
impl Clone for RowLinks {
    fn clone(&self) -> Self {
        Self {
            runs: crate::memory::clone_vec(&self.runs),
            links: crate::memory::clone_vec(&self.links),
            payload_bytes: self.payload_bytes,
        }
    }
}
impl Hyperlink {
    pub(crate) fn owned_bytes(&self) -> usize {
        crate::memory::allocation(std::mem::size_of::<Self>() + std::mem::size_of::<usize>())
            + crate::memory::allocation(self.uri.len())
            + match &self.id {
                LinkIdentity::Explicit(id) => crate::memory::allocation(id.len()),
                LinkIdentity::Implicit(_) => 0,
            }
    }
}

impl RowLinks {
    pub fn owned_bytes(&self) -> usize {
        Self::allocation_bytes(self.runs.capacity(), self.links.capacity()) + self.payload_bytes
    }
    pub fn allocation_bytes(runs: usize, links: usize) -> usize {
        crate::memory::allocation(std::mem::size_of::<Self>())
            + crate::memory::vector::<Run>(runs)
            + crate::memory::vector::<LinkRef>(links)
    }
    pub fn archive_table(&self) -> &[LinkRef] {
        &self.links
    }
    pub fn archive_runs(&self) -> impl Iterator<Item = (u16, u16, u16)> + '_ {
        self.runs.iter().map(|run| (run.start, run.end, run.link))
    }
    pub fn archive_capacities(&self) -> (usize, usize) {
        (self.runs.capacity(), self.links.capacity())
    }
    pub fn restore_archive(
        table: Vec<LinkRef>,
        runs: Vec<(u16, u16, u16)>,
        run_capacity: usize,
        cols: usize,
    ) -> Option<Self> {
        let mut restored = Vec::with_capacity(run_capacity);
        let mut previous = 0;
        for (start, end, link) in runs {
            if usize::from(start) < previous
                || start >= end
                || usize::from(end) > cols
                || usize::from(link) >= table.len()
            {
                return None;
            }
            previous = usize::from(end);
            restored.push(Run { start, end, link });
        }
        let payload_bytes = table.iter().map(|link| link.owned_bytes()).sum();
        Some(Self {
            runs: restored,
            links: table,
            payload_bytes,
        })
    }
    #[cfg(test)]
    pub fn from_ordered_runs(runs: Vec<(Range<usize>, LinkRef)>, cols: usize) -> Option<Self> {
        use std::collections::HashMap;
        let mut result = Self::default();
        let mut ids = HashMap::new();
        let mut previous = 0;
        for (range, link) in runs {
            if range.is_empty()
                || range.start < previous
                || range.end > cols
                || range.end > u16::MAX as usize
            {
                return None;
            }
            previous = range.end;
            let id = if let Some(&id) = ids.get(&link) {
                id
            } else {
                let id = u16::try_from(result.links.len()).ok()?;
                ids.insert(link.clone(), id);
                result.payload_bytes += link.owned_bytes();
                result.links.push(link);
                id
            };
            result.runs.push(Run {
                start: range.start as u16,
                end: range.end as u16,
                link: id,
            });
        }
        result.coalesce();
        Some(result)
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn get(&self, col: usize) -> Option<&LinkRef> {
        let index = self.runs.partition_point(|run| usize::from(run.end) <= col);
        let run = self.runs.get(index)?;
        (usize::from(run.start) <= col).then(|| &self.links[usize::from(run.link)])
    }

    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = (Range<usize>, &LinkRef)> {
        self.runs.iter().map(|run| {
            (
                usize::from(run.start)..usize::from(run.end),
                &self.links[usize::from(run.link)],
            )
        })
    }

    fn id(&mut self, link: &LinkRef, cols: usize) -> u16 {
        if let Some(index) = self
            .links
            .iter()
            .rposition(|old| LinkRef::ptr_eq(old, link) || **old == **link)
        {
            return index as u16;
        }
        if self.links.len() > cols * 2 {
            let mut ids = vec![u16::MAX; self.links.len()];
            let mut links = Vec::with_capacity(self.runs.len() + 1);
            for run in &mut self.runs {
                let old = usize::from(run.link);
                if ids[old] == u16::MAX {
                    ids[old] = links.len() as u16;
                    links.push(self.links[old].clone());
                }
                run.link = ids[old];
            }
            self.links = links;
            self.payload_bytes = self.links.iter().map(|link| link.owned_bytes()).sum();
        }
        let id = self.links.len() as u16;
        self.payload_bytes += link.owned_bytes();
        self.links.push(link.clone());
        id
    }

    /// Replace just this range. Existing runs outside it retain their identity.
    pub fn set(&mut self, range: Range<usize>, link: Option<&LinkRef>, cols: usize) {
        if range.is_empty() {
            return;
        }
        let link = link.map(|value| self.id(value, cols));
        let start = range.start as u16;
        let end = range.end as u16;
        // Printing a continuous link is normally a single append/extension.
        if let Some(last) = self.runs.last_mut() {
            if last.end <= start {
                if last.end == start && Some(last.link) == link {
                    last.end = end;
                } else if let Some(link) = link {
                    self.runs.push(Run { start, end, link });
                }
                return;
            }
        } else {
            if let Some(link) = link {
                self.runs.push(Run { start, end, link });
            }
            return;
        }
        let first = self.runs.partition_point(|run| run.end <= start);
        let last = self.runs.partition_point(|run| run.start < end);
        let left = self
            .runs
            .get(first)
            .copied()
            .filter(|run| run.start < start)
            .map(|run| Run { end: start, ..run });
        let right = last
            .checked_sub(1)
            .and_then(|index| self.runs.get(index))
            .copied()
            .filter(|run| run.end > end)
            .map(|run| Run { start: end, ..run });
        self.runs.splice(
            first..last,
            left.into_iter()
                .chain(link.map(|link| Run { start, end, link }))
                .chain(right),
        );
        self.coalesce();
    }

    fn coalesce(&mut self) {
        let mut out = 0;
        for index in 0..self.runs.len() {
            let run = self.runs[index];
            if out > 0 && self.runs[out - 1].end == run.start && self.runs[out - 1].link == run.link
            {
                self.runs[out - 1].end = run.end;
            } else {
                self.runs[out] = run;
                out += 1;
            }
        }
        self.runs.truncate(out);
    }

    pub fn shift_right(&mut self, col: usize, count: usize, cols: usize) {
        let mut runs = Vec::with_capacity(self.runs.len() + 1);
        for run in self.runs.iter().copied() {
            if usize::from(run.start) < col {
                runs.push(Run {
                    end: run.end.min(col as u16),
                    ..run
                });
            }
            let start = usize::from(run.start).max(col) + count;
            let end = (usize::from(run.end) + count).min(cols);
            if usize::from(run.end) > col && start < end {
                runs.push(Run {
                    start: start as u16,
                    end: end as u16,
                    ..run
                });
            }
        }
        self.runs = runs;
        self.coalesce();
    }

    pub fn shift_left(&mut self, col: usize, count: usize) {
        let mut runs = Vec::with_capacity(self.runs.len() + 1);
        for run in self.runs.iter().copied() {
            if usize::from(run.start) < col {
                runs.push(Run {
                    end: run.end.min(col as u16),
                    ..run
                });
            }
            let start = usize::from(run.start).max(col + count);
            if usize::from(run.end) > start {
                runs.push(Run {
                    start: (start - count) as u16,
                    end: run.end - count as u16,
                    ..run
                });
            }
        }
        self.runs = runs;
        self.coalesce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_ranges_match_dense_edits_and_survive_table_compaction() {
        const COLS: usize = 61;
        let mut ranges = RowLinks::default();
        let mut dense: Vec<Option<LinkRef>> = vec![None; COLS];
        let mut seed = 0x84823874u64;
        for step in 0..20_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let col = ((seed >> 16) as usize) % COLS;
            let count = 1 + ((seed >> 32) as usize) % (COLS - col);
            match seed % 4 {
                0 => {
                    ranges.shift_left(col, count);
                    dense[col..].rotate_left(count);
                    dense[COLS - count..].fill(None);
                }
                1 => {
                    ranges.shift_right(col, count, COLS);
                    dense[col..].rotate_right(count);
                    dense[col..col + count].fill(None);
                }
                _ => {
                    let link = (!seed.is_multiple_of(7)).then(|| {
                        LinkRef::new(Hyperlink {
                            id: LinkIdentity::Implicit(step),
                            uri: format!("uri{step}").into_bytes().into_boxed_slice(),
                        })
                    });
                    ranges.set(col..col + count, link.as_ref(), COLS);
                    dense[col..col + count].fill(link);
                }
            }
            for (col, expected) in dense.iter().enumerate() {
                assert_eq!(ranges.get(col), expected.as_ref(), "step{step} col{col}");
            }
            let runs = ranges
                .iter()
                .map(|(range, link)| (range, link.clone()))
                .collect();
            let restored = RowLinks::from_ordered_runs(runs, COLS).unwrap();
            for (col, expected) in dense.iter().enumerate() {
                assert_eq!(restored.get(col), expected.as_ref());
            }
            assert!(ranges.links.len() <= COLS * 2 + 1);
            assert_eq!(
                ranges.payload_bytes,
                ranges
                    .links
                    .iter()
                    .map(|link| link.owned_bytes())
                    .sum::<usize>()
            );
        }
    }
}
