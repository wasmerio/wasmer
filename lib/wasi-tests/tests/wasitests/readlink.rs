#[test]
fn test_readlink() {
    assert_wasi_output!(
        "../../wasitests/readlink.wasm",
        "readlink",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/readlink.out"
    );
}
