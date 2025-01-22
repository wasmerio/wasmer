pub fn print_info_on_error(output: &std::process::Output, context: &str) {
    if !output.status.success() {
        println!("{context}");
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
