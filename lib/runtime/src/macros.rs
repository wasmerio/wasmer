#[macro_export]
macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt), line!()) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("wasmer-runtime(:{})::", $fmt, "\n"), line!(), $($arg)*) });
}
