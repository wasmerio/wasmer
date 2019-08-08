#[test]
fn test_fd_allocate() {
    assert_wasi_output!(
        "../../wasitests/fd_allocate.wasm",
        "fd_allocate",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/temp")
        ),],
        vec![],
        "../../wasitests/fd_allocate.out"
    );
}
