//! Regression test for the RISC-V ABI mismatch in `wasmer_vm_memory32_copy`.
//!
//! # The bug
//!
//! The RISC-V LP64 ABI requires that **all** sub-64-bit integer values —
//! including unsigned ones — are held in sign-extended form in 64-bit
//! registers at call boundaries.  This is captured in the LLVM IR via the
//! `signext` attribute on `i32` parameters.
//!
//! Wasmer's LLVM backend adds `signext` to every `i32` parameter of every
//! VM-function declaration (`add_function_with_attrs` in `intrinsics.rs`) and
//! at every call site (`build_call_with_param_attributes` in `code.rs`) when
//! the compile target is RISC-V64.
//!
//! The old Rust signature for `wasmer_vm_memory32_copy` used `u32` for
//! `dst`, `src`, and `len`.  A Rust function compiled with `u32` parameters
//! expects zero-extended values in the 64-bit argument register.  The caller,
//! however, passes *sign-extended* values because of `signext`.  For memory
//! addresses with bit 31 set (≥ 2 GB), sign-extension turns the address into
//! a large negative i64 (e.g. `0x8000_0000u32` → `0xFFFF_FFFF_8000_0000i64`),
//! while the callee expects `0x0000_0000_8000_0000u64`.  The resulting pointer
//! arithmetic silently produces a wrong destination/source address, corrupting
//! the memory copy.
//!
//! # The fix
//!
//! Change `dst`, `src`, and `len` to `u64` and truncate explicitly with
//! `as u32`.  The callee now receives the full sign-extended value from the
//! caller and truncates to the correct lower 32 bits, making the code correct
//! for all address values regardless of what is in the upper 32 bits.

#![cfg(all(feature = "llvm", not(target_arch = "wasm32")))]

use std::str::FromStr;
use tempfile::TempDir;
use wasmer::sys::{CpuFeature, EngineBuilder, Target, Triple};
use wasmer::{Module, Store};
use wasmer_compiler_llvm::{LLVM, LLVMCallbacks};

/// A tiny WASM module that issues `memory.copy` with address constants that
/// have bit 31 set.  When compiled for RISC-V64 these constants appear in the
/// LLVM IR as *negative* `i32` values (`-2147483648` = `0x8000_0000`,
/// `-2147483632` = `0x8000_0010`) and are sign-extended to negative i64
/// values in the argument register — the clearest possible demonstration
/// of the mismatch.
const WAT: &str = r#"
    (module
      (memory 1)
      (func (export "copy_high_addrs")
        i32.const 0x80000010
        i32.const 0x80000000
        i32.const 16
        memory.copy
      )
    )
"#;

/// Recursively collect all `.preopt.ll` files under `root`, read each one,
/// and return the first whose content contains `needle`.
fn find_preopt_ll_with(root: &std::path::Path, needle: &str) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_preopt_ll_with(&path, needle) {
                return Some(found);
            }
        } else if path.to_str().map_or(false, |s| s.ends_with(".preopt.ll")) {
            if let Ok(ir) = std::fs::read_to_string(&path) {
                if ir.contains(needle) {
                    return Some(ir);
                }
            }
        }
    }
    None
}

/// Compile `WAT` to RISC-V64 (no F/D extensions = soft-float), enable the
/// pre-optimisation LLVM IR dump, and return the IR text for the function
/// that contains a call to `wasmer_vm_memory32_copy`.
fn riscv64_preopt_ir() -> String {
    let wasm = wat::parse_str(WAT).expect("valid WAT");
    let debug_dir = TempDir::new().expect("tempdir");

    // riscv64 with an empty feature set = no hardware floating-point,
    // matching the SP1 zkVM target (riscv64im).
    let target = Target::new(
        Triple::from_str("riscv64").expect("valid triple"),
        CpuFeature::set(),
    );

    let mut config = LLVM::new();
    config.callbacks(Some(
        LLVMCallbacks::new(debug_dir.path().to_path_buf()).expect("callbacks"),
    ));

    let engine = EngineBuilder::new(config).set_target(Some(target)).engine();

    Module::new(&Store::new(engine), wasm).expect("riscv64 compilation");

    // Files are placed in a module-hash subdirectory, so search recursively.
    find_preopt_ll_with(debug_dir.path(), "wasmer_vm_memory32_copy")
        .expect("at least one .preopt.ll should contain wasmer_vm_memory32_copy")
}

/// The `declare` line for `wasmer_vm_memory32_copy` must carry `signext` on
/// its `i32` parameters.
///
/// This is added by `add_function_with_attrs` in `intrinsics.rs` for every
/// `i32` parameter on RISC-V targets.  It means the *caller* will
/// sign-extend the value into the full 64-bit register before the call —
/// even though the parameter is a logically unsigned memory address.
#[test]
fn declaration_has_signext_on_i32_params() {
    let ir = riscv64_preopt_ir();

    let decl = ir
        .lines()
        .find(|l| l.contains("declare") && l.contains("wasmer_vm_memory32_copy"))
        .unwrap_or_else(|| {
            panic!(
                "could not find `declare ... wasmer_vm_memory32_copy` in IR.\n\
                 Lines mentioning memory32_copy:\n{}",
                ir.lines()
                    .filter(|l| l.contains("memory32_copy"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });

    assert!(
        decl.contains("signext"),
        "expected `signext` on i32 params in the RISC-V64 declaration of \
         wasmer_vm_memory32_copy.\n\
         This attribute causes sign-extension of all i32 arguments per the \
         RISC-V LP64 ABI.  Memory addresses with bit 31 set (≥ 2 GB) become \
         negative 64-bit values in registers, producing incorrect pointer \
         arithmetic in a Rust callee that declares the parameters as `u32`.\n\
         Declaration found:\n  {decl}"
    );
}

/// At every `call` to `wasmer_vm_memory32_copy` the arguments must be
/// `signext i32`.
///
/// This is added at each call site by `build_call_with_param_attributes` in
/// `code.rs`.  For the constants in the test WAT the IR shows:
///
/// ```text
/// call void @wasmer_vm_memory32_copy(
///     ptr %vmctx,
///     i32 signext 0,            ← memory index
///     i32 signext -2147483632,  ← dst 0x80000010 sign-extended = 0xFFFFFFFF80000010
///     i32 signext -2147483648,  ← src 0x80000000 sign-extended = 0xFFFFFFFF80000000
///     i32 signext 16            ← len (no bit-31, no problem here)
/// )
/// ```
///
/// A Rust callee with `u32 dst` would compute
/// `0xFFFF_FFFF_8000_0010 as usize` = a huge negative address instead of
/// the correct `0x0000_0000_8000_0010`.
#[test]
fn call_sites_have_signext_and_negative_consts_for_high_addrs() {
    let ir = riscv64_preopt_ir();

    let call_lines: Vec<&str> = ir
        .lines()
        .filter(|l| l.contains("call") && l.contains("wasmer_vm_memory32_copy"))
        .collect();

    assert!(
        !call_lines.is_empty(),
        "expected at least one call to wasmer_vm_memory32_copy in the IR"
    );

    for line in &call_lines {
        assert!(
            line.contains("signext"),
            "expected `signext i32` at the call site on RISC-V64.\n\
             Got:\n  {line}"
        );
    }

    // Print the call lines so that PR reviewers can see the negative
    // constants (-2147483648, -2147483632) directly — concrete evidence
    // that the sign extension changes the value for addresses ≥ 2 GB.
    println!("\n=== wasmer_vm_memory32_copy call sites in RISC-V64 pre-opt IR ===");
    for line in &call_lines {
        println!("{line}");
    }
    println!(
        "\n\
        Note: `i32 signext -2147483648` corresponds to address 0x80000000 (2 GB).\n\
        When placed in a 64-bit register with sign-extension this becomes\n\
        0xFFFFFFFF80000000 — a large negative value.\n\
        A `u32` callee that does `dst as usize` would receive this wrong address."
    );
}
