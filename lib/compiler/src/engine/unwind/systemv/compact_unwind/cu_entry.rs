#[derive(Clone)]
pub struct CompactUnwindEntryReader {
    pub ptr: *const u8,
    pub len: usize,
}

impl CompactUnwindEntryReader {
    pub fn new(ptr: *const u8, len: usize) -> Self {
        Self { ptr, len }
    }

    pub unsafe fn read<T: Copy>(&mut self) -> T {
        unsafe {
            if self.len == 0 {
                panic!("Trying to read after the CompactUnwind section!");
            }
            let result = self.ptr.cast::<T>().read_unaligned();
            let size = std::mem::size_of::<T>();
            self.ptr = self.ptr.byte_add(size);
            self.len -= size;
            result
        }
    }
}

/// An entry in the `__compact_unwind` section.
#[derive(Clone)]
pub struct CompactUnwindEntry {
    pub function_addr: usize,
    pub length: u32,
    pub compact_encoding: u32,
    pub personality_addr: usize,
    pub lsda_addr: usize,
}

impl std::fmt::Debug for CompactUnwindEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompactUnwindEntry")
            .field("function_addr", &(self.function_addr as *const u8))
            .field("length", &self.length)
            .field("compact_encoding", &self.compact_encoding)
            .field("personality_addr", &(self.personality_addr as *const u8))
            .field("lsda_addr", &(self.lsda_addr as *const u8))
            .finish()
    }
}

impl CompactUnwindEntry {
    const ENCODING_MODE_MASK: u32 = 0x0f000000;

    pub unsafe fn from_ptr_and_len(ptr: *const u8, len: usize) -> Self {
        let mut reader = CompactUnwindEntryReader::new(ptr, len);
        let function_addr = reader.read::<usize>();
        let length = reader.read::<u32>();
        let compact_encoding = reader.read::<u32>();
        let personality_addr = reader.read::<usize>();
        let lsda_addr = reader.read::<usize>();
        Self {
            function_addr,
            length,
            compact_encoding,
            personality_addr,
            lsda_addr,
        }
    }

    pub fn is_dwarf(&self) -> bool {
        const DWARFMODE: u32 = 0x04000000;
        (self.compact_encoding & Self::ENCODING_MODE_MASK) == DWARFMODE
    }

    pub fn cannot_be_merged(&self) -> bool {
        const STACK_INDIRECT_MODE: u32 = 0x03000000;
        (self.compact_encoding & Self::ENCODING_MODE_MASK) == STACK_INDIRECT_MODE
    }
}
