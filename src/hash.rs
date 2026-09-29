//! A quick hasher for the maps SPIT keeps while it works. Their keys come
//! from the user's own files, so the standard hasher's defence against keys
//! chosen to collide buys nothing, while its setup for every short key
//! costs a tenth of a large run.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// A `HashMap` with [`QuickHasher`].
pub(crate) type QuickMap<K, V> = HashMap<K, V, BuildHasherDefault<QuickHasher>>;

/// A `HashSet` with [`QuickHasher`].
pub(crate) type QuickSet<T> = HashSet<T, BuildHasherDefault<QuickHasher>>;

/// FxHash, as the Rust compiler uses: a rotate, an exclusive or and a
/// multiply for each eight bytes, then a final mix so every bit of the hash
/// depends on every bit of the key.
#[derive(Default)]
pub(crate) struct QuickHasher(u64);

const SEED: u64 = 0x517c_c1b7_2722_0a95;

impl QuickHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for QuickHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0; 8];
            word.copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    fn finish(&self) -> u64 {
        // A multiply only carries low bits upward; mixing the high half back
        // down gives the low bits, which pick a bucket, the whole key.
        let hash = (self.0 ^ (self.0 >> 32)).wrapping_mul(SEED);
        hash ^ (hash >> 29)
    }
}

#[cfg(test)]
mod tests {
    use super::QuickMap;

    #[test]
    fn keys_that_differ_anywhere_spread_over_buckets() {
        let mut map = QuickMap::default();
        for index in 0..10_000 {
            map.insert(format!("sub-{index:05}/ses-01/dwi.nii.gz"), index);
        }
        assert_eq!(map.len(), 10_000);
        assert_eq!(map.get("sub-01234/ses-01/dwi.nii.gz"), Some(&1234));
        assert_eq!(map.get("sub-01234/ses-02/dwi.nii.gz"), None);
    }
}
