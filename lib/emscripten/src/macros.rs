macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt), line!()) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt, "\n"), line!(), $($arg)*) });
}

macro_rules! emscripten_memory_pointer {
    ($memory:expr, $pointer:expr) => {
        unsafe { $memory.as_ptr().add($pointer as usize) }
    };
}
