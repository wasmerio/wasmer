// Args:
// mapdir: temp:wasitests/test_fs/temp

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("wasitests/test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("/");

    let file_to_create = base.join("temp/path_rename_file.txt");
    let file_to_rename_to = base.join("temp/path_renamed_file.txt");

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
