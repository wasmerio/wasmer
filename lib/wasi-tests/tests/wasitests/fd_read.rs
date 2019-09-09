#[test]
fn test_fd_read() {
    assert_wasi_output!(
        "../../wasitests/fd_read.wasm",
        "fd_read",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/fd_read.out"
    );
}
