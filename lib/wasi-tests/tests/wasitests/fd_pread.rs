#[test]
fn test_fd_pread() {
    assert_wasi_output!(
        "../../wasitests/fd_pread.wasm",
        "fd_pread",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/fd_pread.out"
    );
}
