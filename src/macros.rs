/// This macro helps to get a function for an instance easily
/// let func: fn(i32) -> i32 = get_instance_function!(instance, func_index);
#[macro_export]
macro_rules! get_instance_function {
    ($instance:expr, $func_index:expr) => {{
        use std::mem;
        let func_addr = $instance.get_function_pointer($func_index);
        unsafe { mem::transmute(func_addr) }
    }};
}

macro_rules! include_wast2wasm_bytes {
    ($x:expr) => {{
        use wabt::wat2wasm;
        const wast_bytes: &[u8] = include_bytes!($x);
        wat2wasm(wast_bytes.to_vec()).expect(&format!("Can't convert {} file to wasm", $x))
    }};
}

// #[cfg(feature = "debug")]
#[macro_export]
macro_rules! debug {
    ($fmt:expr) => (println!(concat!("Wasmer::", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (println!(concat!("Wasmer::", $fmt, "\n"), $($arg)*));
}
