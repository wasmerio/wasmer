extern "C" {
    fn print_str(base: *const u8, len: usize) -> i32;
}

#[no_mangle]
pub extern "C" fn main() -> i32 {
    let v: Vec<i32> = (0..10).collect();
    let s = format!("Hello world from WebAssembly. Some heap allocated integers: {:?}", v);
    let s = s.as_bytes();
    unsafe {
        print_str(s.as_ptr(), s.len());
    }
    return 0;
}
