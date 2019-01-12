#[cfg(test)]
mod tests {

    use std::rc::Rc;
    use wabt::wat2wasm;
    use wasmer_clif_backend::CraneliftCompiler;
    use wasmer_runtime::import::Imports;

    // The semantics of stack overflow are documented at:
    // https://webassembly.org/docs/semantics/#stack-overflow
    #[test]
    #[ignore]
    fn test_stack_overflow() {
        let module_str = "(module
      (type (;0;) (func (result i64)))
      (func (;0;) (type 0) (result i64)
        i64.const 356)
      (func (;1;) (type 0) (result i64)
        i32.const 1
        call_indirect (type 0))
      (table (;0;) 2 anyfunc)
      (export \"type-i64\" (func 1))
      (elem (;0;) (i32.const 0) 0 1))
    ";
        let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
        let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new())
            .expect("WASM can't be compiled");
        let mut instance = module
            .instantiate(Rc::new(Imports::new()))
            .expect("WASM can't be instantiated");
        let result = instance.call("type-i64", &[]);
        assert!(
            result.is_err(),
            "should fail with error due to stack overflow"
        );
        // TODO The kind of error and message needs to be defined, not spec defined, maybe RuntimeError or RangeError
        if let Err(message) = result {
            assert!(!message.contains("segmentation violation"));
            assert!(!message.contains("bus error"));
        }
    }

}
