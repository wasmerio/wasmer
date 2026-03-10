#[macro_use]
extern crate lazy_static;

#[allow(dead_code)]
extern "C" {
    fn get_hashed_password(ptr: u32, len: u32) -> u32;
    fn print_char(c: u32);
}

#[allow(dead_code)]
fn print_str(s: &str) {
    for c in s.chars() {
        unsafe { print_char(c as u32) };
    }
    unsafe { print_char(b'\n' as u32) };
}

fn load_hashed_password() -> Option<String> {
    let mut buffer = String::with_capacity(32);
    for _ in 0..32 {
        buffer.push(0 as char);
    }
    let result =
        unsafe { get_hashed_password(buffer.as_mut_ptr() as u32, buffer.capacity() as u32) };

    if result == 0 {
        Some(buffer)
    } else {
        None
    }
}

lazy_static! {
    static ref HASHED_PASSWORD: String = load_hashed_password().unwrap();
}

static PASSWORD_CHARS: &'static [u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

// for simplicty we define a scheme for mapping numbers onto passwords
fn num_to_password(mut num: u64) -> String {
    let mut extra_zero = num == 0;
    let mut out = String::new();
    while num > 0 {
        out.push(PASSWORD_CHARS[num as usize % PASSWORD_CHARS.len()] as char);
        extra_zero = extra_zero || num == PASSWORD_CHARS.len() as u64;
        num /= PASSWORD_CHARS.len() as u64;
    }

    if extra_zero {
        out.push(PASSWORD_CHARS[0] as char);
    }

    out
}

#[repr(C)]
struct RetStr {
    ptr: u32,
    len: u32,
}

// returns a (pointer, len) to the password or null
#[no_mangle]
fn check_password(from: u64, to: u64) -> u64 {
    for i in from..to {
        let password = num_to_password(i);
        let digest = md5::compute(&password);

        let hash_as_str = format!("{:x}", digest);
        if hash_as_str == *HASHED_PASSWORD {
            let ret = RetStr {
                ptr: password.as_ptr() as usize as u32,
                len: password.len() as u32,
            };
            // leak the data so ending the function doesn't corrupt it, if we cared the host could free it after
            std::mem::forget(password);
            return unsafe { std::mem::transmute(ret) };
        }
    }

    return 0;
}

fn main() {}
