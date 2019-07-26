#![allow(warnings, dead_code)]

#[cfg(test)]
mod tests {

    const TESTS: &[&str] = &[
        "spectests/address.wast",
        "spectests/align.wast",
        "spectests/binary.wast",
        "spectests/block.wast",
        "spectests/br.wast",
        "spectests/br_if.wast",
        "spectests/br_table.wast",
        "spectests/break_drop.wast",
        "spectests/call.wast",
        "spectests/call_indirect.wast",
        "spectests/comments.wast",
        "spectests/const_.wast",
        "spectests/conversions.wast",
        "spectests/custom.wast",
        "spectests/data.wast",
        "spectests/elem.wast",
        "spectests/endianness.wast",
        "spectests/exports.wast",
        "spectests/f32_.wast",
        "spectests/f32_bitwise.wast",
        "spectests/f32_cmp.wast",
        "spectests/f64_.wast",
        "spectests/f64_bitwise.wast",
        "spectests/f64_cmp.wast",
        "spectests/fac.wast",
        "spectests/float_exprs.wast",
        "spectests/float_literals.wast",
        "spectests/float_memory.wast",
        "spectests/float_misc.wast",
        "spectests/forward.wast",
        "spectests/func.wast",
        "spectests/func_ptrs.wast",
        "spectests/get_local.wast",
        "spectests/globals.wast",
        "spectests/i32_.wast",
        "spectests/i64_.wast",
        "spectests/if_.wast",
        "spectests/int_exprs.wast",
        "spectests/int_literals.wast",
        "spectests/labels.wast",
        "spectests/left_to_right.wast",
        "spectests/loop_.wast",
        "spectests/memory.wast",
        "spectests/memory_grow.wast",
        "spectests/memory_redundancy.wast",
        "spectests/memory_trap.wast",
        "spectests/nop.wast",
        "spectests/return_.wast",
        "spectests/select.wast",
        "spectests/set_local.wast",
        "spectests/stack.wast",
        "spectests/start.wast",
        "spectests/store_retval.wast",
        "spectests/switch.wast",
        "spectests/tee_local.wast",
        "spectests/token.wast",
        "spectests/traps.wast",
        "spectests/typecheck.wast",
        "spectests/types.wast",
        "spectests/unwind.wast",
        #[cfg(feature = "llvm")]
        "spectests/simd.wast",
        #[cfg(feature = "llvm")]
        "spectests/simd_binaryen.wast",
    ];

    use wasmer_runtime_core::backend::Compiler;

    #[cfg(feature = "clif")]
    fn get_compiler() -> impl Compiler {
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[cfg(feature = "llvm")]
    fn get_compiler() -> impl Compiler {
        use wasmer_llvm_backend::LLVMCompiler;
        LLVMCompiler::new()
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler() -> impl Compiler {
        use wasmer_singlepass_backend::SinglePassCompiler;
        SinglePassCompiler::new()
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler() -> impl Compiler {
        panic!("compiler not specified, activate a compiler via features");
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    use std::path::PathBuf;
    use std::{env, fs, io::Write};
    use wabt::script::{Action, Command, CommandKind, ScriptParser, Value};
    use wasmer_runtime_core::error::CompileError;
    use wasmer_runtime_core::import::ImportObject;
    use wasmer_runtime_core::Instance;

    fn parse_and_run(path: &PathBuf) -> Result<(), String> {
        let source = fs::read(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let mut features = wabt::Features::new();
        features.enable_simd();
        let mut parser: ScriptParser =
            ScriptParser::from_source_and_name_with_features(&source, filename, features)
                .expect(&format!("Failed to parse script {}", &filename));

        let parse_result = parser.next();

        while let Some(Command { kind, line }) =
            parser.next().map_err(|e| format!("Parse err: {:?}", e))?
        {
            let mut instance: Option<Instance> = None;
            println!("line: {:?}", line);
            match kind {
                CommandKind::Module { module, name } => {
                    println!("in module");
                    let module =
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                            .expect("WASM can't be compiled");
                    let i = module
                        .instantiate(&ImportObject::new())
                        .expect("WASM can't be instantiated");
                    instance = Some(i);
                }
                CommandKind::AssertReturn { action, expected } => {
                    println!("in assert return");
                }
                CommandKind::AssertReturnCanonicalNan { action } => {
                    println!("AssertReturnCanonicalNan")
                }
                CommandKind::AssertReturnArithmeticNan { action } => {
                    println!("AssertReturnArithmeticNan")
                }
                CommandKind::AssertTrap { action, message } => println!("AssertTrap"),
                CommandKind::AssertInvalid { module, message } => println!("AssertInvalid"),
                CommandKind::AssertMalformed { module, message } => {
                    let module =
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler());
                    if let Err(CompileError::InternalError { msg }) = module {
                        println!("expected: {:?}", message);
                        println!("actual: {:?}", msg);
                    } else if let Err(CompileError::ValidationError { msg }) = module {
                        println!("expected: {:?}", message);
                        println!("actual: {:?}", msg);
                    } else {
                        println!("Should be malformed")
                    }
                    println!("AssertMalformed")
                }
                CommandKind::AssertUninstantiable { module, message } => {
                    println!("AssertUninstantiable")
                }
                CommandKind::AssertExhaustion { action, message } => println!("AssertExhaustion"),
                CommandKind::AssertUnlinkable { module, message } => println!("AssertUnlinkable"),
                CommandKind::Register { name, as_name } => println!("Register"),
                CommandKind::PerformAction(ref action) => println!("PerformAction"),
                _ => panic!("unknown wast command"),
            }
        }
        Ok(())
    }

    #[test]
    fn test_run_spectests() {
        let mut success = true;

        for test in TESTS.iter() {
            let test_name = test.split("/").last().unwrap().split(".").next().unwrap();
            println!("Running:  {:?} =============================>", test_name);
            let mut wast_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            wast_path.push(test);
            let result = parse_and_run(&wast_path);
            if let Ok(()) = result {
                println!("Success: {:?}.wast", test_name);
            } else {
                println!("Failed: {:?}.wast", test_name);
            }
        }

        assert!(success, "tests passed")
    }

}
