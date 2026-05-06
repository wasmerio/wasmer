use super::{run_build_script, run_wasm_with_runner_config};

wasm_test!(test_pipe_send_recv_compat, "pipe_send_recv_compat", stdout = "pipe send/recv works");

wasm_test!(
    test_nonblocking_connect,
    "nonblocking-connect",
    stdout = "nonblocking connect returned immediately"
);

// https://github.com/wasmerio/wasmer/issues/6366
wasm_test!(
    #[ignore = "flaky test (#6366)"]
    test_socket_pair,
    "socket-pair"
);

#[test]
fn test_udp() {
    let wasm = run_build_script(file!(), "udp").unwrap();
    for arg in ["addr-reuse", "ipv6", "autobind-connect", "autobind-sendto"] {
        let result = run_wasm_with_runner_config(&wasm, wasm.parent().unwrap(), |runner| {
            runner.with_args([arg]);
        })
        .unwrap();
        assert_eq!(
            result.exit_code,
            Some(0),
            "case={arg}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
