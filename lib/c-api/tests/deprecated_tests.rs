use std::process::Command;

#[test]
fn test_deprecated_c_api() {
    let project_tests_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/deprecated/");

    let cmake_args = vec![
        ".",
        #[cfg(feature = "wasi")]
        "-DWASI_TESTS=ON",
        #[cfg(feature = "emscripten")]
        "-DEMSCRIPTEN_TESTS=ON",
        // We need something like this to get this working on Windows, but this doesn't seem
        // quite right -- perhaps it's double escaping the quotes?
        #[cfg(target_os = "windows")]
        r#"-G "MinGW Makefiles""#,
    ];
    // we use -f so it doesn't fail if the file doesn't exist
    run_command("rm", project_tests_dir, vec!["-f", "CMakeCache.txt"]);
    run_command("cmake", project_tests_dir, cmake_args);
    run_command("make", project_tests_dir, vec!["-Wdev", "-Werror=dev"]);
    run_command("make", project_tests_dir, vec!["test", "ARGS=\"-V\""]);
}

fn run_command(command_str: &str, dir: &str, args: Vec<&str>) {
    println!(
        "Running command: `{}` with arguments: {:?}",
        command_str, args
    );

    let mut command = Command::new(command_str);
    command.args(&args);
    command.current_dir(dir);

    match command.output() {
        Ok(result) => {
            println!(
                ">   Status: `{:?}`\n>   Stdout: `{}`\n>   Stderr: `{}`",
                result.status.code(),
                String::from_utf8_lossy(&result.stdout[..]),
                String::from_utf8_lossy(&result.stderr[..]),
            );

            if result.status.success() {
                assert!(true)
            } else {
                panic!("Command failed with exit status: `{:?}`", result.status);
            }
        }
        Err(error) => panic!("Command failed: `{}`", error),
    }

    println!("\n");
}
