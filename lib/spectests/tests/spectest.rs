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

    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

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
            _testkey: &str,
            excludes: &Vec<Exclude>,
            line: u64,
        ) {
            if excludes
                .iter()
                .any(|e| e.line_matches(line) && e.exclude_kind == ExcludeKind::Fail)
            {
                self.allowed_failure += 1;
                return;
            }
            self.failed += 1;
            self.failures.push(failure);
        }
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
    fn get_target_family() -> &'static str {
        "unix"
    }

    #[cfg(windows)]
    fn get_target_family() -> &'static str {
        "windows"
    }

    fn get_target_arch() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else if cfg!(target_arch = "mips") {
            "mips"
        } else if cfg!(target_arch = "powerpc") {
            "powerpc"
        } else if cfg!(target_arch = "powerpc64") {
            "powerpc64"
        } else if cfg!(target_arch = "arm") {
            "arm"
        } else {
            panic!("unknown target arch")
        }
    }

    //  clif:skip:data.wast:172:unix:x86
    #[allow(dead_code)]
    struct Exclude {
        backend: Option<String>,
        exclude_kind: ExcludeKind,
        file: String,
        line: Option<u64>,
        target_family: Option<String>,
        target_arch: Option<String>,
    }

    impl Exclude {
        fn line_matches(&self, value: u64) -> bool {
            self.line.is_none() || self.line.unwrap() == value
        }

        fn line_exact_match(&self, value: u64) -> bool {
            self.line.is_some() && self.line.unwrap() == value
        }

        fn matches_backend(&self, value: &str) -> bool {
            self.backend.is_none() || self.backend.as_ref().unwrap() == value
        }

        fn matches_target_family(&self, value: &str) -> bool {
            self.target_family.is_none() || self.target_family.as_ref().unwrap() == value
        }

        fn matches_target_arch(&self, value: &str) -> bool {
            self.target_arch.is_none() || self.target_arch.as_ref().unwrap() == value
        }

        fn from(
            backend: &str,
            exclude_kind: &str,
            file: &str,
            line: &str,
            target_family: &str,
            target_arch: &str,
        ) -> Exclude {
            let backend: Option<String> = match backend {
                "*" => None,
                "clif" => Some("clif".to_string()),
                "singlepass" => Some("singlepass".to_string()),
                "llvm" => Some("llvm".to_string()),
                _ => panic!("backend {:?} not recognized", backend),
            };
            let exclude_kind = match exclude_kind {
                "skip" => ExcludeKind::Skip,
                "fail" => ExcludeKind::Fail,
                _ => panic!("exclude kind {:?} not recognized", exclude_kind),
            };
            let line = match line {
                "*" => None,
                _ => Some(
                    line.parse::<u64>()
                        .expect(&format!("expected * or int: {:?}", line)),
                ),
            };
            let target_family = match target_family {
                "*" => None,
                _ => Some(target_family.to_string()),
            };
            let target_arch = match target_arch {
                "*" => None,
                _ => Some(target_arch.to_string()),
            };
            Exclude {
                backend,
                exclude_kind,
                file: file.to_string(),
                line,
                target_family,
                target_arch,
            }
        }
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler_name() -> &'static str {
        panic!("compiler not specified, activate a compiler via features");
        "unknown"
    }

    fn with_instance<F, R>(
        maybe_instance: Option<Arc<Mutex<Instance>>>,
        named_modules: &HashMap<String, Arc<Mutex<Instance>>>,
        module: &Option<String>,
        f: F,
    ) -> Option<R>
    where
        R: Sized,
        F: FnOnce(&Instance) -> R,
    {
        let ref ins = module
            .as_ref()
            .and_then(|name| named_modules.get(name).cloned())
            .or(maybe_instance)?;
        let guard = ins.lock().unwrap();
        Some(f(guard.borrow()))
    }

    use glob::glob;
    use std::collections::HashMap;
    use std::fs;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use wabt::script::{Action, Command, CommandKind, ScriptParser, Value};
    use wasmer_runtime::{
        compile_with_config,
        error::CompileError,
        func, imports,
        types::{ElementType, MemoryDescriptor, TableDescriptor},
        units::Pages,
        CompilerConfig, Ctx, Export, Features, Global, ImportObject, Instance, LikeNamespace,
        Memory, Table,
    };

    fn parse_and_run(
        path: &PathBuf,
        file_excludes: &HashSet<String>,
        excludes: &HashMap<String, Vec<Exclude>>,
    ) -> Result<TestReport, String> {
        let mut test_report = TestReport {
            failures: vec![],
            passed: 0,
            failed: 0,
            allowed_failure: 0,
        };

        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();

        // Entire file is excluded by line * and skip
        if file_excludes.contains(filename) {
            return Ok(test_report);
        }

        let mut features = wabt::Features::new();
        features.enable_simd();
        features.enable_threads();
        features.enable_sign_extension();
        features.enable_sat_float_to_int();
        let mut parser: ScriptParser =
            ScriptParser::from_source_and_name_with_features(&source, filename, features)
                .expect(&format!("Failed to parse script {}", &filename));

        use std::panic;
        let mut instance: Option<Arc<Mutex<Instance>>> = None;

        let mut named_modules: HashMap<String, Arc<Mutex<Instance>>> = HashMap::new();

        let mut registered_modules: HashMap<String, Arc<Mutex<Instance>>> = HashMap::new();
        //
        let empty_excludes = vec![];
        let excludes = if excludes.contains_key(filename) {
            excludes.get(filename).unwrap()
        } else {
            &empty_excludes
        };

        let backend = get_compiler_name();

        while let Some(Command { kind, line }) =
            parser.next().map_err(|e| format!("Parse err: {:?}", e))?
        {
            let test_key = format!("{}:{}:{}", backend, filename, line);
            // Use this line to debug which test is running
            println!("Running test: {}", test_key);

            // Skip tests that match this line
            if excludes
                .iter()
                .any(|e| e.line_exact_match(line) && e.exclude_kind == ExcludeKind::Skip)
            {
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
                        let module = compile_with_config(&module.into_vec(), config)
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
                                line,
                            );
                            instance = None;
                        }
                        Ok(i) => {
                            let i = Arc::new(Mutex::new(i));
                            if name.is_some() {
                                named_modules.insert(name.unwrap(), Arc::clone(&i));
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
                            let maybe_call_result = with_instance(
                                instance.clone(),
                                &named_modules,
                                &module,
                                |instance| {
                                    let params: Vec<wasmer_runtime::types::Value> =
                                        args.iter().cloned().map(convert_value).collect();
                                    instance.call(&field, &params[..])
                                },
                            );
                            if maybe_call_result.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertReturn"),
                                        message: format!("No instance available: {:?}", &module),
                                    },
                                    &test_key,
                                    excludes,
                                    line,
                                );
                            } else {
                                let call_result = maybe_call_result.unwrap();
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
                                            line,
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
                                                }, &test_key, excludes, line);
                                            } else {
                                                test_report.count_passed();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Action::Get { module, field } => {
                            let maybe_call_result = with_instance(
                                instance.clone(),
                                &named_modules,
                                &module,
                                |instance| {
                                    instance
                                        .get_export(&field)
                                        .expect(&format!("missing global {:?}", &field))
                                },
                            );
                            if maybe_call_result.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertReturn Get"),
                                        message: format!("No instance available {:?}", &module),
                                    },
                                    &test_key,
                                    excludes,
                                    line,
                                );
                            } else {
                                let export: Export = maybe_call_result.unwrap();
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
                                                line,
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
                                            line,
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
                        let maybe_call_result =
                            with_instance(instance.clone(), &named_modules, &module, |instance| {
                                let params: Vec<wasmer_runtime::types::Value> =
                                    args.iter().cloned().map(convert_value).collect();
                                instance.call(&field, &params[..])
                            });
                        if maybe_call_result.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertReturnCanonicalNan"),
                                    message: format!("No instance available {:?}", &module),
                                },
                                &test_key,
                                excludes,
                                line,
                            );
                        } else {
                            let call_result = maybe_call_result.unwrap();
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
                                        line,
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
                                                        "value is not canonical nan {:?} ({:?})",
                                                        v,
                                                        value_to_hex(v.clone()),
                                                    ),
                                                },
                                                &test_key,
                                                excludes,
                                                line,
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
                        let maybe_call_result =
                            with_instance(instance.clone(), &named_modules, &module, |instance| {
                                let params: Vec<wasmer_runtime::types::Value> =
                                    args.iter().cloned().map(convert_value).collect();
                                instance.call(&field, &params[..])
                            });
                        if maybe_call_result.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertReturnArithmeticNan"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                                line,
                            );
                        } else {
                            let call_result = maybe_call_result.unwrap();
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
                                        line,
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
                                                        "value is not arithmetic nan {:?} ({:?})",
                                                        v,
                                                        value_to_hex(v.clone()),
                                                    ),
                                                },
                                                &test_key,
                                                excludes,
                                                line,
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
                        let maybe_call_result =
                            with_instance(instance.clone(), &named_modules, &module, |instance| {
                                let params: Vec<wasmer_runtime::types::Value> =
                                    args.iter().cloned().map(convert_value).collect();
                                instance.call(&field, &params[..])
                            });
                        if maybe_call_result.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertTrap"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                                line,
                            );
                        } else {
                            let call_result = maybe_call_result.unwrap();
                            use wasmer_runtime::error::{CallError, RuntimeError};
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
                                                line,
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
                                                        line,
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
                                        line,
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
                        compile_with_config(&module.into_vec(), config)
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
                                    line,
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
                                line,
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
                        compile_with_config(&module.into_vec(), config)
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
                                    line,
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
                                line,
                            );
                        }
                    }
                }
                CommandKind::AssertUninstantiable { module, message: _ } => {
                    let spectest_import_object = get_spectest_import_object(&registered_modules);
                    let config = CompilerConfig {
                        features: Features {
                            simd: true,
                            threads: true,
                        },
                        ..Default::default()
                    };
                    let module = compile_with_config(&module.into_vec(), config)
                        .expect("WASM can't be compiled");
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        module
                            .instantiate(&spectest_import_object)
                            .expect("WASM can't be instantiated");
                    }));
                    match result {
                        Err(_) => test_report.count_passed(),
                        Ok(_) => {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "AssertUninstantiable"),
                                    message: format!(
                                        "instantiate successful, expected uninstantiable"
                                    ),
                                },
                                &test_key,
                                excludes,
                                line,
                            );
                        }
                    };
                }
                CommandKind::AssertExhaustion { action, message: _ } => {
                    match action {
                        Action::Invoke {
                            module,
                            field,
                            args,
                        } => {
                            let maybe_call_result = with_instance(
                                instance.clone(),
                                &named_modules,
                                &module,
                                |instance| {
                                    let params: Vec<wasmer_runtime::types::Value> =
                                        args.iter().cloned().map(convert_value).collect();
                                    instance.call(&field, &params[..])
                                },
                            );
                            if maybe_call_result.is_none() {
                                test_report.add_failure(
                                    SpecFailure {
                                        file: filename.to_string(),
                                        line: line,
                                        kind: format!("{}", "AssertExhaustion"),
                                        message: format!("No instance available"),
                                    },
                                    &test_key,
                                    excludes,
                                    line,
                                );
                            } else {
                                let call_result = maybe_call_result.unwrap();
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
                                            line,
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
                        let module = compile_with_config(&module.into_vec(), config)
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
                                line,
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
                                    line,
                                );
                            }
                            Err(e) => match e {
                                wasmer_runtime::error::Error::LinkError(_) => {
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
                                        line,
                                    );
                                }
                            },
                        },
                    }
                }
                CommandKind::Register { name, as_name } => {
                    let instance: Option<Arc<Mutex<Instance>>> = match name {
                        Some(ref name) => {
                            let i = named_modules.get(name);
                            match i {
                                Some(ins) => Some(Arc::clone(ins)),
                                None => None,
                            }
                        }
                        None => match instance {
                            Some(ref i) => Some(Arc::clone(i)),
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
                            line,
                        );
                    }
                }
                CommandKind::PerformAction(ref action) => match action {
                    Action::Invoke {
                        module,
                        field,
                        args,
                    } => {
                        let maybe_call_result =
                            with_instance(instance.clone(), &named_modules, &module, |instance| {
                                let params: Vec<wasmer_runtime::types::Value> =
                                    args.iter().cloned().map(convert_value).collect();
                                instance.call(&field, &params[..])
                            });
                        if maybe_call_result.is_none() {
                            test_report.add_failure(
                                SpecFailure {
                                    file: filename.to_string(),
                                    line: line,
                                    kind: format!("{}", "PerformAction"),
                                    message: format!("No instance available"),
                                },
                                &test_key,
                                excludes,
                                line,
                            );
                        } else {
                            let call_result = maybe_call_result.unwrap();
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
                                        line,
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

    fn is_canonical_nan(val: wasmer_runtime::types::Value) -> bool {
        match val {
            wasmer_runtime::types::Value::F32(x) => x.is_canonical_nan(),
            wasmer_runtime::types::Value::F64(x) => x.is_canonical_nan(),
            _ => panic!("value is not a float {:?}", val),
        }
    }

    fn is_arithmetic_nan(val: wasmer_runtime::types::Value) -> bool {
        match val {
            wasmer_runtime::types::Value::F32(x) => x.is_quiet_nan(),
            wasmer_runtime::types::Value::F64(x) => x.is_quiet_nan(),
            _ => panic!("value is not a float {:?}", val),
        }
    }

    fn value_to_hex(val: wasmer_runtime::types::Value) -> String {
        match val {
            wasmer_runtime::types::Value::I32(x) => format!("{:#x}", x),
            wasmer_runtime::types::Value::I64(x) => format!("{:#x}", x),
            wasmer_runtime::types::Value::F32(x) => format!("{:#x}", x.to_bits()),
            wasmer_runtime::types::Value::F64(x) => format!("{:#x}", x.to_bits()),
            wasmer_runtime::types::Value::V128(x) => format!("{:#x}", x),
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

    fn convert_wasmer_value(other: wasmer_runtime::types::Value) -> SpectestValue {
        match other {
            wasmer_runtime::types::Value::I32(v) => SpectestValue::I32(v),
            wasmer_runtime::types::Value::I64(v) => SpectestValue::I64(v),
            wasmer_runtime::types::Value::F32(v) => SpectestValue::F32(v.to_bits()),
            wasmer_runtime::types::Value::F64(v) => SpectestValue::F64(v.to_bits()),
            wasmer_runtime::types::Value::V128(v) => SpectestValue::V128(v),
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

    fn convert_value(other: Value<f32, f64>) -> wasmer_runtime::types::Value {
        match other {
            Value::I32(v) => wasmer_runtime::types::Value::I32(v),
            Value::I64(v) => wasmer_runtime::types::Value::I64(v),
            Value::F32(v) => wasmer_runtime::types::Value::F32(v),
            Value::F64(v) => wasmer_runtime::types::Value::F64(v),
            Value::V128(v) => wasmer_runtime::types::Value::V128(v),
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
        registered_modules: &HashMap<String, Arc<Mutex<Instance>>>,
    ) -> ImportObject {
        let memory_desc = MemoryDescriptor::new(Pages(1), Some(Pages(2)), false).unwrap();
        let memory = Memory::new(memory_desc).unwrap();

        let global_i32 = Global::new(wasmer_runtime::types::Value::I32(666));
        let global_f32 = Global::new(wasmer_runtime::types::Value::F32(666.0));
        let global_f64 = Global::new(wasmer_runtime::types::Value::F64(666.0));

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
            import_object.register(name.clone(), Arc::clone(instance));
        }
        import_object
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    enum ExcludeKind {
        Skip,
        Fail,
    }

    use core::borrow::Borrow;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    /// Reads the excludes.txt file into a hash map
    fn read_excludes() -> (HashMap<String, Vec<Exclude>>, HashSet<String>) {
        let mut excludes_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        excludes_path.push("tests");
        excludes_path.push("excludes.txt");
        let input = File::open(excludes_path).unwrap();
        let buffered = BufReader::new(input);
        let mut result = HashMap::new();
        let mut file_excludes = HashSet::new();
        let current_backend = get_compiler_name();
        let current_target_family = get_target_family();
        let current_target_arch = get_target_arch();

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

                let file = *split.get(2).unwrap();
                let exclude = match split.len() {
                    0..=3 => panic!("expected at least 4 exclude conditions"),
                    4 => Exclude::from(
                        *split.get(0).unwrap(),
                        *split.get(1).unwrap(),
                        *split.get(2).unwrap(),
                        *split.get(3).unwrap(),
                        "*",
                        "*",
                    ),
                    5 => Exclude::from(
                        *split.get(0).unwrap(),
                        *split.get(1).unwrap(),
                        *split.get(2).unwrap(),
                        *split.get(3).unwrap(),
                        *split.get(4).unwrap(),
                        "*",
                    ),
                    6 => Exclude::from(
                        *split.get(0).unwrap(),
                        *split.get(1).unwrap(),
                        *split.get(2).unwrap(),
                        *split.get(3).unwrap(),
                        *split.get(4).unwrap(),
                        *split.get(5).unwrap(),
                    ),
                    _ => panic!("too many exclude conditions {}", split.len()),
                };

                if exclude.matches_backend(current_backend)
                    && exclude.matches_target_family(current_target_family)
                    && exclude.matches_target_arch(current_target_arch)
                {
                    // Skip the whole file for line * and skip
                    if exclude.line.is_none() && exclude.exclude_kind == ExcludeKind::Skip {
                        file_excludes.insert(file.to_string());
                    }

                    if !result.contains_key(file) {
                        result.insert(file.to_string(), vec![]);
                    }
                    result.get_mut(file).unwrap().push(exclude);
                }
            }
        }
        (result, file_excludes)
    }

    #[test]
    fn test_run_spectests() {
        let mut success = true;
        let mut test_reports = vec![];

        let (excludes, file_excludes) = read_excludes();

        let mut glob_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        glob_path.push("spectests");
        glob_path.push("*.wast");

        let glob_str = glob_path.to_str().unwrap();
        for entry in glob(glob_str).expect("Failed to read glob pattern") {
            match entry {
                Ok(wast_path) => {
                    let result = parse_and_run(&wast_path, &file_excludes, &excludes);
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
