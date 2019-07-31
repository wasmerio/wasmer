extern "C" {
    fn it_works() -> i32;
}

#[no_mangle]
pub fn plugin_entrypoint(n: i32) -> i32 {
    println!("Hello from inside WASI");
    let result = unsafe { it_works() };
    result + n
}

pub fn main() {}
