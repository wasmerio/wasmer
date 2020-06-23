// WASI:
// mapdir: hamlet:test_fs/hamlet

use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "wasi_unstable")]
extern "C" {
    fn fd_close(fd: u32) -> u16;
    fn fd_fdstat_set_flags(fd: u32, flags: u16) -> u16;
}

const FIRST_PREOPEN_FD: u32 = 4;

fn main() {
    #[cfg(target_os = "wasi")]
    {
        let result = unsafe { fd_fdstat_set_flags(FIRST_PREOPEN_FD, 1 << 2) };
        println!(
            "accessing preopen fd was a {}",
            if result == 0 { "success" } else { "failure" }
        );

        let result = unsafe { fd_close(FIRST_PREOPEN_FD) };
        println!(
            "Closing preopen fd was a {}",
            if result == 0 { "success" } else { "failure" }
        );

        let result = unsafe { fd_fdstat_set_flags(FIRST_PREOPEN_FD, 1 << 2) };
        println!(
            "accessing closed preopen fd was an EBADF error: {}",
            if result == 8 { "true" } else { "false" }
        );
    }

    #[cfg(not(target_os = "wasi"))]
    {
        println!("accessing preopen fd was a success");
        println!("Closing preopen fd was a success");
        println!("accessing closed preopen fd was an EBADF error: true");
    }
}
