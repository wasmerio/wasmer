macro_rules! emscripten_memory_pointer {
    ($ctx:expr, $memory:expr, $pointer:expr) => {
        $memory.data_ptr(&$ctx).wrapping_add($pointer as usize)
    };
}
