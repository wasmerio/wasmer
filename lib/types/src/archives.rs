#[cfg(feature = "core")]
use core::hash::Hash;
use indexmap::IndexMap;
use rkyv::{Archive, Deserialize, Serialize};
#[cfg(feature = "std")]
use std::{collections::HashMap, hash::Hash};

#[derive(Serialize, Deserialize, Archive)]
/// Rkyv Archivable IndexMap
pub struct ArchivableIndexMap<K: Hash + Eq + Archive, V: Archive> {
    indices: HashMap<K, u64>,
    entries: Vec<(K, V)>,
}

impl<K: Hash + Eq + Archive + Clone, V: Archive> From<IndexMap<K, V>> for ArchivableIndexMap<K, V> {
    fn from(it: IndexMap<K, V>) -> ArchivableIndexMap<K, V> {
        let mut r = ArchivableIndexMap {
            indices: HashMap::new(),
            entries: Vec::new(),
        };
        let mut i: u64 = 0;
        for (k, v) in it.into_iter() {
            r.indices.insert(k.clone(), i);
            r.entries.push((k, v));
            i += 1;
        }
        r
    }
}

impl<K: Hash + Eq + Archive + Clone, V: Archive> Into<IndexMap<K, V>> for ArchivableIndexMap<K, V> {
    fn into(self) -> IndexMap<K, V> {
        let mut r = IndexMap::new();
        for (k, v) in self.entries.into_iter() {
            r.insert(k, v);
        }
        r
    }
}
