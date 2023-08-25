use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::path::Path;

fn print_contents(path: impl AsRef<Path> + Copy) {
    let contents = fs::read_to_string(path).unwrap();
    println!(
        "{}:{contents}",
        path.as_ref().to_string_lossy().into_owned()
    );
}

fn main() {
    unsafe {
        fs::create_dir("/test-dir").unwrap();

        fs::write("/test-dir/a", "a").unwrap();
        print_contents("/test-dir/a");

        let dir = File::open("/test-dir").unwrap();
        let dir_raw_fd = dir.as_raw_fd() as u32;

        wasi::path_rename(dir_raw_fd, "a", dir_raw_fd, "b").unwrap();

        print_contents("/test-dir/b");

        fs::write("/test-dir/c", "c").unwrap();
        print_contents("/test-dir/c");

        fs::rename("/test-dir/c", "/test-dir/d").unwrap();

        print_contents("/test-dir/d");
    }
}
