extern "C" {
    fn call_guest_fn(f: u32) -> u32;
    fn call_guest_fn2(f: u32) -> u32;
    fn host_callback() -> u32;
}

#[no_mangle]
fn test_callback() -> u32 {
    42
}

#[no_mangle]
fn test_callback2() -> u32 {
    45
}

fn main() {
    unsafe { call_guest_fn(test_callback as usize as u32) };
    unsafe { call_guest_fn(host_callback as usize as u32) };
    unsafe { call_guest_fn(test_callback2 as usize as u32) };
    unsafe { call_guest_fn2(test_callback2 as usize as u32) };
    unsafe { call_guest_fn2(test_callback as usize as u32) };
    unsafe { call_guest_fn2(host_callback as usize as u32) };
}
