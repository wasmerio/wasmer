macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {
        0 as usize
        // unsafe { $memory.as_ptr().add($pointer as usize) }
    };
}
