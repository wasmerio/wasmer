use std::process::Command;

pub fn compile_with_cmake_and_run_test(project_tests_dir: &str, cmake_args: Vec<&str>) {
    run_command(
        "rm",
        project_tests_dir,
        vec![
            "-f", // we use -f so it doesn't fail if the file doesn't exist
            "CMakeCache.txt",
        ],
    );
    run_command("cmake", project_tests_dir, cmake_args);
    run_command("make", project_tests_dir, vec!["clean"]);
    run_command("make", project_tests_dir, vec!["-Wdev", "-Werror=dev"]);
    run_command("make", project_tests_dir, vec!["test", "ARGS=\"-V\""]);
}

pub fn run_command(command_str: &str, dir: &str, args: Vec<&str>) {
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
