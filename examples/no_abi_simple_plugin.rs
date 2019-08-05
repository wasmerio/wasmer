extern "C" {
    fn it_works() -> i32;
}

#[no_mangle]
pub fn plugin_entrypoint(n: i32) -> i32 {
    let result = unsafe { it_works() };
    result + n
}

fn main() {}
