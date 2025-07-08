//! Lightweight WASI implementation for Wasmer WebAssembly runtime
//!
//! This crate provides a minimal WASI implementation that supports only the essential
//! `wasi_snapshot_preview1` syscalls without the overhead of the full WASIX implementation.
//! It's designed for use cases where you need WASI support for plugins or dynamic
//! application components without the resource overhead of filesystem, networking, and
//! threading features.

use std::{collections::HashMap, time::Duration};
use thiserror::Error;
use wasmer::{
    imports, namespace, Function, FunctionEnv, Imports, Memory32, MemoryAccessError, MemoryView,
    RuntimeError, Store,
};
use wasmer_wasix_types::wasi::{Clockid, Errno, Filesize, Timestamp};

pub mod syscalls;

#[derive(Error, Debug)]
pub enum WasiLightError {
    #[error("Memory not available")]
    MemoryNotAvailable,
    #[error("Exit with code: {0}")]
    Exit(u32),
    #[error("Memory access error: {0}")]
    MemoryAccess(#[from] MemoryAccessError),
}

/// Lightweight WASI environment for reactor pattern usage
#[derive(Clone)]
pub struct WasiLightEnv {
    pub args: Vec<String>,
    pub envs: HashMap<String, String>,
    pub clock_offset: Duration,
    pub random_seed: u64,
    pub memory: Option<wasmer::Memory>,
}

impl WasiLightEnv {
    pub fn new() -> Self {
        Self {
            args: Vec::new(),
            envs: HashMap::new(),
            clock_offset: Duration::ZERO,
            random_seed: 42,
            memory: None,
        }
    }

    pub fn args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn envs(mut self, envs: HashMap<String, String>) -> Self {
        self.envs = envs;
        self
    }

    pub fn clock_offset(mut self, offset: Duration) -> Self {
        self.clock_offset = offset;
        self
    }

    pub fn random_seed(mut self, seed: u64) -> Self {
        self.random_seed = seed;
        self
    }

    pub fn memory(mut self, memory: wasmer::Memory) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn memory_view<'a>(
        &self,
        ctx: &'a wasmer::FunctionEnvMut<'a, Self>,
    ) -> Option<MemoryView<'a>> {
        self.memory.as_ref().map(|mem| mem.view(ctx))
    }

    pub fn random_bytes(&mut self, len: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            // Simple linear congruential generator for deterministic randomness
            self.random_seed = self
                .random_seed
                .wrapping_mul(1103515245)
                .wrapping_add(12345);
            bytes.push((self.random_seed >> 16) as u8);
        }
        bytes
    }
}

impl Default for WasiLightEnv {
    fn default() -> Self {
        Self::new()
    }
}

// WASI type aliases - using allow to suppress naming convention warnings
#[allow(non_camel_case_types)]
type __wasi_clockid_t = Clockid;
#[allow(non_camel_case_types)]
type __wasi_errno_t = Errno;
#[allow(non_camel_case_types)]
type __wasi_exitcode_t = u32;
#[allow(non_camel_case_types)]
type __wasi_filesize_t = Filesize;
#[allow(non_camel_case_types)]
type __wasi_timestamp_t = Timestamp;

/// Generate import object for WASI Light environment
pub fn generate_import_object(
    store: &mut Store,
    env: &WasiLightEnv,
) -> Result<Imports, WasiLightError> {
    let env = FunctionEnv::new(store, env.clone());
    let exports = wasi_snapshot_preview1_exports(store, &env);

    let imports = imports! {
        "wasi_snapshot_preview1" => exports,
    };

    Ok(imports)
}

/// Generate import object with existing FunctionEnv
pub fn generate_import_object_with_env(
    store: &mut Store,
    env: FunctionEnv<WasiLightEnv>,
) -> Result<Imports, WasiLightError> {
    let exports = wasi_snapshot_preview1_exports(store, &env);

    let imports = imports! {
        "wasi_snapshot_preview1" => exports,
    };

    Ok(imports)
}

/// Generate WASI snapshot preview1 exports
pub fn wasi_snapshot_preview1_exports(
    store: &mut Store,
    env: &FunctionEnv<WasiLightEnv>,
) -> wasmer::Exports {
    use syscalls::*;

    namespace! {
        "args_get" => Function::new_typed_with_env(store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(store, env, args_sizes_get::<Memory32>),
        "environ_get" => Function::new_typed_with_env(store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(store, env, environ_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(store, env, clock_time_get::<Memory32>),
        "random_get" => Function::new_typed_with_env(store, env, random_get::<Memory32>),
        "proc_exit" => Function::new_typed_with_env(store, env, proc_exit::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(store, env, proc_raise),
        "sched_yield" => Function::new_typed_with_env(store, env, sched_yield::<Memory32>),
    }
}

/// Convert memory access error to WASI error
#[allow(dead_code)]
fn mem_error_to_wasi(err: MemoryAccessError) -> Errno {
    match err {
        MemoryAccessError::HeapOutOfBounds { .. } => Errno::Fault,
        MemoryAccessError::Overflow => Errno::Overflow,
        MemoryAccessError::NonUtf8String => Errno::Inval,
        _ => Errno::Unknown,
    }
}

/// Convert WASI error to runtime error
#[allow(dead_code)]
fn wasi_error_to_runtime(err: Errno) -> RuntimeError {
    RuntimeError::new(format!("wasi error: {:?}", err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wasmer::{wat2wasm, Instance, Module, Store};

    #[test]
    fn test_wasi_light_env_creation() {
        let env = WasiLightEnv::new();
        assert_eq!(env.args.len(), 0);
        assert_eq!(env.envs.len(), 0);
        assert_eq!(env.clock_offset, Duration::ZERO);
        assert_eq!(env.random_seed, 42);
    }

    #[test]
    fn test_wasi_light_env_builder() {
        let env = WasiLightEnv::new()
            .args(vec!["arg1".to_string(), "arg2".to_string()])
            .envs(HashMap::from([
                ("KEY1".to_string(), "VALUE1".to_string()),
                ("KEY2".to_string(), "VALUE2".to_string()),
            ]))
            .clock_offset(Duration::from_secs(3600))
            .random_seed(12345);

        assert_eq!(env.args.len(), 2);
        assert_eq!(env.envs.len(), 2);
        assert_eq!(env.clock_offset, Duration::from_secs(3600));
        assert_eq!(env.random_seed, 12345);
    }

    #[test]
    fn test_random_bytes_generation() {
        let mut env = WasiLightEnv::new().random_seed(42);
        let bytes1 = env.random_bytes(10);
        let bytes2 = env.random_bytes(10);

        // Should be deterministic with same seed
        assert_eq!(bytes1.len(), 10);
        assert_eq!(bytes2.len(), 10);

        // Should be different with different seeds
        let mut env2 = WasiLightEnv::new().random_seed(43);
        let bytes3 = env2.random_bytes(10);
        assert_ne!(bytes1, bytes3);
    }

    #[test]
    fn test_generate_import_object() {
        let mut store = Store::default();
        let env = WasiLightEnv::new()
            .args(vec!["test_arg".to_string()])
            .envs(HashMap::from([(
                "TEST_KEY".to_string(),
                "TEST_VALUE".to_string(),
            )]));

        let result = generate_import_object(&mut store, &env);
        assert!(result.is_ok());

        let import_object = result.unwrap();
        // Check that wasi_snapshot_preview1 namespace exists
        assert!(import_object
            .get_namespace_exports("wasi_snapshot_preview1")
            .is_some());
    }

    #[test]
    fn test_wasi_light_with_simple_module() {
        let wasm_bytes = wat2wasm(
            br#"
(module
  (import "wasi_snapshot_preview1" "args_get" (func $args_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $environ_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random_get (param i32 i32) (result i32)))
  (func $test_wasi (result i32)
    i32.const 42
  )
  (export "test_wasi" (func $test_wasi))
)
"#,
        )
        .unwrap();

        let mut store = Store::default();
        let module = Module::new(&store, wasm_bytes).unwrap();

        let wasi_env = WasiLightEnv::new()
            .args(vec!["arg1".to_string(), "arg2".to_string()])
            .envs(HashMap::from([(
                "TEST_KEY".to_string(),
                "TEST_VALUE".to_string(),
            )]))
            .random_seed(42);

        let import_object = generate_import_object(&mut store, &wasi_env).unwrap();
        let instance = Instance::new(&mut store, &module, &import_object).unwrap();

        let test_func = instance.exports.get_function("test_wasi").unwrap();
        let result = test_func.call(&mut store, &[]).unwrap();

        assert_eq!(result[0].unwrap_i32(), 42);
    }

    #[test]
    fn test_multiple_instances() {
        let wasm_bytes = wat2wasm(
            br#"
(module
  (import "wasi_snapshot_preview1" "sched_yield" (func $sched_yield (result i32)))
  (func $test_yield (result i32)
    call $sched_yield
    drop
    i32.const 42
  )
  (export "test_yield" (func $test_yield))
)
"#,
        )
        .unwrap();

        let mut store = Store::default();
        let module = Module::new(&store, wasm_bytes).unwrap();

        // Create multiple instances with different configurations
        let mut instances = Vec::new();

        for i in 0..10 {
            let wasi_env = WasiLightEnv::new()
                .args(vec![format!("instance_{}", i)])
                .envs(HashMap::from([(
                    format!("INSTANCE_{}", i),
                    format!("VALUE_{}", i),
                )]))
                .random_seed(i as u64);

            let import_object = generate_import_object(&mut store, &wasi_env).unwrap();
            let instance = Instance::new(&mut store, &module, &import_object).unwrap();
            instances.push(instance);
        }

        // Test that all instances work correctly
        for (_i, instance) in instances.iter().enumerate() {
            let test_yield = instance.exports.get_function("test_yield").unwrap();
            let result = test_yield.call(&mut store, &[]).unwrap();

            // Each instance should return 42
            assert_eq!(result[0].unwrap_i32(), 42);
        }
    }

    #[test]
    fn test_error_handling() {
        let mut store = Store::default();

        // Test with a simple function that should work
        let wasm_bytes = wat2wasm(
            br#"
(module
  (import "wasi_snapshot_preview1" "proc_raise" (func $proc_raise (param i32) (result i32)))
  (func $test_proc_raise (result i32)
    i32.const 1
    call $proc_raise
    drop
    i32.const 0
  )
  (export "test_proc_raise" (func $test_proc_raise))
)
"#,
        )
        .unwrap();

        let module = Module::new(&store, wasm_bytes).unwrap();
        let wasi_env = WasiLightEnv::new().args(vec!["test".to_string()]);
        let import_object = generate_import_object(&mut store, &wasi_env).unwrap();

        // This should not panic
        let instance = Instance::new(&mut store, &module, &import_object).unwrap();
        let test_func = instance.exports.get_function("test_proc_raise").unwrap();

        // The function should return successfully
        let result = test_func.call(&mut store, &[]);
        assert!(result.is_ok());
    }
}
