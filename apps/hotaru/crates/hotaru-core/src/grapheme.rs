//! Sparse row-local clusters with cached heap ownership charges.
use compact_str::CompactString;

fn heap(text: &CompactString) -> usize {
    if text.is_heap_allocated() {
        crate::memory::allocation(text.capacity())
    } else {
        0
    }
}
#[derive(Debug, Default)]
pub(crate) struct GraphemeMap(Vec<(usize, CompactString)>, usize);
impl Clone for GraphemeMap {
    fn clone(&self) -> Self {
        let mut entries = Vec::with_capacity(self.0.capacity());
        entries.extend(self.0.iter().map(|(col, text)| {
            let mut copy = CompactString::with_capacity(text.capacity());
            copy.push_str(text);
            (*col, copy)
        }));
        Self(entries, self.1)
    }
}
impl GraphemeMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity), 0)
    }
    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }
    pub fn owned_bytes(&self) -> usize {
        crate::memory::vector::<(usize, CompactString)>(self.0.capacity()) + self.1
    }
    pub fn archive_iter(&self) -> impl Iterator<Item = (usize, &CompactString)> {
        self.0.iter().map(|(col, text)| (*col, text))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn clear(&mut self) {
        self.0.clear();
        self.1 = 0;
    }
    fn position(&self, col: usize) -> Result<usize, usize> {
        self.0.binary_search_by_key(&col, |(column, _)| *column)
    }
    pub fn get(&self, col: &usize) -> Option<&CompactString> {
        self.position(*col).ok().map(|i| &self.0[i].1)
    }
    pub fn contains_key(&self, col: &usize) -> bool {
        self.position(*col).is_ok()
    }
    pub fn modify(
        &mut self,
        col: usize,
        create: impl FnOnce() -> CompactString,
        change: impl FnOnce(&mut CompactString),
    ) {
        let index = match self.position(col) {
            Ok(i) => i,
            Err(i) => {
                let text = create();
                self.1 += heap(&text);
                self.0.insert(i, (col, text));
                i
            }
        };
        let text = &mut self.0[index].1;
        let before = heap(text);
        change(text);
        self.1 = self.1 - before + heap(text);
    }
    pub fn insert(&mut self, col: usize, text: CompactString) {
        let charge = heap(&text);
        match self.position(col) {
            Ok(i) => {
                self.1 -= heap(&self.0[i].1);
                self.0[i].1 = text;
            }
            Err(i) => self.0.insert(i, (col, text)),
        }
        self.1 += charge;
    }
    pub fn remove(&mut self, col: &usize) {
        if let Ok(i) = self.position(*col) {
            self.1 -= heap(&self.0.remove(i).1);
        }
    }
    /// Returns whether retained heap charges changed. Removing inline text
    /// preserves the vector capacity and does not change its ownership charge.
    pub fn retain(&mut self, mut keep: impl FnMut(&usize, &CompactString) -> bool) -> bool {
        let mut removed = 0;
        self.0.retain(|(col, text)| {
            let yes = keep(col, text);
            if !yes {
                removed += heap(text);
            }
            yes
        });
        self.1 -= removed;
        removed != 0
    }
    pub fn shift_right(&mut self, col: usize, count: usize, len: usize) {
        let mut removed = 0;
        self.0.retain_mut(|(at, text)| {
            if *at < col {
                true
            } else if *at + count < len {
                *at += count;
                true
            } else {
                removed += heap(text);
                false
            }
        });
        self.1 -= removed;
    }
    pub fn shift_left(&mut self, col: usize, count: usize) {
        let mut removed = 0;
        self.0.retain_mut(|(at, text)| {
            if *at < col {
                true
            } else if *at >= col + count {
                *at -= count;
                true
            } else {
                removed += heap(text);
                false
            }
        });
        self.1 -= removed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn sparse_edits_match_ordered_map() {
        let mut actual = GraphemeMap::new();
        let mut expected = BTreeMap::<usize, CompactString>::new();
        let mut seed = 0x734b_1902u32;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let col = seed as usize % 80;
            let count = (seed as usize >> 10) % (81 - col);
            match seed % 6 {
                0 => {
                    let text = CompactString::new("e\u{301}👩🏽‍💻");
                    actual.insert(col, text.clone());
                    expected.insert(col, text);
                }
                1 => {
                    actual.remove(&col);
                    expected.remove(&col);
                }
                2 => {
                    actual.shift_right(col, count, 80);
                    expected = expected
                        .into_iter()
                        .filter_map(|(at, text)| {
                            if at < col {
                                Some((at, text))
                            } else if at + count < 80 {
                                Some((at + count, text))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
                3 => {
                    actual.shift_left(col, count);
                    expected = expected
                        .into_iter()
                        .filter_map(|(at, text)| {
                            if at < col {
                                Some((at, text))
                            } else if at >= col + count {
                                Some((at - count, text))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
                4 => {
                    actual.retain(|&at, _| at < col || at >= col + count);
                    expected.retain(|&at, _| at < col || at >= col + count);
                }
                _ => {
                    actual.modify(col, || "a".into(), |text| text.push('\u{301}'));
                    expected
                        .entry(col)
                        .or_insert_with(|| "a".into())
                        .push('\u{301}');
                }
            }
            assert_eq!(
                actual.0,
                expected
                    .iter()
                    .map(|(&at, text)| (at, text.clone()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                actual.1,
                actual.0.iter().map(|(_, text)| heap(text)).sum::<usize>()
            );
            for at in 0..80 {
                assert_eq!(actual.get(&at), expected.get(&at));
                assert_eq!(actual.contains_key(&at), expected.contains_key(&at));
            }
        }
    }
}
