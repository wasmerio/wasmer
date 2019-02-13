use wasmer_runtime_core::{
    backend::{Compiler, Token},
    error::CompileError,
    module::ModuleInner,
};

mod code;
mod intrinsics;
mod read_info;
mod state;

pub struct LLVMCompiler {
    _private: (),
}

impl LLVMCompiler {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Compiler for LLVMCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> Result<ModuleInner, CompileError> {
        let (_info, _code_reader) = read_info::read_module(wasm).unwrap();

        unimplemented!()
    }
}

#[test]
fn test_read_module() {
    use wabt::wat2wasm;
    let wat = r#"
        (module
        (type $t0 (func (param i32) (result i32)))
        (type $t1 (func (result i32)))
        (func $foo (type $t0) (param i32) (result i32)
            get_local 0
            (if
                (then
                    i32.const 42
                    set_local 0
                )
                (else 
                    i32.const 24
                    set_local 0
                )
            )
            get_local 0
        ))
    "#;
    let wasm = wat2wasm(wat).unwrap();

    let (info, code_reader) = read_info::read_module(&wasm).unwrap();

    code::parse_function_bodies(&info, code_reader).unwrap();
}
