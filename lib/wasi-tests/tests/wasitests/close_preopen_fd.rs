#[test]
fn test_close_preopen_fd() {
    assert_wasi_output!(
        "../../wasitests/close_preopen_fd.wasm",
        "close_preopen_fd",
        vec![],
        vec![(
            "hamlet".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/close_preopen_fd.out"
    );
}
