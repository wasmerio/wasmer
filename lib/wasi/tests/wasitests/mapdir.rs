#[test]
fn test_mapdir() {
    assert_wasi_output!(
        "../../wasitests/mapdir.wasm",
        "mapdir",
        vec![],
        "../../wasitests/mapdir.out"
    );
}
