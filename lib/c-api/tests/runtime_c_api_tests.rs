use std::process::Command;

#[test]
fn test_c_api() {
    let project_tests_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");

    let cmake_args = vec![
        ".",
        #[cfg(feature = "wasi")]
        "-DWASI_TESTS=ON",
        #[cfg(feature = "emscripten")]
        "-DEMSCRIPTEN_TESTS=ON",
    ];
    // we use -f so it doesn't fail if the file doesn't exist
    run_command("rm", project_tests_dir, vec!["-f", "CMakeCache.txt"]);
    run_command("cmake", project_tests_dir, cmake_args);
    run_command("make", project_tests_dir, vec!["-Wdev", "-Werror=dev"]);
    run_command("make", project_tests_dir, vec!["test", "ARGS=\"-V\""]);
}

fn run_command(command_str: &str, dir: &str, args: Vec<&str>) {
    println!("Running command: `{}` args: {:?}", command_str, args);

    let mut command = Command::new(command_str);

    command.args(&args);

    command.current_dir(dir);

    let result = command.output();

    match result {
        Ok(r) => {
            println!("output:");

            if let Some(code) = r.status.code() {
                println!("status: {}", code);
            } else {
                println!("status: None");
            }

            println!("stdout:");
            println!("{}", String::from_utf8_lossy(&r.stdout[..]));
            println!("stderr:");
            println!("{}", String::from_utf8_lossy(&r.stderr[..]));

            if r.status.success() {
                assert!(true)
            } else {
                panic!("Command failed with exit status: {:?}", r.status);
            }
        }

        Err(e) => panic!("Command failed: {}", e),
    }
}
