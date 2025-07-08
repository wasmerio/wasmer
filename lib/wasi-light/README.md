# `wasmer-wasi-light`

A lightweight WASI implementation for Wasmer WebAssembly runtime, designed for reactor-style usage with minimal resource overhead.

## Overview

Provides minimal WASI Preview 1 support without filesystem, networking, or threading overhead. Ideal for plugins, dynamic components, and scenarios requiring many fast instance creations.

## Key Features

- **Lightweight**: Minimal memory footprint and fast instantiation
- **Reactor-friendly**: Designed for multiple entrypoints and function calls
- **No resource overhead**: No filesystem, networking, or threading initialization
- **WASI Preview 1**: Standard `wasi_snapshot_preview1` interface
- **Configurable**: Args, envs, clock offset, random seed

## Implementation

### Core Components

- `WasiLightEnv`: Environment with args, envs, clock offset, random seed
- `generate_import_object()`: Creates WASI import object
- Syscalls: `args_get`, `environ_get`, `clock_time_get`, `random_get`, `proc_exit`, `sched_yield`

### Memory Management

- Optional memory instance for reactor pattern
- Memory access only when needed
- Proper error handling for missing memory

## Usage

```rust
use wasmer::{Instance, Module, Store};
use wasmer_wasi_light::{WasiLightEnv, generate_import_object};
use std::collections::HashMap;

// Create lightweight WASI environment
let wasi_env = WasiLightEnv::new()
    .args(vec!["arg1".to_string(), "arg2".to_string()])
    .envs(HashMap::from([
        ("KEY1".to_string(), "VALUE1".to_string()),
    ]))
    .clock_offset(Duration::from_secs(3600))
    .random_seed(42);

// Generate import object
let import_object = generate_import_object(&mut store, &wasi_env)?;

// Instantiate module
let instance = Instance::new(&mut store, &module, &import_object)?;

// Call exported functions
let func = instance.exports.get_function("your_function")?;
let result = func.call(&mut store, &[])?;
```

## Testing

Run all tests:
```bash
cargo test -p wasmer-wasi-light
```

Run specific test:
```bash
cargo test -p wasmer-wasi-light syscalls::tests::test_clock_res_get
```

## Supported WASI Functions

| Function | Description |
|----------|-------------|
| `args_get` / `args_sizes_get` | Command line arguments |
| `environ_get` / `environ_sizes_get` | Environment variables |
| `clock_res_get` / `clock_time_get` | Time functions |
| `random_get` | Random number generation |
| `proc_exit` / `proc_raise` | Process control |
| `sched_yield` | Yield execution |

## Comparison

| Feature | wasmer-wasix | wasmer-wasi-light |
|---------|-------------|-------------------|
| Filesystem | ✅ Full | ❌ None |
| Networking | ✅ Full | ❌ None |
| Threading | ✅ Full | ❌ None |
| Memory usage | High | Low |
| Instantiation | Slow | Fast |
| Reactor pattern | ❌ | ✅ |
| Resource overhead | High | Minimal |

## When to Use

**Use wasmer-wasi-light when:**
- Creating many instances quickly
- Need only basic WASI functions
- Want minimal resource overhead
- Building reactor-style applications

**Use wasmer-wasix when:**
- Need full WASI/WASIX features
- Require filesystem/network access
- Running complete applications

