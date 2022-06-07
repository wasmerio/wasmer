#[cfg(feature = "core")]
use core::hash::Hash;
use indexmap::IndexMap;
use rkyv::{Archive, Deserialize, Serialize};
#[cfg(feature = "std")]
use std::hash::Hash;

#[derive(Serialize, Deserialize, Archive)]
/// Rkyv Archivable IndexMap
pub struct ArchivableIndexMap<K: Hash + Ord + Archive, V: Archive> {
    entries: Vec<(K, V)>,
}

impl<K: Hash + Ord + Archive + Clone, V: Archive> From<IndexMap<K, V>>
    for ArchivableIndexMap<K, V>
{
    fn from(it: IndexMap<K, V>) -> Self {
        let entries = it.into_iter().collect();
        Self { entries }
    }
}

impl<K: Hash + Ord + Archive + Clone, V: Archive> From<ArchivableIndexMap<K, V>>
    for IndexMap<K, V>
{
    fn from(other: ArchivableIndexMap<K, V>) -> Self {
        other.entries.into_iter().collect()
    }
}
