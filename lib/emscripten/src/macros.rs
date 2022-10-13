macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {
        $memory.data_ptr().wrapping_add($pointer as usize)
    };
}
