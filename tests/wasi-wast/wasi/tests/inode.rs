// WASI:
// dir: test_fs

use std::fs;
use std::io;
#[cfg(target = "wasi")]
use std::os::wasi::fs::MetadataExt;

// `test_fs` will be implicit.
// this need experimental MetadataExt
// this program does nothing in native
// it only tests things in wasi
fn main() {
    #[cfg(target = "wasi")]
    {
        let meta1 = fs::metadata("test_fs/hamlet/act1/scene1.txt").expect("could not find src file");
        let meta2 = fs::metadata("test_fs/hamlet/act1/scene2.txt").expect("could not find src file");
        if meta1.dev() == meta2.dev() && meta1.ino() == meta2.ino() {
            println!("Warning, different files from same folder have same dev/inod");
        }
        let meta3 = fs::metadata("test_fs/hamlet/act2/scene1.txt").expect("could not find src file");
        if meta1.dev() == meta3.dev() && meta1.ino() == meta3.ino() {
            println!("Warning, different files from different folder with same name  have same dev/inod");
        }
        let meta4 = fs::metadata("test_fs/hamlet/act1/../act1/scene1.txt").expect("could not find src file");
        if meta1.dev() != meta4.dev() || meta1.ino() != meta4.ino() {
            println!("Warning, same files have different dev/inod");
        }
    }
    println!("all done");
}
