#[cfg(unix)]
use crate::sys::unix::{Memory, Protect};

#[cfg(windows)]
use crate::sys::windows::{Memory, Protect};

use rkyv::{
    Archive, 
    Archived,
    Fallible,
    Serialize as RkyvSerialize,
    Deserialize as RkyvDeserialize,
    ser::{Serializer, ScratchSpace},
    with::{ArchiveWith, SerializeWith, DeserializeWith},
};

/// A serializable wrapper for Memory.
pub struct ArchivableMemory;

/// The archived contents of a Memory.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, PartialEq)]
#[archive(compare(PartialEq))]
#[archive_attr(derive(Debug))]
#[archive_attr(derive(PartialEq))]
pub struct CompactMemory {
    contents: Vec<u8>,
    content_size: u32,
    protection: Protect,
}

impl CompactMemory {
    /// Construct a CompactMemory from a Memory.
    pub unsafe fn from_memory(memory: &Memory) -> Self {
        CompactMemory {
            contents: memory.as_slice().to_vec(),
            content_size: memory.content_size(),
            protection: memory.protection(),
        }
    }

    /// Construct a Memory from a CompactMemory.
    pub unsafe fn into_memory(&self) -> Memory {
        let bytes = self.contents.as_slice();

        let mut memory = Memory::with_size_protect(bytes.len(), Protect::ReadWrite)
            .expect("Could not create a memory");

        memory.as_slice_mut().copy_from_slice(&*bytes);

        if memory.protection() != self.protection {
            memory
                .protect(.., self.protection)
                .expect("Could not protect memory as its original protection");
        }

        memory.set_content_size(self.content_size);

        memory
    }
}

impl ArchiveWith<Memory> for ArchivableMemory {
    type Archived = <CompactMemory as Archive>::Archived;
    type Resolver = <CompactMemory as Archive>::Resolver;

    unsafe fn resolve_with(memory: &Memory, pos: usize, resolver: Self::Resolver, out: *mut Self::Archived) {
        let archived_memory = CompactMemory::from_memory(memory);
        archived_memory.resolve(pos, resolver, out);
    }
}

impl<S: Fallible + ?Sized> SerializeWith<Memory, S> for ArchivableMemory 
where
    S: Serializer + ScratchSpace
{
    fn serialize_with(memory: &Memory, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        unsafe {
            let archived_memory = CompactMemory::from_memory(memory);
            archived_memory.serialize(serializer)
        }
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<CompactMemory>, Memory, D> for ArchivableMemory
{
    fn deserialize_with(archived_memory: &Archived<CompactMemory>, deserializer: &mut D) -> Result<Memory, D::Error> {
        let compact_memory: CompactMemory = archived_memory.deserialize(deserializer)?;
        let memory = unsafe { compact_memory.into_memory() };

        Ok(memory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkyv::ser::serializers::AllocSerializer;
    use crate::sys::unix::*;

    #[test]
    fn test_new_memory() {
        let bytes = make_test_bytes();
        let memory = make_test_memory(&bytes);
        unsafe {
            assert_eq!(memory.as_slice(), bytes);
        }
    }

    #[test]
    fn test_rkyv_compact_memory() {
        let bytes = make_test_bytes();
        let memory = make_test_memory(&bytes);

        let compact_memory = unsafe { CompactMemory::from_memory(&memory) };

        let mut serializer = AllocSerializer::<4096>::default();
        serializer.serialize_value(&compact_memory).unwrap();
        let serialized = serializer.into_serializer().into_inner();
        assert!(serialized.len() > 0);

        let archived = unsafe { rkyv::archived_root::<CompactMemory>(&serialized[..]) };

        let deserialized: CompactMemory = archived.deserialize(&mut rkyv::Infallible).unwrap();
        assert_eq!(deserialized, compact_memory);

        let deserialized_memory = unsafe { deserialized.into_memory() };
        assert_eq!(deserialized_memory.protection(), memory.protection());
        unsafe {
            assert_eq!(deserialized_memory.as_slice(), memory.as_slice());
        };
    }

    fn make_test_memory(bytes: &Vec<u8>) -> Memory {
        let mut memory = Memory::with_size_protect(1000, Protect::ReadWrite)
            .expect("Could not create memory");
        unsafe {
            memory.as_slice_mut().copy_from_slice(&bytes[..]);
        }
        memory
    }

    fn make_test_bytes() -> Vec<u8> {
        let page_size = page_size::get();
        let mut bytes = b"abcdefghijkl".to_vec();
        let padding_zeros = [0 as u8; 1].repeat(page_size - bytes.len());
        bytes.extend(padding_zeros);
        bytes
    }
}
