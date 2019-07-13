#[test]
fn test_fseek() {
    assert_wasi_output!(
        "../../wasitests/fseek.wasm",
        "fseek",
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/fseek.out"
    );
}
