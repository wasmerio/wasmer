/// Retrieve a WebAssembly function given a Instance and a FuncIndex
/// Example:
/// let func: fn(i32) -> i32 = get_instance_function!(instance, func_index);
#[macro_export]
macro_rules! get_instance_function {
    ($instance:expr, $func_index:expr) => {{
        use std::mem;
        let func_addr = $instance.get_function_pointer($func_index);
        unsafe { mem::transmute(func_addr) }
    }};
}

#[macro_export]
macro_rules! include_wast2wasm_bytes {
    ($x:expr) => {{
        use wabt::wat2wasm;
        const WAST_BYTES: &[u8] = include_bytes!($x);
        wat2wasm(WAST_BYTES.to_vec()).expect(&format!("Can't convert {} file to wasm", $x))
    }};
}

#[macro_export]
macro_rules! debug {
    ($fmt:expr) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("Wasmer::", $fmt)) });
    ($fmt:expr, $($arg:tt)*) => (if cfg!(any(debug_assertions, feature="debug")) { println!(concat!("Wasmer::", $fmt, "\n"), $($arg)*) });
}
