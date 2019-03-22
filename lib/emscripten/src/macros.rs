use wasmer_runtime_core::memory::Memory;
macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {{
        use std::cell::Cell;
        (&$memory.view::<u8>()[($pointer as usize)..]).as_ptr() as *mut Cell<u8> as *mut u8
    }};
}

#[inline]
pub fn emscripten_memory_ptr(memory: &Memory, offset: u32) -> *mut u8 {
    use std::cell::Cell;
    (&memory.view::<u8>()[(offset as usize)..]).as_ptr() as *mut Cell<u8> as *mut u8
}
