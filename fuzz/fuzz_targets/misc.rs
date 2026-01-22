pub fn save_wasm_file(data: &[u8]) {
    if let Ok(path) = std::env::var("DUMP_TESTCASE") {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(&path).unwrap();
        eprintln!("Saving fuzzed WASM file to: {path:?}");
        file.write_all(data).unwrap();
    }
}
