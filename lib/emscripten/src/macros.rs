macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {{
        use std::cell::Cell;
        (&$memory.view::<u8>()[($pointer as usize)..]).as_ptr() as *mut Cell<u8> as *mut u8
    }};
}
