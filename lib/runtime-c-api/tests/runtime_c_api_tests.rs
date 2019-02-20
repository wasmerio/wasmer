use std::process::Command;

#[test]
fn test_c_api() {
    let project_tests_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    run_command("cmake", project_tests_dir, Some("."));
    run_command("make", project_tests_dir, None);
    run_command("make", project_tests_dir, Some("test"));
}

fn run_command(command_str: &str, dir: &str, arg: Option<&str>) {
    println!("Running command: `{}` arg: {:?}", command_str, arg);
    let mut command = Command::new(command_str);
    if let Some(a) = arg {
        command.arg(a);
    }
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
