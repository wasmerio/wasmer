#![allow(warnings, dead_code)]

#[cfg(test)]
mod tests {

    struct SpecFailure {
        file: String,
        line: u64,
        kind: String,
        message: String,
    }

    struct TestReport {
        failures: Vec<SpecFailure>,
        passed: u32,
        failed: u32,
    }

    impl TestReport {
        pub fn countPassed(&mut self) {
            self.passed += 1;
        }

        pub fn hasFailures(&self) -> bool {
            self.failed > 0
        }

        pub fn addFailure(&mut self, failure: SpecFailure) {
            self.failed += 1;
            self.failures.push(failure);
        }

        pub fn print_report(&self) {
            //            println!("total tests: {}", self.passed + self.failed);
            //            println!("passed: {}", self.passed);
            //            println!("failed: {}", self.failed);
            println!("failures:");
            for failure in self.failures.iter() {
                println!(
                    "    {:?} {:?} {:?} {:?}",
                    failure.file, failure.line, failure.kind, failure.message
                );
            }
        }
    }

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

    fn parse_and_run(path: &PathBuf) -> Result<TestReport, String> {
        let mut test_report = TestReport {
            failures: vec![],
            passed: 0,
            failed: 0,
        };

        // TODO Collect results
        // TODO Add more assertions
        // TODO
        // TODO Allow running WAST &str directly
        let source = fs::read(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let mut features = wabt::Features::new();
        features.enable_simd();
        let mut parser: ScriptParser =
            ScriptParser::from_source_and_name_with_features(&source, filename, features)
                .expect(&format!("Failed to parse script {}", &filename));

        let parse_result = parser.next();
        use std::panic;
        while let Some(Command { kind, line }) =
            parser.next().map_err(|e| format!("Parse err: {:?}", e))?
        {
            let mut instance: Option<Instance> = None;
            // println!("line: {:?}", line);
            match kind {
                CommandKind::Module { module, name } => {
                    println!("Module");
                    let result = panic::catch_unwind(|| {
                        let module =
                            wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                                .expect("WASM can't be compiled");
                        let i = module
                            .instantiate(&ImportObject::new())
                            .expect("WASM can't be instantiated");
                        i
                    });
                    match result {
                        Err(e) => {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "Module"),
                                message: format!("caught panic {:?}", e),
                            });
                            instance = None;
                        }
                        Ok(i) => {
                            instance = Some(i);
                        }
                    }
                }
                CommandKind::AssertReturn { action, expected } => {
                    match action {
                        Action::Invoke {
                            module,
                            field,
                            args,
                        } => {
                            if instance.is_none() {
                                test_report.addFailure(SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{:?}", "AssertReturn"),
                                    message: format!("No instance avaiable"),
                                });
                            } else {

                            }
                            //                            let params: Vec<Value> = args.iter().cloned().map(|x| x.into()).collect();
                            //                            instance.call(field, )
                        }
                        _ => println!("unexpected action"),
                    }
                    //                    println!("in assert return");
                }
                CommandKind::AssertReturnCanonicalNan { action } => {
                    println!("AssertReturnCanonicalNan")
                }
                CommandKind::AssertReturnArithmeticNan { action } => {
                    println!("AssertReturnArithmeticNan")
                }
                CommandKind::AssertTrap { action, message } => println!("AssertTrap"),
                CommandKind::AssertInvalid { module, message } => {
                    println!("AssertInvalid");
                    let result = panic::catch_unwind(|| {
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                    });
                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg }) = module {
                                test_report.countPassed();
                                println!("expected: {:?}", message);
                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg }) = module {
                                test_report.countPassed();
                                println!("validation expected: {:?}", message);
                                println!("validation actual: {:?}", msg);
                            } else {
                                test_report.addFailure(SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{:?}", "AssertInvalid"),
                                    message: "Should be invalid".to_string(),
                                });
                            }
                        }
                        Err(p) => {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertInvalid"),
                                message: format!("caught panic {:?}", p),
                            });
                        }
                    }
                }
                CommandKind::AssertMalformed { module, message } => {
                    println!("AssertMalformed");

                    let result = panic::catch_unwind(|| {
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                    });

                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg }) = module {
                                test_report.countPassed();
                                println!("expected: {:?}", message);
                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg }) = module {
                                test_report.countPassed();
                                println!("validation expected: {:?}", message);
                                println!("validation actual: {:?}", msg);
                            } else {
                                println!("Should be malformed");
                            }
                        }
                        Err(p) => {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertMalformed"),
                                message: format!("caught panic {:?}", p),
                            });
                        }
                    }
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
        test_report.print_report();
        Ok(test_report)
    }

    #[test]
    fn test_run_spectests() {
        let mut success = true;
        let mut test_reports = vec![];

        for test in TESTS.iter() {
            let test_name = test.split("/").last().unwrap().split(".").next().unwrap();
            //            println!("Running:  {:?} =============================>", test_name);
            let mut wast_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            wast_path.push(test);
            let result = parse_and_run(&wast_path);
            match result {
                Ok(test_report) => {
                    if test_report.hasFailures() {
                        success = false
                    }
                    test_reports.push(test_report);
                }
                Err(e) => {
                    success = false;
                    println!("Unexpected test run error: {:?}", e)
                }
            }
        }

        // Print summary
        let mut failures = vec![];
        let mut total_passed = 0;
        let mut total_failed = 0;
        for mut test_report in test_reports.into_iter() {
            total_passed += test_report.passed;
            total_failed += test_report.failed;
            failures.append(&mut test_report.failures);
        }

        println!("");
        println!("");
        println!("Spec tests summary report: ");
        println!("total: {}", total_passed + total_failed);
        println!("passed: {}", total_passed);
        println!("failed: {}", total_failed);
        for failure in failures.iter() {
            println!(
                "    {:?} {:?} {:?} {:?}",
                failure.file, failure.line, failure.kind, failure.message
            );
        }
        println!("");
        println!("");
        assert!(success, "tests passed")
    }

}
