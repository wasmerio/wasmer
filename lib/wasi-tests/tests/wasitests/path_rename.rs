#[test]
fn test_path_rename() {
    assert_wasi_output!(
        "../../wasitests/path_rename.wasm",
        "path_rename",
        vec![],
        vec![(
            "temp".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/temp")
        ),],
        vec![],
        "../../wasitests/path_rename.out"
    );
}
