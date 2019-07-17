#[test]
fn test_mapdir() {
    assert_wasi_output!(
        "../../wasitests/mapdir.wasm",
        "mapdir",
        vec![],
        vec![(
            ".".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/mapdir.out"
    );
}
