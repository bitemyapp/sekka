//! The graphs' history: the last two minutes of the system's figures, in
//! fixed rings that never grow.

use crate::model::Frame;

/// How many readings a graph shows.
pub const LEN: usize = 120;

/// The last [`LEN`] values of one series, oldest first when read.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    values: [f32; LEN],
    head: usize,
    len: usize,
}

impl Default for Ring {
    fn default() -> Ring {
        Ring { values: [0.0; LEN], head: 0, len: 0 }
    }
}

impl std::hash::Hash for Ring {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.head.hash(h);
        self.len.hash(h);
        if let Some(v) = self.last() {
            v.to_bits().hash(h);
        }
    }
}

impl Ring {
    pub fn push(&mut self, v: f32) {
        self.values[self.head] = if v.is_finite() { v } else { 0.0 };
        self.head = (self.head + 1) % LEN;
        self.len = (self.len + 1).min(LEN);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The `i`th value, oldest first.
    pub fn get(&self, i: usize) -> f32 {
        let start = (self.head + LEN - self.len) % LEN;
        self.values[(start + i) % LEN]
    }

    pub fn last(&self) -> Option<f32> {
        (self.len > 0).then(|| self.values[(self.head + LEN - 1) % LEN])
    }

    pub fn max(&self) -> f32 {
        (0..self.len).map(|i| self.get(i)).fold(0.0, f32::max)
    }
}

/// A graphics card's series, by its PCI address: fractions of its engines'
/// time and of its memory.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct CardHistory {
    pub pci: String,
    pub busy: Ring,
    pub memory: Ring,
}

/// Every graph's series.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct History {
    /// Percent of all processors: user, and system.
    pub cpu_user: Ring,
    pub cpu_system: Ring,
    /// Fractions of memory: used by programs, and cached.
    pub mem_used: Ring,
    pub mem_cached: Ring,
    /// Bytes a second.
    pub disk_read: Ring,
    pub disk_write: Ring,
    pub net_rx: Ring,
    pub net_tx: Ring,
    /// The cards present, in the reading's order.
    pub cards: Vec<CardHistory>,
}

impl History {
    /// The series of the card at `pci`.
    pub fn card(&self, pci: &str) -> Option<&CardHistory> {
        self.cards.iter().find(|c| c.pci == pci)
    }

    pub fn push(&mut self, f: &Frame) {
        let mut cards = Vec::with_capacity(f.gpus.len());
        for g in &f.gpus {
            let mut c = match self.cards.iter().position(|c| c.pci == g.pci) {
                Some(i) => self.cards.swap_remove(i),
                None => CardHistory { pci: g.pci.clone(), ..Default::default() },
            };
            c.busy.push(g.busy.unwrap_or(0.0) as f32 / 100.0);
            let memory = g.vram_used.zip(g.vram_total).filter(|(_, total)| *total > 0);
            c.memory.push(memory.map_or(0.0, |(used, total)| used as f32 / total as f32));
            cards.push(c);
        }
        self.cards = cards;
        let s = &f.system;
        self.cpu_user.push(s.user as f32);
        self.cpu_system.push(s.kernel as f32);
        let total = s.memory.total.max(1) as f32;
        self.mem_used.push(s.memory.used() as f32 / total);
        self.mem_cached.push((s.memory.cached + s.memory.buffers) as f32 / total);
        self.disk_read.push(s.disk_read as f32);
        self.disk_write.push(s.disk_write as f32);
        self.net_rx.push(s.net_rx as f32);
        self.net_tx.push(s.net_tx as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_keeps_the_last_values_in_order() {
        let mut r = Ring::default();
        assert_eq!(r.last(), None);
        for i in 0..(LEN + 5) {
            r.push(i as f32);
        }
        assert_eq!(r.len(), LEN);
        assert_eq!(r.get(0), 5.0);
        assert_eq!(r.get(LEN - 1), (LEN + 4) as f32);
        assert_eq!(r.max(), (LEN + 4) as f32);
        r.push(f32::NAN);
        assert_eq!(r.last(), Some(0.0), "nothing odd is drawn");
    }

    #[test]
    fn a_card_keeps_its_history_when_the_cards_are_numbered_again() {
        let card = |pci: &str, busy| crate::model::Gpu {
            pci: pci.into(),
            busy: Some(busy),
            vram_used: Some(8 << 30),
            vram_total: Some(32 << 30),
            ..Default::default()
        };
        let mut f = Frame { gpus: vec![card("0000:78:00.0", 10.0), card("0000:01:00.0", 60.0)], ..Default::default() };
        let mut h = History::default();
        h.push(&f);
        f.gpus.reverse();
        h.push(&f);
        let nvidia = h.card("0000:01:00.0").unwrap();
        assert_eq!((nvidia.busy.len(), nvidia.busy.last(), nvidia.memory.last()), (2, Some(0.6), Some(0.25)));
        f.gpus.pop();
        h.push(&f);
        assert!(h.card("0000:78:00.0").is_none(), "a card that went away is forgotten");
    }
}
