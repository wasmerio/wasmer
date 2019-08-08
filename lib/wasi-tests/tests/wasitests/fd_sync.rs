#[test]
fn test_fd_sync() {
    assert_wasi_output!(
        "../../wasitests/fd_sync.wasm",
        "fd_sync",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/temp")
        ),],
        vec![],
        "../../wasitests/fd_sync.out"
    );
}
