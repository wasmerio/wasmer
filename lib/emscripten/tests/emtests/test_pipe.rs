use crate::emtests::_common::assert_emscripten_output;

#[test]
fn test_pipe() {
    let wasm_bytes = include_bytes!("../../emtests/test_pipe.wasm");
    let expected_str = include_str!("../../emtests/test_pipe.out");
    assert_emscripten_output(wasm_bytes, expected_str);
}
