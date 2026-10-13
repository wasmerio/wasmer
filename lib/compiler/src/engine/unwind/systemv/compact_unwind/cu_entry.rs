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
