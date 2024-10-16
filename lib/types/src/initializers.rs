/*
 * ! Remove me once rkyv generates doc-comments for fields or generates an #[allow(missing_docs)]
 * on their own.
 */
#![allow(missing_docs)]

use crate::indexes::{FunctionIndex, GlobalIndex, MemoryIndex, TableIndex};
use crate::lib::std::boxed::Box;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// A WebAssembly table initializer.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(Clone, Debug, Hash, PartialEq, Eq, RkyvSerialize, RkyvDeserialize, Archive)]
#[rkyv(derive(Debug))]
pub struct TableInitializer {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: usize,
    /// The values to write into the table elements.
    pub elements: Box<[FunctionIndex]>,
}

/// A memory index and offset within that memory where a data initialization
/// should be performed.
#[derive(Clone, Debug, PartialEq, Eq, RkyvSerialize, RkyvDeserialize, Archive)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[rkyv(derive(Debug))]
pub struct DataInitializerLocation {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,

    /// Optionally a Global variable base to initialize at.
    pub base: Option<GlobalIndex>,

    /// A constant offset to initialize at.
    pub offset: usize,
}

/// Any struct that acts like a `DataInitializerLocation`.
#[allow(missing_docs)]
pub trait DataInitializerLocationLike {
    fn memory_index(&self) -> MemoryIndex;
    fn base(&self) -> Option<GlobalIndex>;
    fn offset(&self) -> usize;
}

impl DataInitializerLocationLike for &DataInitializerLocation {
    fn memory_index(&self) -> MemoryIndex {
        self.memory_index
    }

    fn base(&self) -> Option<GlobalIndex> {
        self.base
    }

    fn offset(&self) -> usize {
        self.offset
    }
}

impl DataInitializerLocationLike for &ArchivedDataInitializerLocation {
    fn memory_index(&self) -> MemoryIndex {
        MemoryIndex::from_u32(rkyv::deserialize::<_, ()>(&self.memory_index).unwrap().0)
    }

    fn base(&self) -> Option<GlobalIndex> {
        match &self.base {
            rkyv::option::ArchivedOption::None => None,
            rkyv::option::ArchivedOption::Some(base) => rkyv::deserialize::<_, String>(base).ok(),
        }
    }

    fn offset(&self) -> usize {
        rkyv::deserialize::<_, ()>(&self.offset).unwrap()
    }
}

/// A data initializer for linear memory.
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct DataInitializer<'data> {
    /// The location where the initialization is to be performed.
    pub location: DataInitializerLocation,

    /// The initialization data.
    pub data: &'data [u8],
}

/// As `DataInitializer` but owning the data rather than
/// holding a reference to it
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(Debug, Clone, PartialEq, Eq, RkyvSerialize, RkyvDeserialize, Archive)]
#[rkyv(derive(Debug))]
pub struct OwnedDataInitializer {
    /// The location where the initialization is to be performed.
    pub location: DataInitializerLocation,

    /// The initialization owned data.
    pub data: Box<[u8]>,
}

/// Any struct that acts like a `DataInitializer`.
#[allow(missing_docs)]
pub trait DataInitializerLike<'a> {
    type Location: DataInitializerLocationLike + Copy + 'a;

    fn location(&self) -> Self::Location;
    fn data(&self) -> &'a [u8];
}

impl OwnedDataInitializer {
    /// Creates a new `OwnedDataInitializer` from a `DataInitializer`.
    pub fn new(borrowed: &DataInitializer<'_>) -> Self {
        Self {
            location: borrowed.location.clone(),
            data: borrowed.data.to_vec().into_boxed_slice(),
        }
    }
}

impl<'a> DataInitializerLike<'a> for &'a OwnedDataInitializer {
    type Location = &'a DataInitializerLocation;

    fn location(&self) -> Self::Location {
        &self.location
    }

    fn data(&self) -> &'a [u8] {
        self.data.as_ref()
    }
}

impl<'a> DataInitializerLike<'a> for &'a ArchivedOwnedDataInitializer {
    type Location = &'a ArchivedDataInitializerLocation;

    fn location(&self) -> Self::Location {
        &self.location
    }

    fn data(&self) -> &'a [u8] {
        self.data.as_ref()
    }
}
