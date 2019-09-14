#![deny(
    bad_style,
    dead_code,
    unused_imports,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]

#[cfg(test)]
mod tests {

    // TODO fix spec failures
    // TODO fix panics and remove panic handlers
    // TODO do something with messages _message, message: _, msg: _
    // TODO consider git submodule for spectests? & separate dir for simd/extra tests
    // TODO cleanup refactor
    // TODO Files could be run with multiple threads
    // TODO Allow running WAST &str directly (E.g. for use outside of spectests)

    use std::rc::Rc;

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
        allowed_failure: u32,
    }

    impl TestReport {
        pub fn count_passed(&mut self) {
            self.passed += 1;
        }

        pub fn has_failures(&self) -> bool {
            self.failed > 0
        }

        pub fn add_failure(
            &mut self,
            failure: SpecFailure,
            testkey: &str,
            excludes: &HashMap<String, Exclude>,
        ) {
            if excludes.contains_key(testkey) {
                self.allowed_failure += 1;
                return;
            }
            let platform_key = format!("{}:{}", testkey, get_platform());
            if excludes.contains_key(&platform_key) {
                self.allowed_failure += 1;
                return;
            }
            self.failed += 1;
            self.failures.push(failure);
        }
    }

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

    #[cfg(feature = "clif")]
    fn get_compiler_name() -> &'static str {
        "clif"
    }

    #[cfg(feature = "llvm")]
    fn get_compiler_name() -> &'static str {
        "llvm"
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler_name() -> &'static str {
        "singlepass"
    }

    #[cfg(unix)]
    fn get_platform() -> &'static str {
        "unix"
    }

    #[cfg(windows)]
    fn get_platform() -> &'static str {
        "windows"
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler_name() -> &'static str {
        panic!("compiler not specified, activate a compiler via features");
        "unknown"
    }

    use glob::glob;
    use std::collections::HashMap;
    use std::fs;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use wabt::script::{Action, Command, CommandKind, ScriptParser, Value};
    use wasmer_runtime_core::backend::{Compiler, CompilerConfig, Features};
    use wasmer_runtime_core::error::CompileError;
    use wasmer_runtime_core::import::ImportObject;
    use wasmer_runtime_core::Instance;
    use wasmer_runtime_core::{
        export::Export,
        global::Global,
        import::LikeNamespace,
        memory::Memory,
        table::Table,
        types::{ElementType, MemoryDescriptor, TableDescriptor},
        units::Pages,
    };
    use wasmer_runtime_core::{func, imports, vm::Ctx};

    fn parse_and_run(
        path: &PathBuf,
        excludes: &HashMap<String, Exclude>,
    ) -> Result<TestReport, String> {
        let mut test_report = TestReport {
            failures: vec![],
            passed: 0,
            failed: 0,
            allowed_failure: 0,
        };

        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let backend = get_compiler_name();

        let platform = get_platform();
        let star_key = format!("{}:{}:*", backend, filename);
        let platform_star_key = format!("{}:{}:*:{}", backend, filename, platform);
        if (excludes.contains_key(&star_key) && *excludes.get(&star_key).unwrap() == Exclude::Skip)
            || (excludes.contains_key(&platform_star_key)
                && *excludes.get(&platform_star_key).unwrap() == Exclude::Skip)
        {
            return Ok(test_report);
        }

        let mut features = wabt::Features::new();
        features.enable_simd();
        features.enable_threads();
        let mut parser: ScriptParser =
            ScriptParser::from_source_and_name_with_features(&source, filename, features)
                .expect(&format!("Failed to parse script {}", &filename));

        use std::panic;
        let mut instance: Option<Rc<Instance>> = None;

        let mut named_modules: HashMap<String, Rc<Instance>> = HashMap::new();

        let mut registered_modules: HashMap<String, Rc<Instance>> = HashMap::new();
        //

        while let Some(Command { kind, line }) =
            parser.next().map_err(|e| format!("Parse err: {:?}", e))?
        {
            let test_key = format!("{}:{}:{}", backend, filename, line);
            let test_platform_key = format!("{}:{}:{}:{}", backend, filename, line, platform);
            // Use this line to debug which test is running
            println!("Running test: {}", test_key);

            if (excludes.contains_key(&test_key)
                && *excludes.get(&test_key).unwrap() == Exclude::Skip)
                || (excludes.contains_key(&test_platform_key)
                    && *excludes.get(&test_platform_key).unwrap() == Exclude::Skip)
            {
                //                println!("Skipping test: {}", test_key);
                continue;
            }

            match kind {
                CommandKind::Module { module, name } => {
                    //                    println!("Module");
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        let spectest_import_object =
                            get_spectest_import_object(&registered_modules);
                        let config = CompilerConfig {
                            features: Features {
                                simd: true,
                                threads: true,
                            },
                            ..Default::default()
                        };
                        let module = wasmer_runtime_core::compile_with_config(
                            &module.into_vec(),
                            &get_compiler(),
                            config,
                        )
                        .expect("WASM can't be compiled");
                        let i = module
                            .instantiate(&spectest_import_object)
                            .expect("WASM can't be instantiated");
                        i
                    }));
                    match result {
                        Err(e) => {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "Module"),
                                    message: format!("caught panic {:?}", e),
                                },
                                &test_key,
                                excludes,
                            );
                            instance = None;
                        }
                        Ok(i) => {
                            let i = Rc::new(i);
                            if name.is_some() {
                                named_modules.insert(name.unwrap(), Rc::clone(&i));
                            }
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
                            let instance: Option<&Instance> = match module {
                                Some(ref name) => {
                                    let i = named_modules.get(name);
                                    match i {
                                        Some(ins) => Some(ins.borrow()),
                                        None => None,
                                    }
                                }
                                None => match instance {
                                    Some(ref i) => Some(i.borrow()),
                                    None => None,
                                },
                            };
                            if instance.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertReturn"),
                                        message: format!("No instance available: {:?}", &module),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            } else {
                                let params: Vec<wasmer_runtime_core::types::Value> =
                                    args.iter().cloned().map(|x| convert_value(x)).collect();
                                let call_result = instance.unwrap().call(&field, &params[..]);
                                match call_result {
                                    Err(e) => {
                                        test_report.add_failure(
                                            SpecFailure {
                                                file: filename.to_string(),
                                                line,
                                                kind: format!("{}", "AssertReturn"),
                                                message: format!("Call failed {:?}", e),
                                            },
                                            &test_key,
                                            excludes,
                                        );
                                    }
                                    Ok(values) => {
                                        for (i, v) in values.iter().enumerate() {
                                            let expected_value =
                                                convert_wabt_value(*expected.get(i).unwrap());
                                            let v = convert_wasmer_value(v.clone());
                                            if v != expected_value {
                                                test_report.add_failure(SpecFailure {
                                                    file: filename.to_string(),
                                                    line,
                                                    kind: format!("{}", "AssertReturn"),
                                                    message: format!(
                                                        "result {:?} ({:?}) does not match expected {:?} ({:?})",
                                                        v, to_hex(v.clone()), expected_value, to_hex(expected_value.clone())
                                                    ),
                                                }, &test_key, excludes);
                                            } else {
                                                test_report.count_passed();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Action::Get { module, field } => {
                            let instance: Option<&Instance> = match module {
                                Some(ref name) => {
                                    let i = named_modules.get(name);
                                    match i {
                                        Some(ins) => Some(ins.borrow()),
                                        None => None,
                                    }
                                }
                                None => match instance {
                                    Some(ref i) => Some(i.borrow()),
                                    None => None,
                                },
                            };
                            if instance.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertReturn Get"),
                                        message: format!("No instance available {:?}", &module),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            } else {
                                let export: Export = instance
                                    .unwrap()
                                    .get_export(&field)
                                    .expect(&format!("missing global {:?}", &field));
                                match export {
                                    Export::Global(g) => {
                                        let value = g.get();
                                        let expected_value =
                                            convert_value(*expected.get(0).unwrap());
                                        if value == expected_value {
                                            test_report.count_passed();
                                        } else {
                                            test_report.add_failure(
                                                SpecFailure {
                                                    file: filename.to_string(),
                                                    line: line,
                                                    kind: format!("{}", "AssertReturn Get"),
                                                    message: format!(
                                                        "Expected Global {:?} got: {:?}",
                                                        expected_value, value
                                                    ),
                                                },
                                                &test_key,
                                                excludes,
                                            );
                                        }
                                    }
                                    _ => {
                                        test_report.add_failure(
                                            SpecFailure {
                                                file: filename.to_string(),
                                                line: line,
                                                kind: format!("{}", "AssertReturn Get"),
                                                message: format!("Expected Global"),
                                            },
                                            &test_key,
                                            excludes,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    //                    println!("in assert return");
                }
                CommandKind::AssertReturnCanonicalNan { action } => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        let instance: Option<&Instance> = match module {
                            Some(ref name) => {
                                let i = named_modules.get(name);
                                match i {
                                    Some(ins) => Some(ins.borrow()),
                                    None => None,
                                }
                            }
                            None => match instance {
                                Some(ref i) => Some(i.borrow()),
                                None => None,
                            },
                        };
                        if instance.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertReturnCanonicalNan"),
                                    message: format!("No instance available {:?}", &module),
                                },
                                &test_key,
                                excludes,
                            );
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.unwrap().call(&field, &params[..]);
                            match call_result {
                                Err(e) => {
                                    test_report.add_failure(
                                        SpecFailure {
                                            file: filename.to_string(),
                                            line,
                                            kind: format!("{}", "AssertReturnCanonicalNan"),
                                            message: format!("Call failed {:?}", e),
                                        },
                                        &test_key,
                                        excludes,
                                    );
                                }
                                Ok(values) => {
                                    for v in values.iter() {
                                        if is_canonical_nan(v.clone()) {
                                            test_report.count_passed();
                                        } else {
                                            test_report.add_failure(
                                                SpecFailure {
                                                    file: filename.to_string(),
                                                    line,
                                                    kind: format!(
                                                        "{:?}",
                                                        "AssertReturnCanonicalNan"
                                                    ),
                                                    message: format!(
                                                        "value is not canonical nan {:?}",
                                                        v
                                                    ),
                                                },
                                                &test_key,
                                                excludes,
                                            );
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
                        let instance: Option<&Instance> = match module {
                            Some(ref name) => {
                                let i = named_modules.get(name);
                                match i {
                                    Some(ins) => Some(ins.borrow()),
                                    None => None,
                                }
                            }
                            None => match instance {
                                Some(ref i) => Some(i.borrow()),
                                None => None,
                            },
                        };
                        if instance.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertReturnArithmeticNan"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                            );
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.unwrap().call(&field, &params[..]);
                            match call_result {
                                Err(e) => {
                                    test_report.add_failure(
                                        SpecFailure {
                                            file: filename.to_string(),
                                            line,
                                            kind: format!("{}", "AssertReturnArithmeticNan"),
                                            message: format!("Call failed {:?}", e),
                                        },
                                        &test_key,
                                        excludes,
                                    );
                                }
                                Ok(values) => {
                                    for v in values.iter() {
                                        if is_arithmetic_nan(v.clone()) {
                                            test_report.count_passed();
                                        } else {
                                            test_report.add_failure(
                                                SpecFailure {
                                                    file: filename.to_string(),
                                                    line,
                                                    kind: format!(
                                                        "{:?}",
                                                        "AssertReturnArithmeticNan"
                                                    ),
                                                    message: format!(
                                                        "value is not arithmetic nan {:?}",
                                                        v
                                                    ),
                                                },
                                                &test_key,
                                                excludes,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => panic!("unexpected action in assert return arithmetic nan"),
                },
                CommandKind::AssertTrap { action, message: _ } => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        let instance: Option<&Instance> = match module {
                            Some(ref name) => {
                                let i = named_modules.get(name);
                                match i {
                                    Some(ins) => Some(ins.borrow()),
                                    None => None,
                                }
                            }
                            None => match instance {
                                Some(ref i) => Some(i.borrow()),
                                None => None,
                            },
                        };
                        if instance.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertTrap"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                            );
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.unwrap().call(&field, &params[..]);
                            use wasmer_runtime_core::error::{CallError, RuntimeError};
                            match call_result {
                                Err(e) => {
                                    match e {
                                        CallError::Resolve(_) => {
                                            test_report.add_failure(
                                                SpecFailure {
                                                    file: filename.to_string(),
                                                    line,
                                                    kind: format!("{}", "AssertTrap"),
                                                    message: format!("expected trap, got {:?}", e),
                                                },
                                                &test_key,
                                                excludes,
                                            );
                                        }
                                        CallError::Runtime(r) => {
                                            match r {
                                                RuntimeError::Trap { .. } => {
                                                    // TODO assert message?
                                                    test_report.count_passed()
                                                }
                                                RuntimeError::Error { .. } => {
                                                    test_report.add_failure(
                                                        SpecFailure {
                                                            file: filename.to_string(),
                                                            line,
                                                            kind: format!("{}", "AssertTrap"),
                                                            message: format!(
                                                            "expected trap, got Runtime:Error {:?}",
                                                            r
                                                        ),
                                                        },
                                                        &test_key,
                                                        excludes,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(values) => {
                                    test_report.add_failure(
                                        SpecFailure {
                                            file: filename.to_string(),
                                            line,
                                            kind: format!("{}", "AssertTrap"),
                                            message: format!("expected trap, got {:?}", values),
                                        },
                                        &test_key,
                                        excludes,
                                    );
                                }
                            }
                        }
                    }
                    _ => println!("unexpected action"),
                },
                CommandKind::AssertInvalid { module, message: _ } => {
                    //                    println!("AssertInvalid");
                    let result = panic::catch_unwind(|| {
                        let config = CompilerConfig {
                            features: Features {
                                simd: true,
                                threads: true,
                            },
                            ..Default::default()
                        };
                        wasmer_runtime_core::compile_with_config(
                            &module.into_vec(),
                            &get_compiler(),
                            config,
                        )
                    });
                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg: _ }) = module {
                                test_report.count_passed();
                            //                                println!("expected: {:?}", message);
                            //                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg: _ }) = module {
                                test_report.count_passed();
                            //                                println!("validation expected: {:?}", message);
                            //                                println!("validation actual: {:?}", msg);
                            } else {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertInvalid"),
                                        message: "Should be invalid".to_string(),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            }
                        }
                        Err(p) => {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertInvalid"),
                                    message: format!("caught panic {:?}", p),
                                },
                                &test_key,
                                excludes,
                            );
                        }
                    }
                }
                CommandKind::AssertMalformed { module, message: _ } => {
                    //                    println!("AssertMalformed");

                    let result = panic::catch_unwind(|| {
                        let config = CompilerConfig {
                            features: Features {
                                simd: true,
                                threads: true,
                            },
                            ..Default::default()
                        };
                        wasmer_runtime_core::compile_with_config(
                            &module.into_vec(),
                            &get_compiler(),
                            config,
                        )
                    });

                    match result {
                        Ok(module) => {
                            if let Err(CompileError::InternalError { msg: _ }) = module {
                                test_report.count_passed();
                            //                                println!("expected: {:?}", message);
                            //                                println!("actual: {:?}", msg);
                            } else if let Err(CompileError::ValidationError { msg: _ }) = module {
                                test_report.count_passed();
                            //                                println!("validation expected: {:?}", message);
                            //                                println!("validation actual: {:?}", msg);
                            } else {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertMalformed"),
                                        message: format!("should be malformed"),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            }
                        }
                        Err(p) => {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertMalformed"),
                                    message: format!("caught panic {:?}", p),
                                },
                                &test_key,
                                excludes,
                            );
                        }
                    }
                }
                CommandKind::AssertUninstantiable {
                    module: _,
                    message: _,
                } => println!("AssertUninstantiable not yet implmented "),
                CommandKind::AssertExhaustion { action, message: _ } => {
                    match action {
                        Action::Invoke {
                            module,
                            field,
                            args,
                        } => {
                            let instance: Option<&Instance> = match module {
                                Some(ref name) => {
                                    let i = named_modules.get(name);
                                    match i {
                                        Some(ins) => Some(ins.borrow()),
                                        None => None,
                                    }
                                }
                                None => match instance {
                                    Some(ref i) => Some(i.borrow()),
                                    None => None,
                                },
                            };
                            if instance.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertExhaustion"),
                                        message: format!("No instance available"),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            } else {
                                let params: Vec<wasmer_runtime_core::types::Value> =
                                    args.iter().cloned().map(|x| convert_value(x)).collect();
                                let call_result = instance.unwrap().call(&field, &params[..]);
                                match call_result {
                                    Err(_e) => {
                                        // TODO is specific error required?
                                        test_report.count_passed();
                                    }
                                    Ok(values) => {
                                        test_report.add_failure(
                                            SpecFailure {
                                                file: filename.to_string(),
                                                line,
                                                kind: format!("{}", "AssertExhaustion"),
                                                message: format!(
                                                    "Expected call failure, got {:?}",
                                                    values
                                                ),
                                            },
                                            &test_key,
                                            excludes,
                                        );
                                    }
                                }
                            }
                        }
                        _ => println!("unexpected action in assert exhaustion"),
                    }
                }
                CommandKind::AssertUnlinkable { module, message: _ } => {
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        let spectest_import_object =
                            get_spectest_import_object(&registered_modules);
                        let config = CompilerConfig {
                            features: Features {
                                simd: true,
                                threads: true,
                            },
                            ..Default::default()
                        };
                        let module = wasmer_runtime_core::compile_with_config(
                            &module.into_vec(),
                            &get_compiler(),
                            config,
                        )
                        .expect("WASM can't be compiled");
                        module.instantiate(&spectest_import_object)
                    }));
                    match result {
                        Err(e) => {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertUnlinkable"),
                                    message: format!("caught panic {:?}", e),
                                },
                                &test_key,
                                excludes,
                            );
                        }
                        Ok(result) => match result {
                            Ok(_) => {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertUnlinkable"),
                                        message: format!(
                                            "instantiate successful, expected unlinkable"
                                        ),
                                    },
                                    &test_key,
                                    excludes,
                                );
                            }
                            Err(e) => match e {
                                wasmer_runtime_core::error::Error::LinkError(_) => {
                                    test_report.count_passed();
                                }
                                _ => {
                                    test_report.add_failure(
                                        SpecFailure {
                                            file: filename.to_string(),
                                            line: line,
                                            kind: format!("{}", "AssertUnlinkable"),
                                            message: format!("expected link error, got {:?}", e),
                                        },
                                        &test_key,
                                        excludes,
                                    );
                                }
                            },
                        },
                    }
                }
                CommandKind::Register { name, as_name } => {
                    let instance: Option<Rc<Instance>> = match name {
                        Some(ref name) => {
                            let i = named_modules.get(name);
                            match i {
                                Some(ins) => Some(Rc::clone(ins)),
                                None => None,
                            }
                        }
                        None => match instance {
                            Some(ref i) => Some(Rc::clone(i)),
                            None => None,
                        },
                    };

                    if let Some(ins) = instance {
                        registered_modules.insert(as_name, ins);
                    } else {
                        test_report.add_failure(
                            SpecFailure {
                                file: filename.to_string(),
                                line: line,
                                kind: format!("{}", "Register"),
                                message: format!("No instance available"),
                            },
                            &test_key,
                            excludes,
                        );
                    }
                }
                CommandKind::PerformAction(ref action) => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        let instance: Option<&Instance> = match module {
                            Some(ref name) => {
                                let i = named_modules.get(name);
                                match i {
                                    Some(ins) => Some(ins.borrow()),
                                    None => None,
                                }
                            }
                            None => match instance {
                                Some(ref i) => Some(i.borrow()),
                                None => None,
                            },
                        };

                        if instance.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "PerformAction"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                            );
                        } else {
                            let params: Vec<wasmer_runtime_core::types::Value> =
                                args.iter().cloned().map(|x| convert_value(x)).collect();
                            let call_result = instance.unwrap().call(&field, &params[..]);
                            match call_result {
                                Err(e) => {
                                    test_report.add_failure(
                                        SpecFailure {
                                            file: filename.to_string(),
                                            line,
                                            kind: format!("{}", "PerformAction"),
                                            message: format!("Call failed {:?}", e),
                                        },
                                        &test_key,
                                        excludes,
                                    );
                                }
                                Ok(_values) => {
                                    test_report.count_passed();
                                }
                            }
                        }
                    }
                    Action::Get { module, field } => println!(
                        "Action Get not implemented {:?} {:?} {:?} {:?}",
                        module, field, filename, line
                    ),
                },
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

    #[derive(Debug, Clone, Eq, PartialEq)]
    pub enum SpectestValue {
        I32(i32),
        I64(i64),
        F32(u32),
        F64(u64),
        V128(u128),
    }

    fn convert_wasmer_value(other: wasmer_runtime_core::types::Value) -> SpectestValue {
        match other {
            wasmer_runtime_core::types::Value::I32(v) => SpectestValue::I32(v),
            wasmer_runtime_core::types::Value::I64(v) => SpectestValue::I64(v),
            wasmer_runtime_core::types::Value::F32(v) => SpectestValue::F32(v.to_bits()),
            wasmer_runtime_core::types::Value::F64(v) => SpectestValue::F64(v.to_bits()),
            wasmer_runtime_core::types::Value::V128(v) => SpectestValue::V128(v),
        }
    }

    fn convert_wabt_value(other: Value<f32, f64>) -> SpectestValue {
        match other {
            Value::I32(v) => SpectestValue::I32(v),
            Value::I64(v) => SpectestValue::I64(v),
            Value::F32(v) => SpectestValue::F32(v.to_bits()),
            Value::F64(v) => SpectestValue::F64(v.to_bits()),
            Value::V128(v) => SpectestValue::V128(v),
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

    fn to_hex(v: SpectestValue) -> String {
        match v {
            SpectestValue::I32(v) => format!("{:#x}", v),
            SpectestValue::I64(v) => format!("{:#x}", v),
            SpectestValue::F32(v) => format!("{:#x}", v),
            SpectestValue::F64(v) => format!("{:#x}", v),
            SpectestValue::V128(v) => format!("{:#x}", v),
        }
    }

    fn print(_ctx: &mut Ctx) {
        println!("");
    }

    fn print_i32(_ctx: &mut Ctx, val: i32) {
        println!("{}", val);
    }

    fn print_f32(_ctx: &mut Ctx, val: f32) {
        println!("{}", val);
    }

    fn print_f64(_ctx: &mut Ctx, val: f64) {
        println!("{}", val);
    }

    fn print_i32_f32(_ctx: &mut Ctx, val: i32, val2: f32) {
        println!("{} {}", val, val2);
    }

    fn print_f64_f64(_ctx: &mut Ctx, val: f64, val2: f64) {
        println!("{} {}", val, val2);
    }

    fn get_spectest_import_object(
        registered_modules: &HashMap<String, Rc<Instance>>,
    ) -> ImportObject {
        let memory = Memory::new(MemoryDescriptor {
            minimum: Pages(1),
            maximum: Some(Pages(2)),
            shared: false,
        })
        .unwrap();

        let global_i32 = Global::new(wasmer_runtime_core::types::Value::I32(666));
        let global_f32 = Global::new(wasmer_runtime_core::types::Value::F32(666.0));
        let global_f64 = Global::new(wasmer_runtime_core::types::Value::F64(666.0));

        let table = Table::new(TableDescriptor {
            element: ElementType::Anyfunc,
            minimum: 10,
            maximum: Some(20),
        })
        .unwrap();
        let mut import_object = imports! {
            "spectest" => {
                "print" => func!(print),
                "print_i32" => func!(print_i32),
                "print_f32" => func!(print_f32),
                "print_f64" => func!(print_f64),
                "print_i32_f32" => func!(print_i32_f32),
                "print_f64_f64" => func!(print_f64_f64),
                "table" => table,
                "memory" => memory,
                "global_i32" => global_i32,
                "global_f32" => global_f32,
                "global_f64" => global_f64,

            },
        };

        for (name, instance) in registered_modules.iter() {
            import_object.register(name.clone(), Rc::clone(instance));
        }
        import_object
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    enum Exclude {
        Skip,
        Fail,
    }

    use core::borrow::Borrow;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    /// Reads the excludes.txt file into a hash map
    fn read_excludes() -> HashMap<String, Exclude> {
        let mut excludes_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        excludes_path.push("tests");
        excludes_path.push("excludes.txt");
        let input = File::open(excludes_path).unwrap();
        let buffered = BufReader::new(input);
        let mut result = HashMap::new();
        for line in buffered.lines() {
            let mut line = line.unwrap();
            if line.trim().is_empty() || line.starts_with("#") {
                // ignore line
            } else {
                if line.contains("#") {
                    // Allow end of line comment
                    let l: Vec<&str> = line.split('#').collect();
                    line = l.get(0).unwrap().to_string();
                }
                //println!("exclude line {}", line);
                // <backend>:<exclude-kind>:<test-file-name>:<test-file-line>
                let split: Vec<&str> = line.trim().split(':').collect();

                let kind = match *split.get(1).unwrap() {
                    "skip" => Exclude::Skip,
                    "fail" => Exclude::Fail,
                    _ => panic!("unknown exclude kind"),
                };
                let has_platform = split.len() > 4;

                let backend = split.get(0).unwrap();
                let testfile = split.get(2).unwrap();
                let line = split.get(3).unwrap();
                let key = if has_platform {
                    let platform = split.get(4).unwrap();
                    format!("{}:{}:{}:{}", backend, testfile, line, platform)
                } else {
                    format!("{}:{}:{}", backend, testfile, line)
                };
                result.insert(key, kind);
            }
        }
        result
    }

    #[test]
    fn test_run_spectests() {
        let mut success = true;
        let mut test_reports = vec![];

        let excludes = read_excludes();

        let mut glob_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        glob_path.push("spectests");
        glob_path.push("*.wast");

        let glob_str = glob_path.to_str().unwrap();
        for entry in glob(glob_str).expect("Failed to read glob pattern") {
            match entry {
                Ok(wast_path) => {
                    let result = parse_and_run(&wast_path, &excludes);
                    match result {
                        Ok(test_report) => {
                            if test_report.has_failures() {
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
                Err(e) => panic!("glob err: {:?}", e),
            }
        }

        // Print summary
        let mut failures = vec![];
        let mut total_passed = 0;
        let mut total_failed = 0;
        let mut total_allowed_failures = 0;
        for mut test_report in test_reports.into_iter() {
            total_passed += test_report.passed;
            total_failed += test_report.failed;
            total_allowed_failures += test_report.allowed_failure;
            failures.append(&mut test_report.failures);
        }

        println!("");
        println!("Failures:");
        let backend = get_compiler_name();
        for failure in failures.iter() {
            // To print excludes for all failures:
            println!(
                "{}:fail:{}:{} # {} - {}",
                backend, failure.file, failure.line, failure.kind, failure.message
            );
        }
        println!("");
        println!("");
        println!("Spec tests summary report: ");
        println!(
            "total: {}",
            total_passed + total_failed + total_allowed_failures
        );
        println!("passed: {}", total_passed);
        println!("failed: {}", total_failed);
        println!("allowed failures: {}", total_allowed_failures);
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
            let mantissa_msb = 0b1 << 22;
            self.is_nan() && (self.to_bits() & mantissa_msb) != 0
        }

        /// For a NaN to be canonical, the MSB of the mantissa must be set and
        /// all other mantissa bits must be unset.
        fn is_canonical_nan(&self) -> bool {
            return self.to_bits() == 0xFFC0_0000 || self.to_bits() == 0x7FC0_0000;
        }
    }

    impl NaNCheck for f64 {
        /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
        fn is_quiet_nan(&self) -> bool {
            let mantissa_msb = 0b1 << 51;
            self.is_nan() && (self.to_bits() & mantissa_msb) != 0
        }

        /// For a NaN to be canonical, the MSB of the mantissa must be set and
        /// all other mantissa bits must be unset.
        fn is_canonical_nan(&self) -> bool {
            self.to_bits() == 0x7FF8_0000_0000_0000 || self.to_bits() == 0xFFF8_0000_0000_0000
        }
    }
}
