//! Sparse row-local clusters. The usual few combining sequences fit in inline
//! strings, and clearing/repainting a row retains the entry allocation.

use compact_str::CompactString;

#[derive(Clone, Debug, Default)]
pub(crate) struct GraphemeMap(Vec<(usize, CompactString)>);

impl GraphemeMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = (usize, &str)> {
        self.0.iter().map(|(col, text)| (*col, text.as_str()))
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    fn position(&self, col: usize) -> Result<usize, usize> {
        self.0.binary_search_by_key(&col, |(column, _)| *column)
    }
    pub fn get(&self, col: &usize) -> Option<&CompactString> {
        self.position(*col).ok().map(|index| &self.0[index].1)
    }
    pub fn contains_key(&self, col: &usize) -> bool {
        self.position(*col).is_ok()
    }
    pub fn get_or_insert_with(
        &mut self,
        col: usize,
        create: impl FnOnce() -> CompactString,
    ) -> &mut CompactString {
        let index = match self.position(col) {
            Ok(index) => index,
            Err(index) => {
                self.0.insert(index, (col, create()));
                index
            }
        };
        &mut self.0[index].1
    }
    pub fn insert(&mut self, col: usize, text: CompactString) {
        match self.position(col) {
            Ok(index) => self.0[index].1 = text,
            Err(index) => self.0.insert(index, (col, text)),
        }
    }
    pub fn remove(&mut self, col: &usize) {
        if let Ok(index) = self.position(*col) {
            self.0.remove(index);
        }
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&usize, &CompactString) -> bool) {
        self.0.retain(|(col, text)| keep(col, text));
    }
    pub fn shift_right(&mut self, col: usize, count: usize, len: usize) {
        self.0.retain_mut(|(at, _)| {
            if *at < col {
                true
            } else if *at + count < len {
                *at += count;
                true
            } else {
                false
            }
        });
    }
    pub fn shift_left(&mut self, col: usize, count: usize) {
        self.0.retain_mut(|(at, _)| {
            if *at < col {
                true
            } else if *at >= col + count {
                *at -= count;
                true
            } else {
                false
            }
        });
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
                    actual
                        .get_or_insert_with(col, || "a".into())
                        .push('\u{301}');
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
            for at in 0..80 {
                assert_eq!(actual.get(&at), expected.get(&at));
                assert_eq!(actual.contains_key(&at), expected.contains_key(&at));
            }
        }
    }
}
