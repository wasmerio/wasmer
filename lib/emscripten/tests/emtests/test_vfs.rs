use crate::emtests::_common::assert_emscripten_output;

#[test]
fn test_vfs() {
    let wasm_bytes = include_bytes!("../../emtests/test_vfs_bundled.wasm");
    let expected_str = include_str!("../../emtests/test_vfs.out");
    assert_emscripten_output(wasm_bytes, expected_str);
}
