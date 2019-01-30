
macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {
        unsafe { $memory.as_ptr().add($pointer as usize) }
    };
}
