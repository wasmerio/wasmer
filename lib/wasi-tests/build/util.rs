pub fn print_info_on_error(output: &std::process::Output, context: &str) {
    if !output.status.success() {
        println!("{}", context);
        println!(
            "stdout:\n{}",
            std::str::from_utf8(&output.stdout[..]).unwrap()
        );
        eprintln!(
            "stderr:\n{}",
            std::str::from_utf8(&output.stderr[..]).unwrap()
        );
    }
}

/// Whether or not we should operate on all WASI tests or not
pub fn should_operate_on_all_wasi_tests() -> bool {
    std::env::var("WASI_TEST_GENERATE_ALL")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
        == 1
}
