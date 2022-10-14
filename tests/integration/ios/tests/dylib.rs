#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::process::{Command, Output, Stdio};

    #[test]
    fn test_runtime() {
        // Remove anuthing left over from tests
        remove_existing_artificats();

        // Tets the 'DylibExample' scheme
        let success = run_ios_test("DylibExample/DylibExample.xcodeproj", "DylibExample");
        if !success {
            panic!("Dylib iOS Tests failed with the above output!");
        }
    }

    fn run_ios_test(dir: &str, scheme: &str) -> bool {
        let command = Command::new("xcodebuild")
            .arg("test")
            .arg("-project")
            .arg(dir)
            .arg("-scheme")
            .arg(scheme)
            .arg("-destination")
            .arg("platform=iOS Simulator,name=iPhone 12 Pro")
            .arg("CODE_SIGNING_ALLOWED=NO")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .expect("Could not run iOS Test");

        // Get output from xcodebuild CLI:
        let stderr = String::from_utf8(command.stderr).unwrap();

        /*
            An iOS Test Result is quite odd, we check stderr for the phrase 'TEST FAILED'
            and then return stdout which contains the failure reason;
            We also check that the command executed correctly!
        */
        let command_success = command.status.success();
        let test_success = !stderr.contains("** TEST FAILED **");

        command_success && test_success
    }

    fn remove_existing_artificats() -> Output {
        Command::new("rm")
            .arg("-f")
            .arg("DylibExample/DylibExample/sum.dylib")
            .output()
            .expect("Could not clear artificats")
    }
}
