// Shamelessly copied from greenwasm-spectest:
// https://github.com/Kimundi/greenwasm/blob/master/greenwasm-spectest/src/lib.rs

#![deny(missing_docs)]

//! A library shim around a mirror of the
//! official [webassembly MVP testsuite](https://github.com/WebAssembly/spec/tree/master/test/core).
//!
//! It can be used as a independent testsuite launcher for other
//! webassembly implementations by implementing `ScriptHandler`.
//!
//! Example:
//! ```should_fail
/*!
use wasmer::spec::*;
struct DummyHandler;
impl ScriptHandler for DummyHandler {
    fn reset(&mut self) {}
    fn action_invoke(&mut self,
                     module: Option<String>,
                     field: String,
                     args: Vec<Value>) -> InvokationResult
    {
        unimplemented!()
    }
    fn action_get(&mut self,
                  module: Option<String>,
                  field: String) -> Value
    {
        unimplemented!()
    }
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>) {
        unimplemented!()
    }
    fn assert_malformed(&mut self, bytes: Vec<u8>) {
        unimplemented!()
    }
    fn assert_invalid(&mut self, bytes: Vec<u8>) {
        unimplemented!()
    }
    fn assert_uninstantiable(&mut self, bytes: Vec<u8>) {
        unimplemented!()
    }
    fn assert_exhaustion(&mut self, action: Action) {
        unimplemented!()
    }
    fn register(&mut self, name: Option<String>, as_name: String) {
        unimplemented!()
    }
}
run_mvp_spectest(&mut DummyHandler).present();
*/
//! ```
//! This would result in a output like this:
//! ```text
/*!
Executing address.wast ...
Executing align.wast ...
Executing binary.wast ...
Executing block.wast ...
Executing br.wast ...
[...]
wast failures:
    address.wast:3, not yet implemented
    address.wast:104, <not attempted>
    address.wast:105, <not attempted>
    ...
wast total: 0 passed; 17955 failed
*/
//! ```

pub extern crate wabt;
pub use wabt::script::Value;
pub use wabt::script::Action;
pub use wabt::script::CommandKind;

use std::path::Path;
use wabt::script::*;

#[cfg(test)]
mod tests;

/// Handles the different script commands of the `*.wast` format.
pub trait ScriptHandler {
    /// Reset all state of the handler, specifically
    /// clearing all loaded modules and assuming a new script file.
    fn reset(&mut self);

    /// Handles an `invoke` action.
    ///
    /// Should call a exported function with name `field` and arguments
    /// `args` from a loaded module.
    ///
    /// Targets either the last loaded module if `module` is None, or
    /// the module registered with the given name otherwise.
    fn action_invoke(&mut self, module: Option<String>, field: String, args: Vec<Value>) -> InvokationResult;

    /// Handles an `get` action.
    ///
    /// Should get a exported global with name `field` from a loaded module.
    ///
    /// Targets either the last loaded module if `module` is None, or
    /// the module registered with the given name otherwise.
    fn action_get(&mut self, module: Option<String>, field: String) -> Value;

    /// Handles an `action`.
    ///
    /// The default implementation dispatches to `action_get` or `
    /// action_invoke`, gathers the result in an vector, and panics
    /// if a function call trapped or exhausted the stack.
    fn action(&mut self, action: Action) -> Vec<Value> {
        match action {
            Action::Invoke { module, field, args } => {
                if let InvokationResult::Vals(v) = self.action_invoke(module, field, args) {
                    v
                } else {
                    panic!("invokation returned Trap or exhausted the stack");
                }
            }
            Action::Get { module, field } => {
                vec![self.action_get(module, field)]
            }
        }
    }

    /// Handles a module load.
    ///
    /// The webassembly module is passed in its binary format in
    /// the `bytes` argument.
    ///
    /// If `name` is `Some`, it should be registered under that name.
    /// In any case it should count as the least recently loaded module.
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>);

    /// Handles an `assert_return`.
    ///
    /// Per default panics if the result of handling the `action`
    /// does not result in the `expected` values.
    ///
    /// Floating point values should, and per default are,
    /// compared according to their bit-pattern, and not their normal
    /// `PartialEq` semantic. See the `NanCompare` wrapper type.
    fn assert_return(&mut self, action: Action, expected: Vec<Value>) {
        let results = self.action(action);
        assert_eq!(NanCompare(&expected), NanCompare(&results));
    }

    /// Handles an `assert_trap`.
    ///
    /// Per default panics if the result of handling the `action`
    /// does not trap, or refers to an global.
    fn assert_trap(&mut self, action: Action) {
        match action {
            Action::Invoke { module, field, args } => {
                if let InvokationResult::Vals(results) = self.action_invoke(module, field, args) {
                    panic!("invokation did not trap, but returned {:?}", results);
                }
            }
            Action::Get { .. } => {
                panic!("a global access can not trap!")
            }
        }
    }

    /// Handles an `assert_malformed`.
    ///
    /// The webassembly module is passed in its binary format in
    /// the `bytes` argument.
    ///
    /// Should panic if the module can be successfully decoded.
    fn assert_malformed(&mut self, bytes: Vec<u8>);

    /// Handles an `assert_malformed`.
    ///
    /// The webassembly module is passed in its binary format in
    /// the `bytes` argument.
    ///
    /// Should panic if the module can be successfully decoded.
    fn assert_invalid(&mut self, bytes: Vec<u8>);

    /// Handles an `assert_unlinkable`.
    ///
    /// The webassembly module is passed in its binary format in
    /// the `bytes` argument.
    ///
    /// Should panic if the module can be successfully linked.
    ///
    /// This seems to be a legacy script command, and per default
    /// just invokes `assert_uninstantiable`.
    fn assert_unlinkable(&mut self, bytes: Vec<u8>) {
        // TODO: figure out the exact difference
        // Currently it looks like a link error is any instantiation error except
        // a runtime error during execution of a start function
        self.assert_uninstantiable(bytes);
    }

    /// Handles an `assert_uninstantiable`.
    ///
    /// The webassembly module is passed in its binary format in
    /// the `bytes` argument.
    ///
    /// Should panic if the module can be successfully instantiated.
    fn assert_uninstantiable(&mut self, bytes: Vec<u8>);

    /// Handles an `assert_trap`.
    ///
    /// Should panic if the result of handling the `action`
    /// does not exhaust the stack, or refers to an global.
    fn assert_exhaustion(&mut self, action: Action);

    /// Handles an `assert_return_canonical_nan`.
    ///
    /// Per default panics if the result of handling the `action`
    /// does not result in single canonical NaN floating point value.
    ///
    /// Any canonical NaN is also a arithmetic NaN.
    ///
    /// Floating point values should, and per default are,
    /// compared according to their bit-pattern, and not their normal
    /// `PartialEq` semantic. See the `NanCompare` wrapper type.
    fn assert_return_canonical_nan(&mut self, action: Action) {
        let results = self.action(action);
        match *results {
            [Value::F32(v)] if v.is_canonical_nan() => {}
            [Value::F64(v)] if v.is_canonical_nan() => {}
            ref x => {
                panic!("unexpected value {:?}", NanCompare(x));
            }
        }
    }

    /// Handles an `assert_return_arithmetic_nan`.
    ///
    /// Per default panics if the result of handling the `action`
    /// does not result in single arithmetic NaN floating point value.
    ///
    /// Any canonical NaN is also a arithmetic NaN.
    ///
    /// Floating point values should, and per default are,
    /// compared according to their bit-pattern, and not their normal
    /// `PartialEq` semantic. See the `NanCompare` wrapper type.
    fn assert_return_arithmetic_nan(&mut self, action: Action) {
        let results = self.action(action);
        match *results {
            [Value::F32(v)] if v.is_arithmetic_nan() => {}
            [Value::F64(v)] if v.is_arithmetic_nan() => {}
            ref x => {
                panic!("unexpected value {:?}", NanCompare(x));
            }
        }
    }

    /// Register a loaded module under the name `as_name`.
    ///
    /// If `name` is `Some`, it should be registered under that name.
    /// In any case it should count as the least recently loaded module.
    fn register(&mut self, name: Option<String>, as_name: String);
}

/// Wrapper type that compares a list of `wabt` `Value`s
/// according to their bit-pattern if they contain floating point values,
/// and according to regular `PartialEq` semantics otherwise.
pub struct NanCompare<'a>(pub &'a [Value]);

impl<'a> ::std::cmp::PartialEq for NanCompare<'a> {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        self.0.iter().zip(other.0.iter()).all(|pair| {
            match pair {
                (Value::I32(l), Value::I32(r)) => l == r,
                (Value::I64(l), Value::I64(r)) => l == r,
                (Value::F32(l), Value::F32(r)) if l.is_nan() && r.is_nan() => {
                    l.payload() == r.payload()
                },
                (Value::F64(l), Value::F64(r)) if l.is_nan() && r.is_nan() => {
                    l.payload() == r.payload()
                },
                (Value::F32(l), Value::F32(r)) => l == r,
                (Value::F64(l), Value::F64(r)) => l == r,
                _ => false,
            }
        })
    }
}
impl<'a> ::std::fmt::Debug for NanCompare<'a> {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        formatter.debug_list().entries(self.0.iter().map(|e| {
            match e {
                Value::F32(v) if v.is_nan() => {
                    let p = v.payload();
                    format!("F32(NaN:0x{:x})", p)
                }
                Value::F64(v) if v.is_nan() => {
                    let p = v.payload();
                    format!("F64(NaN:0x{:x})", p)
                }
                _ => format!("{:?}", e)
            }
        })).finish()
    }
}

/// Result of invoking a function.
pub enum InvokationResult {
    /// The function returned successfully with a number of `Value`s
    Vals(Vec<Value>),
    /// The function trapped.
    Trap,
    /// The function exhausted the stack.
    StackExhaustion,
}

/// Extension trait for floating point values.
///
/// Provides methods for accessing the payload
/// of a NaN according to the webassembly spec.
///
/// According to the spec, any canonical NaN is also an arithmetic one.
pub trait NanPayload {
    /// Returns the payload bits of a NaN value.
    fn payload(&self) -> u64;
    /// Returns the number of significant digits in a NaN value.
    fn signif() -> u32;
    /// Returns positive infinite.
    fn infinite() -> Self;
    /// Returns the payload of a canonical NaN.
    fn canonical_payload() -> u64;
    /// Returns an arithmetic NaN with the given payload.
    fn arithmetic_nan(payload: u64) -> Self;
    /// Returns a canonical NaN.
    fn canonical_nan() -> Self;
    /// Checks if a value is an arithmetic NaN.
    fn is_arithmetic_nan(&self) -> bool;
    /// Checks if a value is a canonical NaN.
    fn is_canonical_nan(&self) -> bool;
}
impl NanPayload for f32 {
    fn payload(&self) -> u64 {
        assert!(self.is_nan());
        let bits: u32 = self.to_bits();
        let mask: u32 = (1u32 << 23) - 1;
        let p = bits & mask;
        p as u64
    }
    fn signif() -> u32 { 23 }
    fn infinite() -> Self { 1.0 / 0.0 }
    fn canonical_payload() -> u64 {
        1u64 << (Self::signif() - 1)
    }
    fn arithmetic_nan(payload: u64) -> Self {
        assert!(payload >= Self::canonical_payload());
        let bits: u32 = Self::infinite().to_bits();
        let mask: u32 = (1u32 << Self::signif()) - 1;
        let bits = bits | (mask & (payload as u32));
        Self::from_bits(bits)
    }
    fn canonical_nan() -> Self {
        Self::arithmetic_nan(Self::canonical_payload())
    }
    fn is_arithmetic_nan(&self) -> bool {
        self.is_nan()
    }
    fn is_canonical_nan(&self) -> bool {
        self.is_nan() && self.abs().to_bits() == Self::canonical_nan().to_bits()
    }
}
impl NanPayload for f64 {
    fn payload(&self) -> u64 {
        assert!(self.is_nan());
        let bits: u64 = self.to_bits();
        let mask: u64 = (1u64 << 52) - 1;
        let p = bits & mask;
        p
    }
    fn signif() -> u32 { 52 }
    fn infinite() -> Self { 1.0 / 0.0 }
    fn canonical_payload() -> u64 {
        1u64 << (Self::signif() - 1)
    }
    fn arithmetic_nan(payload: u64) -> Self {
        assert!(payload >= Self::canonical_payload());
        let bits: u64 = Self::infinite().to_bits();
        let mask: u64 = (1u64 << Self::signif()) - 1;
        let bits = bits | (mask & payload);
        Self::from_bits(bits)
    }
    fn canonical_nan() -> Self {
        Self::arithmetic_nan(Self::canonical_payload())
    }
    fn is_arithmetic_nan(&self) -> bool {
        self.is_nan()
    }
    fn is_canonical_nan(&self) -> bool {
        self.is_nan() && self.abs().to_bits() == Self::canonical_nan().to_bits()
    }
}

/// Result of running a series of script commands.
#[must_use]
pub struct SpectestResult {
    /// List of failed commands consisting of
    /// (filename, line number, panic message) tuples.
    pub failures: Vec<(String, u64, String)>,

    /// Number of successful commands.
    pub successes: usize,
}

impl SpectestResult {
    /// Displays the results in `Self` in a form similar
    /// to Rusts testsuite, and raises an panic if any tests failed.
    ///
    /// This is intended to be called from a `#[test]` function.
    pub fn present(self) {
        if self.failures.len() > 0 {
            println!("wast failures:");
            for (i, f) in self.failures.iter().enumerate() {
                println!("    {}:{}, {}", f.0, f.1, f.2);
                if i > 10 {
                    println!("    ...");
                    break;
                }
            }
            println!("wast total: {} passed; {} failed", self.successes, self.failures.len());
            panic!("some wast commands failed");
        } else {
            println!("wast total: {} passed; {} failed", self.successes, self.failures.len());
        }
    }
}

/// Run all scripts of the bundled webassembly testsuite on `handler`.
pub fn run_mvp_spectest<T: ScriptHandler>(handler: &mut T) -> SpectestResult {
    run_all_in_directory(format!("{}/testsuite", env!("CARGO_MANIFEST_DIR")).as_ref(), handler)
}

/// Module that is expected under the name "spectest" by all spectest testcases.
///
/// This is automatically registered by all `run_` functions in this modules
/// that work at file granularity or higher.
pub const SPECTEST_MODULE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/spec/spectest.wasm"));

/// Run all scripts in a given directory on `handler`.
pub fn run_all_in_directory<T: ScriptHandler>(path: &Path, handler: &mut T) -> SpectestResult {
    use std::fs;
    let mut res = SpectestResult {
        failures: vec![],
        successes: 0,
    };

    println!("\n\nRunning testsuite at {}:\n", path.display());

    'outer: for dir in fs::read_dir(&path).unwrap() {
        let dir = dir.unwrap();
        let path = dir.path();
        let filename = path.file_name().unwrap().to_str().unwrap();

        if path.metadata().unwrap().file_type().is_file() && filename.ends_with(".wast") {
            println!("Executing {} ...", filename);
            let res2 = run_single_file(&path, handler);
            res.successes += res2.successes;
            res.failures.extend(res2.failures);
        }
    }

    return res;
}

/// Run `handler` on the single `.wast` script file at `path`.
pub fn run_single_file<T: ScriptHandler>(path: &Path, handler: &mut T) -> SpectestResult {
    use std::fs;

    let mut res = SpectestResult {
        failures: vec![],
        successes: 0,
    };

    let filename = path.file_name().unwrap().to_str().unwrap();
    let source = fs::read(&path).unwrap();

    let mut script = ScriptParser::<>::from_source_and_name(&source, filename).unwrap();
    let mut fatal = false;

    handler.reset();
    {
        let module = SPECTEST_MODULE.to_vec();
        let name = Some("spectest".into());

        use std::panic::*;

        let r = if let Err(msg) = catch_unwind(AssertUnwindSafe(|| {
            handler.module(module, name);
        })) {
            let msg = if let Some(msg) = msg.downcast_ref::<String>() {
                msg.to_string()
            } else if let Some(msg) = msg.downcast_ref::<&'static str>() {
                msg.to_string()
            } else {
                "<unknown>".to_string()
            };
            Err(msg)
        } else {
            Ok(())
        };

        match r {
            Err(msg) => {
                res.failures.push(("<internal spectest module>".to_owned(), 0, msg));
                fatal = true;
            }
            Ok(()) => {
                res.successes += 1;
            }
        }
    }

    while let Some(Command { line, kind }) = script.next().unwrap() {
        if fatal {
            res.failures.push((filename.to_owned(), line, "<not attempted>".to_string()));
            continue;
        }
        match run_single_command(kind, handler) {
            Err(msg) => {
                res.failures.push((filename.to_owned(), line, msg));
                fatal = true;
            }
            Ok(()) => {
                res.successes += 1;
            }
        }
    }

    return res;
}

/// Run `handler` on a single wabt script command, catching any panic that
/// might happen in the process.
///
/// Note that `T` needs to be exception safe, in the sense that any
/// panic that happened during a method call should not affect it beyond
/// a subsequent `reset()` call.
pub fn run_single_command<T: ScriptHandler>(kind: CommandKind, handler: &mut T) -> Result<(), String> {
    use std::panic::*;

    if let Err(msg) = catch_unwind(AssertUnwindSafe(|| {
        run_single_command_no_catch(kind, handler);
    })) {
        let msg = if let Some(msg) = msg.downcast_ref::<String>() {
            msg.to_string()
        } else if let Some(msg) = msg.downcast_ref::<&'static str>() {
            msg.to_string()
        } else {
            "<unknown>".to_string()
        };
        Err(msg)
    } else {
        Ok(())
    }
}

/// Run `handler` on a single wabt script command.
///
/// Unlike `run_single_command`, this does not catch a panic.
pub fn run_single_command_no_catch<C: ScriptHandler>(cmd: CommandKind, c: &mut C) {
    // TODO: Figure out if the "message" fields need to actually be handled
    use wabt::script::CommandKind::*;
    match cmd {
        Module { module, name } => {
            c.module(module.into_vec(), name);
        }
        AssertReturn { action, expected } => {
            c.assert_return(action, expected);
        }
        AssertReturnCanonicalNan { action } => {
            c.assert_return_canonical_nan(action);
        }
        AssertReturnArithmeticNan { action } => {
            c.assert_return_arithmetic_nan(action);
        }
        AssertTrap { action, message: _ } => {
            c.assert_trap(action);
        }
        AssertInvalid { module, message: _ } => {
            c.assert_invalid(module.into_vec());
        }
        AssertMalformed { module, message: _ } => {
            c.assert_malformed(module.into_vec());
        }
        AssertUninstantiable { module, message: _ } => {
            c.assert_uninstantiable(module.into_vec());
        }
        AssertExhaustion { action } => {
            c.assert_exhaustion(action);
        }
        AssertUnlinkable { module, message: _ } => {
            c.assert_unlinkable(module.into_vec());
        }
        Register { name, as_name } => {
            c.register(name, as_name);
        }
        PerformAction(action) => {
            c.action(action);
        }
    }
}

/*
Spectest module .wat, based on wabt's spectest-interp.cc
static void InitEnvironment(Environment* env) {
  HostModule* host_module = env->AppendHostModule("spectest");
  host_module->AppendFuncExport("print", {{}, {}}, PrintCallback);
  host_module->AppendFuncExport("print_i32", {{Type::I32}, {}}, PrintCallback);
  host_module->AppendFuncExport("print_f32", {{Type::F32}, {}}, PrintCallback);
  host_module->AppendFuncExport("print_f64", {{Type::F64}, {}}, PrintCallback);
  host_module->AppendFuncExport("print_i32_f32", {{Type::I32, Type::F32}, {}},
                                PrintCallback);
  host_module->AppendFuncExport("print_f64_f64", {{Type::F64, Type::F64}, {}},
                                PrintCallback);
  host_module->AppendTableExport("table", Limits(10, 20));
  host_module->AppendMemoryExport("memory", Limits(1, 2));
  host_module->AppendGlobalExport("global_i32", false, uint32_t(666));
  host_module->AppendGlobalExport("global_i64", false, uint64_t(666));
  host_module->AppendGlobalExport("global_f32", false, float(666.6f));
  host_module->AppendGlobalExport("global_f64", false, double(666.6));
}
=>
(module
  (type (func))
  (type (func (param i32)))
  (type (func (param f32)))
  (type (func (param f64)))
  (type (func (param i32) (param f32)))
  (type (func (param f64) (param f64)))
  (func (type 0))
  (func (type 1))
  (func (type 2))
  (func (type 3))
  (func (type 4))
  (func (type 5))
  (table 10 20 anyfunc)
  (memory 1 2)
  (global i32 (i32.const 666))
  (global i64 (i64.const 666))
  (global f32 (f32.const 666.6))
  (global f64 (f64.const 666.6))
  (export "print" (func 0))
  (export "print_i32" (func 1))
  (export "print_f32" (func 2))
  (export "print_f64" (func 3))
  (export "print_i32_f32" (func 4))
  (export "print_f64_f64" (func 5))
  (export "table" (table 0))
  (export "memory" (memory 0))
  (export "global_i32" (global 0))
  (export "global_i64" (global 1))
  (export "global_f32" (global 2))
  (export "global_f64" (global 3))
)
the file spectest.wasm in the crate root contains a copy of it as a binary module
*/
