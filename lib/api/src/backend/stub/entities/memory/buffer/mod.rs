use std::marker::PhantomData;

/// Minimal memory buffer placeholder for the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryBuffer<'a> {
    _marker: PhantomData<&'a mut [u8]>,
}

impl<'a> MemoryBuffer<'a> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}
