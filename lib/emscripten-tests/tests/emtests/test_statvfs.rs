#[test]
#[ignore]
fn test_test_statvfs() {
    assert_emscripten_output!(
        "../../emtests/test_statvfs.wasm",
        "test_statvfs",
        vec![],
        "../../emtests/test_statvfs.out"
    );
}
