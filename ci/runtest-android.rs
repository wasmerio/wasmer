use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args = env::args_os()
        .skip(1)
        .filter(|arg| arg != "--quiet")
        .collect::<Vec<_>>();
    assert_eq!(args.len(), 1);
    let test = PathBuf::from(&args[0]);

    // required to run an executable depending on wabt-rs
    let android_ndk_home = env::var("ANDROID_NDK_HOME").expect("Can't get ANDROID_NDK_HOME!");
    let path = format!("{}/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/x86_64-linux-android/libc++_shared.so", android_ndk_home);
    let libcpp_shared = Path::new(&path);

    let dst = Path::new("/data/local/tmp");
    let dst_exec = Path::new("/data/local/tmp").join(test.file_name().unwrap());

    let status = Command::new("adb")
        .arg("wait-for-device")
        .status()
        .expect("failed to run: adb wait-for-device");
    assert!(status.success());

    let status = Command::new("adb")
        .arg("push")
        .arg(&test)
        .arg(&libcpp_shared)
        .arg(&dst)
        .status()
        .expect("failed to run: adb pushr");
    assert!(status.success());

    let output = Command::new("adb")
        .arg("shell")
        .arg("LD_LIBRARY_PATH=/data/local/tmp/")
        .arg(&dst_exec)
        .output()
        .expect("failed to run: adb shell");
    assert!(status.success());

    println!(
        "status: {}\nstdout ---\n{}\nstderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|l| {
            (l.starts_with("PASSED ") && l.contains(" tests")) || l.starts_with("test result: ok")
        })
        .unwrap_or_else(|| {
            panic!("failed to find successful test run");
        });
}
