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

        // TODO Add more assertions
        // TODO Allow running WAST &str directly
        // TODO Check all tests are up to date
        // TODO Files could be run with multiple threads
        let source = fs::read(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let mut features = wabt::Features::new();
        features.enable_simd();
        let mut parser: ScriptParser =
            ScriptParser::from_source_and_name_with_features(&source, filename, features)
                .expect(&format!("Failed to parse script {}", &filename));

        use std::panic;
        let mut instance: Option<Instance> = None;

        while let Some(Command { kind, line }) =
            parser.next().map_err(|e| format!("Parse err: {:?}", e))?
        {
            //            println!("line: {:?}", line);
            match kind {
                CommandKind::Module { module, name } => {
                    //                    println!("Module");
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
                            if (&instance).is_none() {
                                test_report.addFailure(SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{:?}", "AssertReturn"),
                                    message: format!("No instance available"),
                                });
                            } else {
                                let params: Vec<wasmer_runtime_core::types::Value> =
                                    args.iter().cloned().map(|x| convert_value(x)).collect();
                                let call_result =
                                    instance.as_ref().unwrap().call(&field, &params[..]);
                                match call_result {
                                    Err(e) => {
                                        test_report.addFailure(SpecFailure {
                                            file: filename.to_string(),
                                            line,
                                            kind: format!("{:?}", "AssertReturn"),
                                            message: format!("Call failed {:?}", e),
                                        });
                                    }
                                    Ok(values) => {
                                        for (i, v) in values.iter().enumerate() {
                                            let expected_value =
                                                convert_value(*expected.get(i).unwrap());
                                            if (*v != expected_value) {
                                                test_report.addFailure(SpecFailure {
                                                    file: filename.to_string(),
                                                    line,
                                                    kind: format!("{:?}", "AssertReturn"),
                                                    message: format!(
                                                        "result {:?} ({:?}) does not match expected {:?} ({:?})",
                                                        v, to_hex(v.clone()), expected_value, to_hex(expected_value.clone())
                                                    ),
                                                });
                                            } else {
                                                test_report.countPassed();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => println!("unexpected action in assert return"),
                    }
                    //                    println!("in assert return");
                }
                CommandKind::AssertReturnCanonicalNan { action } => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        if (&instance).is_none() {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertReturnCanonicalNan"),
                                message: format!("No instance available"),
                            });
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.as_ref().unwrap().call(&field, &params[..]);
                            match call_result {
                                Err(e) => {
                                    test_report.addFailure(SpecFailure {
                                        file: filename.to_string(),
                                        line,
                                        kind: format!("{:?}", "AssertReturnCanonicalNan"),
                                        message: format!("Call failed {:?}", e),
                                    });
                                }
                                Ok(values) => {
                                    for (i, v) in values.iter().enumerate() {
                                        if is_canonical_nan(v.clone()) {
                                            test_report.countPassed();
                                        } else {
                                            test_report.addFailure(SpecFailure {
                                                file: filename.to_string(),
                                                line,
                                                kind: format!("{:?}", "AssertReturnCanonicalNan"),
                                                message: format!(
                                                    "value is not canonical nan {:?}",
                                                    v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => panic!("unexpected action in assert return canonical nan"),
                },
                CommandKind::AssertReturnArithmeticNan { action } => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        if (&instance).is_none() {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertReturnArithmeticNan"),
                                message: format!("No instance available"),
                            });
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.as_ref().unwrap().call(&field, &params[..]);
                            match call_result {
                                Err(e) => {
                                    test_report.addFailure(SpecFailure {
                                        file: filename.to_string(),
                                        line,
                                        kind: format!("{:?}", "AssertReturnArithmeticNan"),
                                        message: format!("Call failed {:?}", e),
                                    });
                                }
                                Ok(values) => {
                                    for (i, v) in values.iter().enumerate() {
                                        if is_arithmetic_nan(v.clone()) {
                                            test_report.countPassed();
                                        } else {
                                            test_report.addFailure(SpecFailure {
                                                file: filename.to_string(),
                                                line,
                                                kind: format!("{:?}", "AssertReturnArithmeticNan"),
                                                message: format!(
                                                    "value is not arithmetic nan {:?}",
                                                    v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => panic!("unexpected action in assert return arithmetic nan"),
                },
                CommandKind::AssertTrap { action, message } => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        if (&instance).is_none() {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertTrap"),
                                message: format!("No instance available"),
                            });
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.as_ref().unwrap().call(&field, &params[..]);
                            use wasmer_runtime_core::error::{CallError, RuntimeError};
                            match call_result {
                                Err(e) => {
                                    match e {
                                        CallError::Resolve(_) => {
                                            test_report.addFailure(SpecFailure {
                                                file: filename.to_string(),
                                                line,
                                                kind: format!("{:?}", "AssertTrap"),
                                                message: format!("expected trap, got {:?}", e),
                                            });
                                        }
                                        CallError::Runtime(r) => {
                                            match r {
                                                RuntimeError::Trap { .. } => {
                                                    // TODO assert message?
                                                    test_report.countPassed()
                                                }
                                                RuntimeError::Error { .. } => {
                                                    test_report.addFailure(SpecFailure {
                                                        file: filename.to_string(),
                                                        line,
                                                        kind: format!("{:?}", "AssertTrap"),
                                                        message: format!(
                                                            "expected trap, got Runtime:Error {:?}",
                                                            r
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(values) => {
                                    test_report.addFailure(SpecFailure {
                                        file: filename.to_string(),
                                        line,
                                        kind: format!("{:?}", "AssertTrap"),
                                        message: format!("expected trap, got {:?}", values),
                                    });
                                }
                            }
                        }
                    }
                    _ => println!("unexpected action"),
                },
                CommandKind::AssertInvalid { module, message } => {
                    //                    println!("AssertInvalid");
                    let result = panic::catch_unwind(|| {
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                    });
                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg }) = module {
                                test_report.countPassed();
                            //                                println!("expected: {:?}", message);
                            //                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg }) = module {
                                test_report.countPassed();
                            //                                println!("validation expected: {:?}", message);
                            //                                println!("validation actual: {:?}", msg);
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
                    //                    println!("AssertMalformed");

                    let result = panic::catch_unwind(|| {
                        wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                    });

                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg }) = module {
                                test_report.countPassed();
                            //                                println!("expected: {:?}", message);
                            //                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg }) = module {
                                test_report.countPassed();
                            //                                println!("validation expected: {:?}", message);
                            //                                println!("validation actual: {:?}", msg);
                            } else {
                                test_report.addFailure(SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{:?}", "AssertMalformed"),
                                    message: format!("should be malformed"),
                                });
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
                CommandKind::AssertExhaustion { action, message } => {
                    println!("AssertExhaustion not yet implemented")
                }
                CommandKind::AssertUnlinkable { module, message } => {
                    println!("AssertUnlinkable {:? }{:?}", filename, line);
                    let result = panic::catch_unwind(|| {
                        let module =
                            wasmer_runtime_core::compile_with(&module.into_vec(), &get_compiler())
                                .expect("WASM can't be compiled");
                        module.instantiate(&ImportObject::new())
                    });
                    match result {
                        Err(e) => {
                            test_report.addFailure(SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{:?}", "AssertUnlinkable"),
                                message: format!("caught panic {:?}", e),
                            });
                        }
                        Ok(result) => match result {
                            Ok(_) => {
                                test_report.addFailure(SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{:?}", "AssertUnlinkable"),
                                    message: format!("instantiate successful, expected unlinkable"),
                                });
                            }
                            Err(e) => match e {
                                wasmer_runtime_core::error::Error::LinkError(_) => {
                                    test_report.countPassed();
                                }
                                _ => {
                                    test_report.addFailure(SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{:?}", "AssertUnlinkable"),
                                        message: format!("expected link error, got {:?}", e),
                                    });
                                }
                            },
                        },
                    }
                    println!("AssertUnlinkable Done");
                }
                CommandKind::Register { name, as_name } => println!("Register not implemented {:?} {:?}", filename, line),
                CommandKind::PerformAction(ref action) => println!("PerformAction not implemented {:?} {:?}", filename, line),
                _ => panic!("unknown wast command"),
            }
        }
        Ok(test_report)
    }

    fn is_canonical_nan(val: wasmer_runtime_core::types::Value) -> bool {
        match val {
            wasmer_runtime_core::types::Value::F32(x) => x.is_canonical_nan(),
            wasmer_runtime_core::types::Value::F64(x) => x.is_canonical_nan(),
            _ => panic!("value is not a float {:?}", val),
        }
    }

    fn is_arithmetic_nan(val: wasmer_runtime_core::types::Value) -> bool {
        match val {
            wasmer_runtime_core::types::Value::F32(x) => x.is_quiet_nan(),
            wasmer_runtime_core::types::Value::F64(x) => x.is_quiet_nan(),
            _ => panic!("value is not a float {:?}", val),
        }
    }

    fn convert_value(other: Value<f32, f64>) -> wasmer_runtime_core::types::Value {
        match other {
            Value::I32(v) => wasmer_runtime_core::types::Value::I32(v),
            Value::I64(v) => wasmer_runtime_core::types::Value::I64(v),
            Value::F32(v) => wasmer_runtime_core::types::Value::F32(v),
            Value::F64(v) => wasmer_runtime_core::types::Value::F64(v),
            Value::V128(v) => wasmer_runtime_core::types::Value::V128(v),
        }
    }

    fn to_hex(v: wasmer_runtime_core::types::Value) -> String {
        match v {
            wasmer_runtime_core::types::Value::I32(v) => format!("{:#x}", v),
            wasmer_runtime_core::types::Value::I64(v) => format!("{:#x}", v),
            wasmer_runtime_core::types::Value::F32(v) => format!("{:#x}", v.to_bits()),
            wasmer_runtime_core::types::Value::F64(v) => format!("{:#x}", v.to_bits()),
            wasmer_runtime_core::types::Value::V128(v) => format!("{:#x}", v),
        }
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

    /// Bit pattern of an f32 value:
    ///     1-bit sign + 8-bit mantissa + 23-bit exponent = 32 bits
    ///
    /// Bit pattern of an f64 value:
    ///     1-bit sign + 11-bit mantissa + 52-bit exponent = 64 bits
    ///
    /// NOTE: On some old platforms (PA-RISC, some MIPS) quiet NaNs (qNaN) have
    /// their mantissa MSB unset and set for signaling NaNs (sNaN).
    ///
    /// Links:
    ///     * https://en.wikipedia.org/wiki/Floating-point_arithmetic
    ///     * https://github.com/WebAssembly/spec/issues/286
    ///     * https://en.wikipedia.org/wiki/NaN
    ///
    pub trait NaNCheck {
        fn is_quiet_nan(&self) -> bool;
        fn is_canonical_nan(&self) -> bool;
    }

    impl NaNCheck for f32 {
        /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
        fn is_quiet_nan(&self) -> bool {
            let bit_mask = 0b1 << 22; // Used to check if 23rd bit is set, which is MSB of the mantissa
            self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
        }

        /// For a NaN to be canonical, its mantissa bits must all be unset
        fn is_canonical_nan(&self) -> bool {
            let bit_mask: u32 = 0b1____0000_0000____011_1111_1111_1111_1111_1111;
            let masked_value = self.to_bits() ^ bit_mask;
            masked_value == 0xFFFF_FFFF || masked_value == 0x7FFF_FFFF
        }
    }

    impl NaNCheck for f64 {
        /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
        fn is_quiet_nan(&self) -> bool {
            let bit_mask = 0b1 << 51; // Used to check if 52st bit is set, which is MSB of the mantissa
            self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
        }

        /// For a NaN to be canonical, its mantissa bits must all be unset
        fn is_canonical_nan(&self) -> bool {
            let bit_mask: u64 =
                0b1____000_0000_0000____0111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111;
            let masked_value = self.to_bits() ^ bit_mask;
            masked_value == 0x7FFF_FFFF_FFFF_FFFF || masked_value == 0xFFF_FFFF_FFFF_FFFF
        }
    }

}
