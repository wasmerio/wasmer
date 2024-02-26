// WASI:
// dir: test_fs
#![feature(wasi_ext)]

use std::fs;
use std::os::wasi::fs::MetadataExt;

// `test_fs` will be implicit.
// this need experimentat MetadataExt
// this program does nothing in native
// it only tests things in wasi
fn main() {
    let meta1 = fs::metadata("/hamlet/act1/scene1.txt").expect("could not find src file");
    let meta2 = fs::metadata("/hamlet/act1/scene2.txt").expect("could not find src file");
    if meta1.dev() == meta2.dev() && meta1.ino() == meta2.ino() {
        println!("Warning, different files from same folder have same dev/inod");
    }
    let meta3 = fs::metadata("/hamlet/act2/scene1.txt").expect("could not find src file");
    if meta1.dev() == meta3.dev() && meta1.ino() == meta3.ino() {
        println!(
            "Warning, different files from different folder with same name  have same dev/inod"
        );
    }
    let meta4 = fs::metadata("/hamlet/act1/../act1/scene1.txt").expect("could not find src file");
    if meta1.dev() != meta4.dev() || meta1.ino() != meta4.ino() {
        println!("Warning, same files have different dev/inod");
    }

    println!("all done");
}
