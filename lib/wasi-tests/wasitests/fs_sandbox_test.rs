fn main() {
    #[cfg(target = "wasi")]
    let result = std::fs::read_dir("..");
    #[cfg(not(target = "wasi"))]
    let result: Result<(), String> = Err("placeholder".to_string());
    println!(
        "Reading the parent directory was okay? {:?}",
        result.is_ok()
    );
}
