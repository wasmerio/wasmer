#[test]
#[ignore]
fn test_fs_exports() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/FS_exports.wasm",
        "fs_exports",
        vec![],
        "../emscripten_resources/emtests/FS_exports.txt"
    );
}
