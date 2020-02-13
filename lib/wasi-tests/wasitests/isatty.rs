// use libc::isatty;

extern "C" {
    pub fn isatty(fd: i32) -> i32;
}

fn main() {
    println!("stdin: {}", unsafe { isatty(0) });
    println!("stdout: {}", unsafe { isatty(1) });
    println!("stderr: {}", unsafe { isatty(2) });
}
