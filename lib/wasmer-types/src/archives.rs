use cranelift_entity::{EntityRef, PrimaryMap};
use indexmap::IndexMap;

use rkyv::{
    offset_of,
    ser::Serializer,
    std_impl::chd::{ArchivedHashMap, ArchivedHashMapResolver},
    std_impl::{ArchivedVec, VecResolver},
    Archive, Archived, ArchivedUsize, Deserialize, DeserializeUnsized, Fallible, MetadataResolver,
    RawRelPtr, Serialize,
};

#[cfg(feature = "core")]
use core::{
    borrow::Borrow,
    cmp::Reverse,
    hash::{Hash, Hasher},
    iter::FusedIterator,
    marker::PhantomData,
    mem::size_of,
    ops::Index,
    pin::Pin,
    slice,
};

#[cfg(feature = "std")]
use std::{
    borrow::Borrow,
    cmp::Reverse,
    hash::{Hash, Hasher},
    iter::FusedIterator,
    marker::PhantomData,
    mem::size_of,
    ops::Index,
    pin::Pin,
    slice,
};

/// Archivable PrimaryMap, stores same information as a PrimaryMap, but
/// in crate so we can implement rkyv::Archive, etc. traits on it
pub struct ArchivablePrimaryMap<K, V>
    where
        K: EntityRef,
{
    elems: Vec<V>,
    unused: PhantomData<K>,
}

/// PrimaryMap after archive
pub struct ArchivedPrimaryMap<K: EntityRef, V>(ArchivedVec<V>, PhantomData<K>);

impl<K: Archive + EntityRef, V: Archive> Archive for ArchivablePrimaryMap<K, V>
    where
        K::Archived: EntityRef,
{
    type Archived = ArchivedPrimaryMap<K::Archived, V::Archived>;
    type Resolver = VecResolver<MetadataResolver<[V]>>;

    fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
        #[allow(clippy::unit_arg)]
        unsafe {
            ArchivedPrimaryMap(
                Vec::resolve(
                    &self.elems,
                    pos,
                    resolver,
                ),
                PhantomData,
            )
        }
    }
}

impl<K: Serialize<S> + EntityRef, V: Serialize<S>, S: Serializer + ?Sized> Serialize<S>
for ArchivablePrimaryMap<K, V>
    where
        K::Archived: EntityRef,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        self.elems
            .serialize(serializer)
    }
}

impl<K: Archive + EntityRef, V: Archive, D: Fallible + ?Sized> Deserialize<ArchivablePrimaryMap<K, V>, D>
for Archived<ArchivablePrimaryMap<K, V>>
    where
        K::Archived: Deserialize<K, D> + EntityRef,
        V::Archived: Deserialize<V, D>,
        [V::Archived]: DeserializeUnsized<[V], D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<ArchivablePrimaryMap<K, V>, D::Error> {
        let elems: Vec<_> = self.0.deserialize(deserializer)?;
        Ok(ArchivablePrimaryMap {
            elems,
            unused: PhantomData,
        })
    }
}

pub struct ArchiableIndexMap<K: Hash + Eq, V>

pub struct ArchivedIndexMap<K: Hash + Eq, V>(ArchivedVec<(K, V)>);

impl<K: Archive + Hash + Eq, V: Archive> Archive for IndexMap<K, V>
    where
        K::Archived: Hash + Eq,
{
    type Archived = ArchivedHashMap<K::Archived, V::Archived>;
    type Resolver = ArchivedHashMapResolver;

    fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
        resolver.resolve_from_len(pos, self.len())
    }
}

impl<K: Serialize<S> + Hash + Eq, V: Serialize<S>, S: Serializer + ?Sized> Serialize<S>
for IndexMap<K, V>
    where
        K::Archived: Hash + Eq,
{
    // TODO: serialize it as vec is inefficient for access archive (doesn't affect perf of deserialize)
    // If going to access archive without deserialization, we should implment ArchivedIndexMap
    // which had same interface as IndexMap, but implemented similar as rkyv ArchivedHashMap
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok(ArchivedHashMap::serialize_from_iter(
            self.iter(),
            self.len(),
            serializer,
        )?)
    }
}

impl<K: Archive + Hash + Eq, V: Archive, D: Fallible + ?Sized> Deserialize<IndexMap<K, V>, D>
for Archived<IndexMap<K, V>>
    where
        K::Archived: Deserialize<K, D> + Hash + Eq,
        V::Archived: Deserialize<V, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<IndexMap<K, V>, D::Error> {
        let mut result = IndexMap::new();
        for (k, v) in self.iter() {
            result.insert(k.deserialize(deserializer)?, v.deserialize(deserializer)?);
        }
        Ok(result)
    }
}
