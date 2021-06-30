// We don't have access to libc, so we just use isatty
// as an external function
// use libc::isatty;

extern "C" {
    pub fn isatty(fd: i32) -> i32;
}

fn main() {
    #[cfg(target = "wasi")]
    {
        println!("stdin: {}", unsafe { isatty(0) });
        println!("stdout: {}", unsafe { isatty(1) });
        println!("stderr: {}", unsafe { isatty(2) });
    }
    #[cfg(not(target = "wasi"))]
    {
        println!("stdin: 1");
        println!("stdout: 1");
        println!("stderr: 1");
    }
}
