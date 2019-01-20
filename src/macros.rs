#[macro_export]
macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("Wasmer::", $fmt)) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("Wasmer::", $fmt, "\n"), $($arg)*) });
}
