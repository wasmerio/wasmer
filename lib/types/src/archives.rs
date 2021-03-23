use crate::entity::{EntityRef, PrimaryMap};
use indexmap::IndexMap;

use rkyv::{
    offset_of,
    ser::Serializer,
    std_impl::{ArchivedVec, VecResolver},
    Archive, Archived, Deserialize, DeserializeUnsized, Fallible, MetadataResolver, Serialize,
};

#[cfg(feature = "core")]
use core::{hash::Hash, marker::PhantomData};

#[cfg(feature = "std")]
use std::{collections::HashMap, hash::Hash, marker::PhantomData};

/// PrimaryMap after archive
pub struct ArchivedPrimaryMap<K: EntityRef, V>(ArchivedVec<V>, PhantomData<K>);

impl<K: Archive + EntityRef, V: Archive> Archive for PrimaryMap<K, V>
where
    K::Archived: EntityRef,
{
    type Archived = ArchivedPrimaryMap<K::Archived, V::Archived>;
    type Resolver = VecResolver<MetadataResolver<[V]>>;

    fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
        ArchivedPrimaryMap(Vec::resolve(&self.elems, pos, resolver), PhantomData)
    }
}

impl<K: Serialize<S> + EntityRef, V: Serialize<S>, S: Serializer + ?Sized> Serialize<S>
    for PrimaryMap<K, V>
where
    K::Archived: EntityRef,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        self.elems.serialize(serializer)
    }
}

impl<K: Archive + EntityRef, V: Archive, D: Fallible + ?Sized> Deserialize<PrimaryMap<K, V>, D>
    for Archived<PrimaryMap<K, V>>
where
    K::Archived: Deserialize<K, D> + EntityRef,
    V::Archived: Deserialize<V, D>,
    [V::Archived]: DeserializeUnsized<[V], D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<PrimaryMap<K, V>, D::Error> {
        let elems: Vec<_> = self.0.deserialize(deserializer)?;
        Ok(PrimaryMap {
            elems,
            unused: PhantomData,
        })
    }
}

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
