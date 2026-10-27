//! A fast, deterministic hasher (FxHash's): for node ids, paint hashes and
//! cache keys, where hashing is on every frame's path and nothing is
//! adversarial.

use std::hash::{BuildHasherDefault, Hash, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Clone, Copy, Default)]
pub struct Fx(u64);

impl Fx {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for c in words {
            self.add(u64::from_le_bytes(*c));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word) ^ ((rest.len() as u64) << 56));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        // Mix the last word through, so close inputs land far apart.
        let mut x = self.0;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x
    }
}

pub type FxBuild = BuildHasherDefault<Fx>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
pub type FxHashSet<K> = std::collections::HashSet<K, FxBuild>;

/// The hash of one value.
pub fn hash_of(v: &(impl Hash + ?Sized)) -> u64 {
    let mut h = Fx::default();
    v.hash(&mut h);
    h.finish()
}

/// What a widget feeds its drawing into, for the diff: floats by their
/// bits, colours by their channels.
pub struct PaintHasher<'a>(pub(crate) &'a mut Fx);

impl PaintHasher<'_> {
    pub fn add(&mut self, v: &(impl Hash + ?Sized)) -> &mut Self {
        v.hash(self.0);
        self
    }

    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.0.write_u64(v.to_bits());
        self
    }

    pub fn color(&mut self, c: sidestep_ui::Color) -> &mut Self {
        for v in [c.r, c.g, c.b, c.a] {
            self.0.write_u32(v.to_bits());
        }
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.0.write(s.as_bytes());
        self.0.write_u8(0xff);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_and_spread() {
        assert_eq!(hash_of("abc"), hash_of("abc"));
        assert_ne!(hash_of("abc"), hash_of("abd"));
        assert_ne!(hash_of(&(1u64, 2u64)), hash_of(&(2u64, 1u64)));
    }
}
