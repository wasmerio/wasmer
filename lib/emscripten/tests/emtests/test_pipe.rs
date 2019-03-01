use crate::emtests::_common::assert_emscripten_output_f;

#[test]
fn test_pipe() {
    let wasm_bytes: &[u8] = include_bytes!("../../emtests/test_pipe.wasm");
    let expected_str = include_str!("../../emtests/test_pipe.out").to_string();
    assert_emscripten_output_f(wasm_bytes, "test_pipe", expected_str);
}
