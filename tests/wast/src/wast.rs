use crate::errors::{CallError, DirectiveError, DirectiveErrors};
use crate::spectest::spectest_importobject;
use anyhow::{anyhow, bail, Context as _, Result};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::Path;
use std::str;
use std::sync::{Arc, Mutex};
use wasmer::compiler::{compile_with_config_with, compiler_for_backend, Backend, CompilerConfig};
use wasmer::import::ImportObject;
use wasmer::wasm::Value;
use wasmer::wasm::{DynFunc, Features, Global};
use wasmer::*;

/// The wast test script language allows modules to be defined and actions
/// to be performed on them.
pub struct Wast {
    /// Wast files have a concept of a "current" module, which is the most
    /// recently defined.
    current: Option<Arc<Mutex<Instance>>>,
    /// The Import Object that all wast tests will have
    import_object: ImportObject,
    /// The instances in the test
    instances: HashMap<String, Arc<Mutex<Instance>>>,
    /// The Wasmer backend used
    backend: Backend,
    /// A flag indicating if Wast tests should stop as soon as one test fails.
    pub fail_fast: bool,
}

impl Wast {
    /// Construct a new instance of `Wast` with a given imports.
    pub fn new(import_object: ImportObject, backend: Backend) -> Self {
        Self {
            current: None,
            backend,
            import_object,
            instances: HashMap::new(),
            fail_fast: false,
        }
    }

    /// Construcet a new instance of `Wast` with the spectests imports.
    pub fn new_with_spectest(backend: Backend) -> Self {
        let import_object = spectest_importobject();
        Self::new(import_object, backend)
    }

    fn get_instance(&self, instance_name: Option<&str>) -> Result<Arc<Mutex<Instance>>> {
        match instance_name {
            Some(name) => self
                .instances
                .get(name)
                .as_ref()
                .map(|x| Arc::clone(x))
                .ok_or_else(|| anyhow!("failed to find instance named `{}`", name)),
            None => self
                .current
                .as_ref()
                .map(|x| Arc::clone(x))
                .ok_or_else(|| anyhow!("no previous instance found")),
        }
    }

    /// Perform the action portion of a command.
    fn perform_execute(&mut self, exec: wast::WastExecute<'_>) -> Result<Vec<Value>> {
        match exec {
            wast::WastExecute::Invoke(invoke) => self.perform_invoke(invoke),
            wast::WastExecute::Module(mut module) => {
                let binary = module.encode()?;
                let result = self.instantiate(&binary);
                match result {
                    Ok(_) => Ok(Vec::new()),
                    Err(e) => Err(e),
                }
            }
            wast::WastExecute::Get { module, global } => self.get(module.map(|s| s.name()), global),
        }
    }

    fn perform_invoke(&mut self, exec: wast::WastInvoke<'_>) -> Result<Vec<Value>> {
        let values = exec
            .args
            .iter()
            .map(Self::runtime_value)
            .collect::<Result<Vec<_>>>()?;
        self.invoke(exec.module.map(|i| i.name()), exec.name, &values)
    }

    fn assert_return(
        &self,
        result: Result<Vec<Value>>,
        results: &[wast::AssertExpression],
    ) -> Result<()> {
        let values = result?;
        for (v, e) in values.iter().zip(results) {
            if val_matches(v, e)? {
                continue;
            }
            bail!("expected {:?}, got {:?}", e, v)
        }
        Ok(())
    }

    fn assert_trap(&self, result: Result<Vec<Value>>, expected: &str) -> Result<()> {
        let actual = match result {
            Ok(values) => bail!("expected trap, got {:?}", values),
            Err(t) => format!("{}", t),
        };
        if Self::matches_message_assert_trap(expected, &actual) {
            return Ok(());
        }
        bail!("expected '{}', got '{}'", expected, actual)
    }

    fn run_directive(&mut self, directive: wast::WastDirective) -> Result<()> {
        use wast::WastDirective::*;

        match directive {
            Module(mut module) => {
                let binary = module.encode()?;
                self.module(module.id.map(|s| s.name()), &binary)?;
            }
            Register {
                span: _,
                name,
                module,
            } => {
                self.register(module.map(|s| s.name()), name)?;
            }
            Invoke(i) => {
                self.perform_invoke(i)?;
            }
            AssertReturn {
                span: _,
                exec,
                results,
            } => {
                let result = self.perform_execute(exec);
                self.assert_return(result, &results)?;
            }
            AssertTrap {
                span: _,
                exec,
                message,
            } => {
                let result = self.perform_execute(exec);
                self.assert_trap(result, message)?;
            }
            AssertExhaustion {
                span: _,
                call,
                message,
            } => {
                let result = self.perform_invoke(call);
                self.assert_trap(result, message)?;
            }
            AssertInvalid {
                span: _,
                mut module,
                message,
            } => {
                let bytes = module.encode()?;
                let err = match self.module(None, &bytes) {
                    Ok(()) => bail!("expected module to fail to build"),
                    Err(e) => e,
                };
                let error_message = format!("{:?}", err);
                if !Self::matches_message_assert_invalid(&message, &error_message) {
                    bail!(
                        "assert_invalid: expected \"{}\", got \"{}\"",
                        message,
                        error_message
                    )
                }
            }
            AssertMalformed {
                module,
                span: _,
                message: _,
            } => {
                let mut module = match module {
                    wast::QuoteModule::Module(m) => m,
                    // This is a `*.wat` parser test which we're not
                    // interested in.
                    wast::QuoteModule::Quote(_) => return Ok(()),
                };
                let bytes = module.encode()?;
                if let Ok(_) = self.module(None, &bytes) {
                    bail!("expected malformed module to fail to instantiate");
                }
            }
            AssertUnlinkable {
                span: _,
                mut module,
                message,
            } => {
                let bytes = module.encode()?;
                let err = match self.module(None, &bytes) {
                    Ok(()) => bail!("expected module to fail to link"),
                    Err(e) => e,
                };
                let error_message = format!("{:?}", err);
                if !Self::matches_message_assert_unlinkable(&message, &error_message) {
                    bail!(
                        "assert_unlinkable: expected {}, got {}",
                        message,
                        error_message
                    )
                }
            }
        }

        Ok(())
    }

    /// Run a wast script from a byte buffer.
    pub fn run_buffer(&mut self, filename: &str, wast: &[u8]) -> Result<()> {
        let wast = str::from_utf8(wast)?;

        let adjust_wast = |mut err: wast::Error| {
            err.set_path(filename.as_ref());
            err.set_text(wast);
            err
        };

        let buf = wast::parser::ParseBuffer::new(wast).map_err(adjust_wast)?;
        let ast = wast::parser::parse::<wast::Wast>(&buf).map_err(adjust_wast)?;
        let mut errors = Vec::with_capacity(ast.directives.len());
        for directive in ast.directives {
            let sp = directive.span();
            match self.run_directive(directive) {
                Err(e) => {
                    let (line, col) = sp.linecol_in(wast);
                    errors.push(DirectiveError {
                        line: line + 1,
                        col,
                        message: format!("{}", e),
                    });
                    if self.fail_fast {
                        break;
                    }
                }
                Ok(_) => {}
            }
        }
        if !errors.is_empty() {
            return Err(DirectiveErrors {
                filename: filename.to_string(),
                errors,
            }
            .into());
        }
        Ok(())
    }

    /// Run a wast script from a file.
    pub fn run_file(&mut self, path: &Path) -> Result<()> {
        let bytes =
            std::fs::read(path).with_context(|| format!("failed to read `{}`", path.display()))?;
        self.run_buffer(path.to_str().unwrap(), &bytes)
    }
}

// This is the implementation specific to the Runtime
impl Wast {
    /// Define a module and register it.
    fn module(&mut self, instance_name: Option<&str>, module: &[u8]) -> Result<()> {
        let instance = match self.instantiate(module) {
            Ok(i) => i,
            Err(e) => bail!("instantiation failed with: {}", e),
        };
        let instance = Arc::new(Mutex::new(instance));
        if let Some(name) = instance_name {
            self.instances.insert(name.to_string(), instance.clone());
        }
        self.current = Some(instance.clone());
        Ok(())
    }

    fn instantiate(&self, module: &[u8]) -> Result<Instance> {
        // let module = Module::new(module)?;
        let config = CompilerConfig {
            features: Features {
                simd: true,
                threads: true,
            },
            nan_canonicalization: true,
            enable_verification: true,
            ..Default::default()
        };
        let compiler = compiler_for_backend(self.backend).expect("backend not found");
        let module = compile_with_config_with(module, config, &*compiler)?;

        let mut imports = self.import_object.clone_ref();

        for import in module.imports() {
            let module_name = import.namespace;
            if imports.contains_namespace(&module_name) {
                continue;
            }
            let instance = self
                .instances
                .get(&module_name)
                .ok_or_else(|| anyhow!("no module named `{}`", module_name))?;
            imports.register(module_name, Arc::clone(instance));
        }

        let instance = module
            .instantiate(&imports)
            .map_err(|e| anyhow!("Instantiate error: {}", e))?;
        Ok(instance)
    }

    /// Register an instance to make it available for performing actions.
    fn register(&mut self, name: Option<&str>, as_name: &str) -> Result<()> {
        let instance = self.get_instance(name)?;
        self.instances.insert(as_name.to_string(), instance);
        Ok(())
    }

    /// Invoke an exported function from an instance.
    fn invoke(
        &mut self,
        instance_name: Option<&str>,
        field: &str,
        args: &[Value],
    ) -> Result<Vec<Value>> {
        let clonable_instance = self.get_instance(instance_name)?;
        let instance = clonable_instance.lock().unwrap();
        let func: DynFunc = instance.borrow().exports.get(field)?;
        match func.call(args) {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(CallError {
                message: format!("{}", e),
            }
            .into()),
        }
    }

    /// Get the value of an exported global from an instance.
    fn get(&mut self, instance_name: Option<&str>, field: &str) -> Result<Vec<Value>> {
        let clonable_instance = self.get_instance(instance_name)?;
        let instance = clonable_instance.lock().unwrap();
        let global: Global = instance.borrow().exports.get(field)?;
        Ok(vec![global.get()])
    }

    /// Translate from a `script::Value` to a `Val`.
    fn runtime_value(v: &wast::Expression<'_>) -> Result<Value> {
        use wast::Instruction::*;

        if v.instrs.len() != 1 {
            bail!("too many instructions in {:?}", v);
        }
        Ok(match &v.instrs[0] {
            I32Const(x) => Value::I32(*x),
            I64Const(x) => Value::I64(*x),
            F32Const(x) => Value::F32(f32::from_bits(x.bits)),
            F64Const(x) => Value::F64(f64::from_bits(x.bits)),
            V128Const(x) => Value::V128(u128::from_le_bytes(x.to_le_bytes())),
            other => bail!("couldn't convert {:?} to a runtime value", other),
        })
    }

    // Checks if the `assert_unlinkable` message matches the expected one
    fn matches_message_assert_unlinkable(_expected: &str, _actual: &str) -> bool {
        // We skip message matching for now
        true
        // actual.contains(&expected)
    }

    // Checks if the `assert_invalid` message matches the expected one
    fn matches_message_assert_invalid(_expected: &str, _actual: &str) -> bool {
        // We skip message matching for now
        true
        // actual.contains(expected)
        //     // Waiting on https://github.com/WebAssembly/bulk-memory-operations/pull/137
        //     // to propagate to WebAssembly/testsuite.
        //     || (expected.contains("unknown table") && actual.contains("unknown elem"))
        //     // `elem.wast` and `proposals/bulk-memory-operations/elem.wast` disagree
        //     // on the expected error message for the same error.
        //     || (expected.contains("out of bounds") && actual.contains("does not fit"))
    }

    // Checks if the `assert_trap` message matches the expected one
    fn matches_message_assert_trap(_expected: &str, _actual: &str) -> bool {
        // We skip message matching for now
        true
        // actual.contains(expected)
        //     // `bulk-memory-operations/bulk.wast` checks for a message that
        //     // specifies which element is uninitialized, but our traps don't
        //     // shepherd that information out.
        //     || (expected.contains("uninitialized element 2") && actual.contains("uninitialized element"))
    }
}

fn extract_lane_as_i8(bytes: u128, lane: usize) -> i8 {
    (bytes >> (lane * 8)) as i8
}

fn extract_lane_as_i16(bytes: u128, lane: usize) -> i16 {
    (bytes >> (lane * 16)) as i16
}

fn extract_lane_as_i32(bytes: u128, lane: usize) -> i32 {
    (bytes >> (lane * 32)) as i32
}

fn extract_lane_as_i64(bytes: u128, lane: usize) -> i64 {
    (bytes >> (lane * 64)) as i64
}

fn val_matches(actual: &Value, expected: &wast::AssertExpression) -> Result<bool> {
    Ok(match (actual, expected) {
        (Value::I32(a), wast::AssertExpression::I32(b)) => a == b,
        (Value::I64(a), wast::AssertExpression::I64(b)) => a == b,
        // Note that these float comparisons are comparing bits, not float
        // values, so we're testing for bit-for-bit equivalence
        (Value::F32(a), wast::AssertExpression::F32(b)) => f32_matches(a.to_bits(), &b),
        (Value::F64(a), wast::AssertExpression::F64(b)) => f64_matches(a.to_bits(), &b),
        (Value::V128(a), wast::AssertExpression::V128(b)) => v128_matches(*a, &b),
        // Legacy comparators
        (Value::F32(a), wast::AssertExpression::LegacyCanonicalNaN) => a.is_canonical_nan(),
        (Value::F32(a), wast::AssertExpression::LegacyArithmeticNaN) => a.is_arithmetic_nan(),
        (Value::F64(a), wast::AssertExpression::LegacyCanonicalNaN) => a.is_canonical_nan(),
        (Value::F64(a), wast::AssertExpression::LegacyArithmeticNaN) => a.is_arithmetic_nan(),
        _ => bail!(
            "don't know how to compare {:?} and {:?} yet",
            actual,
            expected
        ),
    })
}

fn f32_matches(actual: u32, expected: &wast::NanPattern<wast::Float32>) -> bool {
    match expected {
        wast::NanPattern::CanonicalNan => f32::from_bits(actual).is_canonical_nan(),
        wast::NanPattern::ArithmeticNan => f32::from_bits(actual).is_arithmetic_nan(),
        wast::NanPattern::Value(expected_value) => actual == expected_value.bits,
    }
}

fn f64_matches(actual: u64, expected: &wast::NanPattern<wast::Float64>) -> bool {
    match expected {
        wast::NanPattern::CanonicalNan => f64::from_bits(actual).is_canonical_nan(),
        wast::NanPattern::ArithmeticNan => f64::from_bits(actual).is_arithmetic_nan(),
        wast::NanPattern::Value(expected_value) => actual == expected_value.bits,
    }
}

fn v128_matches(actual: u128, expected: &wast::V128Pattern) -> bool {
    match expected {
        wast::V128Pattern::I8x16(b) => b
            .iter()
            .enumerate()
            .all(|(i, b)| *b == extract_lane_as_i8(actual, i)),
        wast::V128Pattern::I16x8(b) => b
            .iter()
            .enumerate()
            .all(|(i, b)| *b == extract_lane_as_i16(actual, i)),
        wast::V128Pattern::I32x4(b) => b
            .iter()
            .enumerate()
            .all(|(i, b)| *b == extract_lane_as_i32(actual, i)),
        wast::V128Pattern::I64x2(b) => b
            .iter()
            .enumerate()
            .all(|(i, b)| *b == extract_lane_as_i64(actual, i)),
        wast::V128Pattern::F32x4(b) => b.iter().enumerate().all(|(i, b)| {
            let a = extract_lane_as_i32(actual, i) as u32;
            f32_matches(a, b)
        }),
        wast::V128Pattern::F64x2(b) => b.iter().enumerate().all(|(i, b)| {
            let a = extract_lane_as_i64(actual, i) as u64;
            f64_matches(a, b)
        }),
    }
}

pub trait NaNCheck {
    fn is_arithmetic_nan(&self) -> bool;
    fn is_canonical_nan(&self) -> bool;
}

impl NaNCheck for f32 {
    fn is_arithmetic_nan(&self) -> bool {
        const AF32_NAN: u32 = 0x0040_0000;
        (self.to_bits() & AF32_NAN) == AF32_NAN
    }

    fn is_canonical_nan(&self) -> bool {
        return (self.to_bits() & 0x7fff_ffff) == 0x7fc0_0000;
    }
}

impl NaNCheck for f64 {
    fn is_arithmetic_nan(&self) -> bool {
        const AF64_NAN: u64 = 0x0008_0000_0000_0000;
        (self.to_bits() & AF64_NAN) == AF64_NAN
    }

    fn is_canonical_nan(&self) -> bool {
        (self.to_bits() & 0x7fff_ffff_ffff_ffff) == 0x7ff8_0000_0000_0000
    }
}
