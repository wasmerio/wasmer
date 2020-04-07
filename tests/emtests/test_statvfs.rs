#[test]
#[ignore]
fn test_test_statvfs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_statvfs.wasm",
        "test_statvfs",
        vec![],
        "../emscripten_resources/emtests/test_statvfs.out"
    );
}
