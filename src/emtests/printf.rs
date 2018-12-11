use crate::apis::generate_emscripten_env;
use crate::webassembly::{instantiate, Export, Instance, start_instance};
use crate::common::stdio::StdioCapturer;


#[test]
fn test_printf() {
    let wasm_bytes = include_bytes!("../../emtests/printf.wasm");
    let import_object = generate_emscripten_env();
    let mut result_object = instantiate(wasm_bytes.to_vec(), import_object).expect("Not compiled properly");
    let mut capturer = StdioCapturer::new();
    start_instance(&result_object.module, &mut result_object.instance, "printf", vec![]);
    let output = capturer.end().0;
    println!("Captured {}", output);
    panic!();
}
