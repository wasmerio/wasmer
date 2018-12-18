pub struct VMOffsets {
    pub(in crate::webassembly) ptr_size: u8,
}

impl VMOffsets {
    pub fn new(ptr_size: u8) -> Self {
        Self { ptr_size }
    }
}
