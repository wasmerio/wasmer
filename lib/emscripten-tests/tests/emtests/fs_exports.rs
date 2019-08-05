#[test]
#[ignore]
fn test_fs_exports() {
    assert_emscripten_output!(
        "../../emtests/FS_exports.wasm",
        "fs_exports",
        vec![],
        "../../emtests/FS_exports.txt"
    );
}
