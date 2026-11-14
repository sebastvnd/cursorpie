//! A small bounded cache with least-recently-used (LRU) eviction.
//!
//! [`Lru`] backs the icon texture cache ([`ui::icons`](crate::ui::icons)). It is plain Rust with
//! no macOS or egui types, so it is covered by ordinary unit tests.
//!
//! Eviction is **frame based**. Entries are only dropped when [`Lru::new_frame`] runs, and never
//! if they were used since the previous call. A screen that shows more entries than the capacity
//! (the icon picker, for example) therefore grows the cache for as long as it is open instead of
//! evicting and reloading the same entries on every frame.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

/// A cached value and the logical time it was last used.
struct Slot<V> {
    value: V,
    used: u64,
}

/// A map that forgets its least recently used entries when asked to (see the module docs).
pub struct Lru<K, V> {
    map: HashMap<K, Slot<V>>,
    /// Logical clock, advanced on every read and write.
    tick: u64,
    /// Value of `tick` at the last [`Lru::new_frame`]. Entries newer than this are protected.
    epoch: u64,
}

impl<K, V> Default for Lru<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            tick: 0,
            epoch: 0,
        }
    }
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The value for `key`, marking it as used.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|s| {
            s.used = tick;
            &s.value
        })
    }

    /// Whether `key` is cached. Does not count as a use.
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.contains_key(key)
    }

    /// Inserts or replaces `key` and marks it as used.
    pub fn insert(&mut self, key: K, value: V) {
        self.tick += 1;
        let used = self.tick;
        self.map.insert(key, Slot { value, used });
    }

    /// Removes one entry.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.remove(key).map(|s| s.value)
    }

    /// The values, in no particular order. Does not count as a use.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values().map(|s| &s.value)
    }

    /// Keeps only the entries for which `keep` returns true. Does not count as a use.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.map.retain(|k, s| keep(k, &s.value));
    }

    /// Drops everything and gives the memory of the table back.
    pub fn clear(&mut self) {
        self.map = HashMap::new();
    }

    /// Marks the start of a frame: evicts the least recently used entries until at most `cap`
    /// remain, skipping every entry that was used since the previous call.
    pub fn new_frame(&mut self, cap: usize) {
        if self.map.len() > cap {
            let epoch = self.epoch;
            let mut stale: Vec<(u64, K)> = self
                .map
                .iter()
                .filter(|(_, s)| s.used <= epoch)
                .map(|(k, s)| (s.used, k.clone()))
                .collect();
            stale.sort_unstable_by_key(|(used, _)| *used);
            for (_, k) in stale.into_iter().take(self.map.len() - cap) {
                self.map.remove(&k);
            }
        }
        self.epoch = self.tick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(n: usize) -> Lru<String, usize> {
        let mut c = Lru::new();
        for i in 0..n {
            c.insert(format!("k{i}"), i);
        }
        c
    }

    #[test]
    fn evicts_the_least_recently_used_first() {
        let mut c = filled(4);
        c.new_frame(10); // everything so far counts as "last frame"
        c.get("k0"); // k0 is now the freshest
        c.new_frame(10);
        c.new_frame(2);
        assert_eq!(c.len(), 2);
        assert!(c.get("k0").is_some(), "the touched entry survives");
        assert!(c.get("k3").is_some(), "the newest insert survives");
        assert!(c.get("k1").is_none());
    }

    #[test]
    fn entries_used_in_the_last_frame_are_never_evicted() {
        let mut c = filled(6);
        c.new_frame(2); // all six were used since the start: nothing may go
        assert_eq!(
            c.len(),
            6,
            "a big screen grows the cache instead of thrashing"
        );
        c.new_frame(2); // none were touched during the frame in between
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn a_partly_used_cache_is_trimmed_to_the_capacity() {
        let mut c = filled(6);
        c.new_frame(6);
        c.get("k4");
        c.get("k5");
        c.new_frame(4); // k4 and k5 were just used, so two of the four others go
        assert_eq!(c.len(), 4);
        assert!(
            c.get("k0").is_none() && c.get("k1").is_none(),
            "the oldest go first"
        );
        c.new_frame(2);
        assert_eq!(c.len(), 2);
        assert!(
            c.get("k4").is_some() && c.get("k5").is_some(),
            "the freshest stay"
        );
    }

    #[test]
    fn contains_does_not_count_as_a_use() {
        let mut c = filled(3);
        c.new_frame(10);
        assert!(c.contains("k1") && !c.contains("nope"));
        c.new_frame(1); // k1 was only asked about, never used: it is evictable
        assert_eq!(c.len(), 1);
        assert!(!c.contains("k0") && !c.contains("k1"));
    }

    #[test]
    fn remove_retain_and_clear() {
        let mut c = filled(5);
        assert_eq!(c.remove("k2"), Some(2));
        assert_eq!(c.remove("k2"), None);
        c.retain(|_, v| v % 2 == 0);
        assert_eq!(c.len(), 2);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn insert_replaces_and_refreshes() {
        let mut c = filled(2);
        c.new_frame(10);
        c.insert("k0".into(), 99);
        c.new_frame(1);
        assert_eq!(c.get("k0"), Some(&99));
        assert!(c.get("k1").is_none());
    }
}
