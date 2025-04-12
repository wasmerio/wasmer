// WASI:
// tempdir: temp

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

fn run_with_toplevel_dir() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("temp");

    let file_to_create = base.join("path_rename_file.txt");
    let file_to_rename_to = base.join("path_renamed_file.txt");

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_to_create)
            .unwrap();

        // text from https://ja.wikipedia.org/wiki/柴犬
        let shiba_string = "「柴犬」という名前は中央高地で使われていたもので、文献上では、昭和初期の日本犬保存会の会誌「日本犬」で用いられている。一般的には、「柴」は小ぶりな雑木を指す。
由来には諸説があり、

    柴藪を巧みにくぐり抜けて猟を助けることから
    赤褐色の毛色が枯れ柴に似ている（柴赤）ことから
    小さなものを表す古語の「柴」から

の3つの説が代表的。";
        let shiba_bytes: Vec<u8> = shiba_string.bytes().collect();
        f.write_all(&shiba_bytes[..]).unwrap();
    }

    std::fs::rename(&file_to_create, &file_to_rename_to).unwrap();
    let mut file = fs::File::open(&file_to_rename_to).expect("Could not open file");
    if file_to_create.exists() {
        println!("The original file still exists!");
        return;
    } else {
        println!("The original file does not still exist!");
    }

    if !file_to_rename_to.exists() {
        println!("The moved file does not exist!");
        return;
    }

    // TODO: add temp directory support for native execution...
    // until then, don't actually inspect the directory when running native code.
    #[cfg(target_os = "wasi")]
    for item in fs::read_dir(&base).unwrap() {
        println!(
            "Found item: {}",
            item.unwrap().path().file_name().unwrap().to_str().unwrap()
        );
    }
    #[cfg(not(target_os = "wasi"))]
    {
        println!("Found item: path_renamed_file.txt");
    }

    let mut out_str = String::new();
    file.read_to_string(&mut out_str).unwrap();
    let mut test_str = String::new();
    let mut out_chars = out_str.chars();
    out_chars.next().unwrap();
    test_str.push(out_chars.next().unwrap());
    test_str.push(out_chars.next().unwrap());

    println!("{}", test_str);
    std::fs::remove_file(file_to_rename_to).unwrap();
}

fn run_with_toplevel_dir_overwrite() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("temp");

    let file_to_create = base.join("path_rename_file.txt");
    let file_to_rename_to = base.join("path_renamed_file.txt");

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_to_create)
            .unwrap();

        // text from https://ja.wikipedia.org/wiki/柴犬
        let shiba_string = "「柴犬」という名前は中央高地で使われていたもので、文献上では、昭和初期の日本犬保存会の会誌「日本犬」で用いられている。一般的には、「柴」は小ぶりな雑木を指す。
由来には諸説があり、

    柴藪を巧みにくぐり抜けて猟を助けることから
    赤褐色の毛色が枯れ柴に似ている（柴赤）ことから
    小さなものを表す古語の「柴」から

の3つの説が代表的。";
        f.write_all(shiba_string.as_bytes()).unwrap();
    }

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_to_rename_to)
            .unwrap();

        let lorem_string = "lorem ispum";
        f.write_all(lorem_string.as_bytes()).unwrap();
    }

    std::fs::rename(&file_to_create, &file_to_rename_to).unwrap();
    let mut file = fs::File::open(&file_to_rename_to).expect("Could not open file");
    if file_to_create.exists() {
        println!("The original file still exists!");
        return;
    } else {
        println!("The original file does not still exist!");
    }


    if !file_to_rename_to.exists() {
        println!("The moved file does not exist!");
        return;
    }

    // TODO: add temp directory support for native execution...
    // until then, don't actually inspect the directory when running native code.
    #[cfg(target_os = "wasi")]
    for item in fs::read_dir(&base).unwrap() {
        println!(
            "Found item: {}",
            item.unwrap().path().file_name().unwrap().to_str().unwrap()
        );
    }
    #[cfg(not(target_os = "wasi"))]
    {
        println!("Found item: path_renamed_file.txt");
    }

    let mut out_str = String::new();
    file.read_to_string(&mut out_str).unwrap();
    let mut test_str = String::new();
    let mut out_chars = out_str.chars();
    out_chars.next().unwrap();
    test_str.push(out_chars.next().unwrap());
    test_str.push(out_chars.next().unwrap());

    println!("{}", test_str);
    std::fs::remove_file(file_to_rename_to).unwrap();
}

fn run_with_sub_dir() {
    #[cfg(not(target_os = "wasi"))]
    let base = PathBuf::from("test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("temp");

    //make a sub-directory
    fs::create_dir(base.join("sub"));

    let file_to_create = base.join("sub/path_rename_file.txt");
    let file_to_rename_to = base.join("sub/path_renamed_file.txt");

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_to_create)
            .unwrap();

        let string = "Hello world";
        let bytes: Vec<u8> = string.bytes().collect();
        f.write_all(&bytes[..]).unwrap();
    }

    std::fs::rename(&file_to_create, &file_to_rename_to).unwrap();
    let mut file = fs::File::open(&file_to_rename_to).expect("Could not open file");
    if file_to_create.exists() {
        println!("run_with_sub_dir: The original file still exists!");
        return;
    } else {
        println!("run_with_sub_dir: The original file does not still exist!");
    }

    if !file_to_rename_to.exists() {
        println!("run_with_sub_dir: The moved file does not exist!");
        return;
    }
    fs::remove_dir_all(base.join("sub"));
}

fn run_with_different_sub_dirs() {
    #[cfg(not(target_os = "wasi"))]
    let base = PathBuf::from("test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("temp");

    //make sub-directories
    fs::create_dir(base.join("a"));
    fs::create_dir(base.join("a/b"));
    fs::create_dir(base.join("c"));
    fs::create_dir(base.join("c/d"));
    fs::create_dir(base.join("c/d/e"));

    let file_to_create = base.join("a/b/path_rename_file.txt");
    let file_to_rename_to = base.join("c/d/e/path_renamed_file.txt");

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_to_create)
            .unwrap();

        let string = "Hello world";
        let bytes: Vec<u8> = string.bytes().collect();
        f.write_all(&bytes[..]).unwrap();
    }

    std::fs::rename(&file_to_create, &file_to_rename_to).unwrap();
    let mut file = fs::File::open(&file_to_rename_to).expect("Could not open file");
    if file_to_create.exists() {
        println!("run_with_different_sub_dirs: The original file still exists!");
        return;
    } else {
        println!("run_with_different_sub_dirs: The original file does not still exist!");
    }

    if !file_to_rename_to.exists() {
        println!("run_with_different_sub_dirs: The moved file does not exist!");
        return;
    }

    fs::remove_dir_all(base.join("a"));
    fs::remove_dir_all(base.join("c"));
}

fn main() {
    run_with_toplevel_dir();
    run_with_toplevel_dir_overwrite();
    run_with_sub_dir();
    run_with_different_sub_dirs();
}
