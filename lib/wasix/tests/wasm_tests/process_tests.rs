use std::path::PathBuf;

use super::{
    run_build_script, run_wasm_with_runner_config, run_wasm_with_runner_config_checked,
    MappedDirectory,
};

fn assert_success(result: &super::WasmRunResult) {
    assert_eq!(
        result.exit_code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn assert_stdout_contains(result: &super::WasmRunResult, expected: &str) {
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.lines().any(|line| line == expected),
        "missing stdout line {expected:?}\n{}",
        super::format_captured_output(result)
    );
}

fn run_with_arg(wasm: &PathBuf, arg: &str) {
    let result = run_wasm_with_runner_config(wasm, wasm.parent().unwrap(), |runner| {
        runner.with_args([arg]);
    })
    .unwrap();
    assert_success(&result);
}

fn run_with_arg_in_home(wasm: &PathBuf, arg: &str) {
    let test_dir = wasm.parent().unwrap();
    let result = run_wasm_with_runner_config(wasm, test_dir, |runner| {
        runner
            .with_mapped_directories([MappedDirectory {
                guest: "/home".to_string(),
                host: test_dir.to_path_buf(),
            }])
            .with_current_dir("/home")
            .with_args([arg]);
    })
    .unwrap();
    assert_success(&result);
}

fn assert_stdout_zero(
    wasm: &PathBuf,
    configure_runner: impl FnOnce(&mut wasmer_wasix::runners::wasi::WasiRunner),
) {
    let result =
        run_wasm_with_runner_config(wasm, wasm.parent().unwrap(), configure_runner).unwrap();
    assert_success(&result);
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
}

#[test]
fn test_atomic_wait_signal_wakes_main_thread() {
    let wasm = run_build_script(file!(), "atomic-wait-signal").unwrap();
    let result = run_wasm_with_runner_config(&wasm, wasm.parent().unwrap(), |_| {}).unwrap();

    assert!(
        result.exit_code != Some(0),
        "expected signal termination\n{}",
        super::format_captured_output(&result)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "waiting",
        "{}",
        super::format_captured_output(&result)
    );
}

fn run_atomic_wait_signal_children_variant(wasm: &PathBuf, arg: &str, expected_stdout: &[&str]) {
    let result = run_wasm_with_runner_config(wasm, wasm.parent().unwrap(), |runner| {
        runner.with_args([arg]);
    })
    .unwrap();

    assert_success(&result);
    for expected in expected_stdout {
        assert_stdout_contains(&result, expected);
    }
}

#[test]
fn test_atomic_wait_signal_targeted_child() {
    let wasm = run_build_script(file!(), "atomic-wait-signal-children").unwrap();
    run_atomic_wait_signal_children_variant(
        &wasm,
        "targeted",
        &["targeted child waiting", "targeted parent survived"],
    );
}

#[test]
// TODO: Should other signals also wake up waiters?
// We have low confidence this is useful outside the kill path.
// SEE https://github.com/wasmerio/wasmer/pull/6536
#[ignore = "SIGTERM atomic waiter wakeups are currently scoped back"]
fn test_atomic_wait_signal_forwarded_to_children() {
    let wasm = run_build_script(file!(), "atomic-wait-signal-children").unwrap();
    run_atomic_wait_signal_children_variant(
        &wasm,
        "forwarded",
        &[
            "forwarding parent waiting",
            "forwarded child 1 waiting",
            "forwarded child 2 waiting",
            "forwarding parent survived",
        ],
    );
}

#[test]
fn test_atomic_wait_signal_vfork_child() {
    let wasm = run_build_script(file!(), "atomic-wait-signal-children").unwrap();
    run_atomic_wait_signal_children_variant(
        &wasm,
        "vfork",
        &["vfork child waiting", "vfork parent survived"],
    );
}

#[test]
fn test_posix_spawn() {
    let wasm = run_build_script(file!(), "posix-spawn").unwrap();
    let test_dir = wasm.parent().unwrap();
    run_wasm_with_runner_config_checked(&wasm, test_dir, |runner| {
        runner
            .with_mapped_directories([MappedDirectory {
                guest: "/home".to_string(),
                host: test_dir.to_path_buf(),
            }])
            .with_current_dir("/home");
    })
    .unwrap();
}

#[test]
fn test_cloexec() {
    let wasm = run_build_script(file!(), "cloexec").unwrap();
    run_with_arg(&wasm, "flag_tests");
    run_with_arg(&wasm, "exec_tests");
    run_with_arg(&wasm, "pipe2_cloexec_test");
}

#[test]
fn test_cross_fs_rename() {
    let wasm = run_build_script(file!(), "cross-fs-rename").unwrap();
    let temp1 = tempfile::tempdir().unwrap();
    let temp2 = tempfile::tempdir().unwrap();
    assert_stdout_zero(&wasm, |runner| {
        runner.with_mapped_directories([
            MappedDirectory {
                guest: "/temp1".to_string(),
                host: temp1.path().to_path_buf(),
            },
            MappedDirectory {
                guest: "/temp2".to_string(),
                host: temp2.path().to_path_buf(),
            },
        ]);
    });
}

#[test]
fn test_fork() {
    let wasm = run_build_script(file!(), "fork").unwrap();
    run_with_arg(&wasm, "failing_exec");
    run_with_arg(&wasm, "cloexec");
}

#[test]
fn test_popen() {
    let wasm = run_build_script(file!(), "popen").unwrap();
    run_with_arg(&wasm, "posix_spawn_direct");
    run_with_arg(&wasm, "pipe2_cloexec");
    run_with_arg(&wasm, "popen");
}

#[test]
fn test_legacy_proc_exec() {
    let wasm = run_build_script(file!(), "legacy-proc-exec").unwrap();
    let test_dir = wasm.parent().unwrap();
    assert_stdout_zero(&wasm, |runner| {
        runner.with_mapped_directories([MappedDirectory {
            guest: "/code".to_string(),
            host: test_dir.to_path_buf(),
        }]);
    });
}

#[test]
fn test_legacy_proc_exec2() {
    let wasm = run_build_script(file!(), "legacy-proc-exec2").unwrap();
    let test_dir = wasm.parent().unwrap();
    assert_stdout_zero(&wasm, |runner| {
        runner.with_mapped_directories([MappedDirectory {
            guest: "/code".to_string(),
            host: test_dir.to_path_buf(),
        }]);
    });
}

wasm_test!(
    test_share_tmp_after_fork,
    "share-tmp-after-fork",
    stdout = "0"
);

#[test]
fn test_share_tmp_after_proc_exec() {
    let wasm = run_build_script(file!(), "share-tmp-after-proc-exec").unwrap();
    let test_dir = wasm.parent().unwrap();
    assert_stdout_zero(&wasm, |runner| {
        runner.with_mapped_directories([MappedDirectory {
            guest: "/code".to_string(),
            host: test_dir.to_path_buf(),
        }]);
    });
}

#[test]
fn test_share_tmp_after_proc_exec2() {
    let wasm = run_build_script(file!(), "share-tmp-after-proc-exec2").unwrap();
    let test_dir = wasm.parent().unwrap();
    assert_stdout_zero(&wasm, |runner| {
        runner.with_mapped_directories([MappedDirectory {
            guest: "/code".to_string(),
            host: test_dir.to_path_buf(),
        }]);
    });
}

fn run_vfork_variant(wasm: &PathBuf, arg: &str) {
    run_with_arg_in_home(wasm, arg);
}

fn run_vfork_suite(wasm: &PathBuf) {
    run_vfork_variant(wasm, "successful_exec");
    run_vfork_variant(wasm, "successful_execlp");
    run_vfork_variant(wasm, "failing_exec");
    run_vfork_variant(wasm, "cloexec");
    run_vfork_variant(wasm, "nested_vfork");
    run_vfork_variant(wasm, "exiting_child");
    run_vfork_variant(wasm, "trapping_child");
}

#[test]
fn test_vfork_asyncify() {
    let wasm = run_build_script(file!(), "vfork").unwrap();
    run_vfork_suite(&wasm);
}

#[test]
fn test_vfork_eh() {
    let wasm = run_build_script(file!(), "vfork").unwrap();
    run_vfork_suite(&wasm.with_file_name("main-eh.wasm"));
}

#[test]
#[ignore = "undefined behavior in legacy fixture"]
fn test_vfork_exit_before_exec() {
    let wasm = run_build_script(file!(), "vfork").unwrap();
    run_vfork_variant(&wasm, "exit_before_exec");
}

#[test]
#[ignore = "undefined behavior in legacy fixture"]
fn test_vfork_trap_before_exec() {
    let wasm = run_build_script(file!(), "vfork").unwrap();
    run_vfork_variant(&wasm, "trap_before_exec");
}
