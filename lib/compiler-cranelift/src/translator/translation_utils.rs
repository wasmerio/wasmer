//! Helper functions and structures for the translation.

use super::func_environ::TargetEnvironment;
use cranelift_codegen::{
    binemit::Reloc,
    ir::{self, AbiParam},
    isa::TargetFrontendConfig,
};
use cranelift_frontend::FunctionBuilder;
use wasmer_compiler::{types::relocation::RelocationKind, wasm_unsupported, wasmparser};
use wasmer_types::{FunctionType, LibCall, Type, WasmError, WasmResult};

/// Helper function translate a Function signature into Cranelift Ir
pub fn signature_to_cranelift_ir(
    signature: &FunctionType,
    target_config: TargetFrontendConfig,
) -> ir::Signature {
    let mut sig = ir::Signature::new(target_config.default_call_conv);
    sig.params.extend(signature.params().iter().map(|&ty| {
        let cret_arg: ir::Type = type_to_irtype(ty, target_config)
            .expect("only numeric types are supported in function signatures");
        AbiParam::new(cret_arg)
    }));
    sig.returns.extend(signature.results().iter().map(|&ty| {
        let cret_arg: ir::Type = type_to_irtype(ty, target_config)
            .expect("only numeric types are supported in function signatures");
        AbiParam::new(cret_arg)
    }));
    // The Vmctx signature
    sig.params.insert(
        0,
        AbiParam::special(target_config.pointer_type(), ir::ArgumentPurpose::VMContext),
    );
    sig
}

/// Helper function translating wasmparser types to Cranelift types when possible.
pub fn reference_type(target_config: TargetFrontendConfig) -> WasmResult<ir::Type> {
    Ok(target_config.pointer_type())
}

/// Helper function translating wasmparser types to Cranelift types when possible.
pub fn type_to_irtype(ty: Type, target_config: TargetFrontendConfig) -> WasmResult<ir::Type> {
    match ty {
        Type::I32 => Ok(ir::types::I32),
        Type::I64 => Ok(ir::types::I64),
        Type::F32 => Ok(ir::types::F32),
        Type::F64 => Ok(ir::types::F64),
        Type::V128 => Ok(ir::types::I8X16),
        Type::ExternRef | Type::FuncRef => reference_type(target_config),
        Type::ExceptionRef => Err(wasm_unsupported!(
            "exnrefs are not supported yet in cranelift"
        )),
        // ty => Err(wasm_unsupported!("type_to_type: wasm type {:?}", ty)),
    }
}

/// Transform Cranelift LibCall into runtime LibCall
pub fn irlibcall_to_libcall(libcall: ir::LibCall) -> LibCall {
    match libcall {
        ir::LibCall::Probestack => LibCall::Probestack,
        ir::LibCall::CeilF32 => LibCall::CeilF32,
        ir::LibCall::CeilF64 => LibCall::CeilF64,
        ir::LibCall::FloorF32 => LibCall::FloorF32,
        ir::LibCall::FloorF64 => LibCall::FloorF64,
        ir::LibCall::TruncF32 => LibCall::TruncF32,
        ir::LibCall::TruncF64 => LibCall::TruncF64,
        ir::LibCall::NearestF32 => LibCall::NearestF32,
        ir::LibCall::NearestF64 => LibCall::NearestF64,
        _ => panic!("Unsupported libcall"),
    }
}

/// Transform Cranelift Reloc to compiler Relocation
pub fn irreloc_to_relocationkind(reloc: Reloc) -> RelocationKind {
    match reloc {
        Reloc::Abs4 => RelocationKind::Abs4,
        Reloc::Abs8 => RelocationKind::Abs8,
        Reloc::X86PCRel4 => RelocationKind::X86PCRel4,
        Reloc::X86CallPCRel4 => RelocationKind::X86CallPCRel4,
        Reloc::X86CallPLTRel4 => RelocationKind::X86CallPLTRel4,
        Reloc::X86GOTPCRel4 => RelocationKind::X86GOTPCRel4,
        Reloc::Arm64Call => RelocationKind::Arm64Call,
        Reloc::RiscvCallPlt => RelocationKind::RiscvCall,
        _ => panic!("The relocation {reloc} is not yet supported."),
    }
}

/// Create a `Block` with the given Wasm parameters.
pub fn block_with_params<'a, PE: TargetEnvironment + ?Sized>(
    builder: &mut FunctionBuilder,
    params: impl Iterator<Item = &'a wasmparser::ValType>,
    environ: &PE,
) -> WasmResult<ir::Block> {
    let block = builder.create_block();
    for ty in params.into_iter() {
        match ty {
            wasmparser::ValType::I32 => {
                builder.append_block_param(block, ir::types::I32);
            }
            wasmparser::ValType::I64 => {
                builder.append_block_param(block, ir::types::I64);
            }
            wasmparser::ValType::F32 => {
                builder.append_block_param(block, ir::types::F32);
            }
            wasmparser::ValType::F64 => {
                builder.append_block_param(block, ir::types::F64);
            }
            wasmparser::ValType::Ref(ty) => {
                if ty.is_extern_ref() || ty.is_func_ref() {
                    builder.append_block_param(block, environ.reference_type());
                } else {
                    return Err(WasmError::Unsupported(format!(
                        "unsupported reference type: {ty:?}"
                    )));
                }
            }
            wasmparser::ValType::V128 => {
                builder.append_block_param(block, ir::types::I8X16);
            }
        }
    }
    Ok(block)
}

/// Turns a `wasmparser` `f32` into a `Cranelift` one.
pub fn f32_translation(x: wasmparser::Ieee32) -> ir::immediates::Ieee32 {
    ir::immediates::Ieee32::with_bits(x.bits())
}

/// Turns a `wasmparser` `f64` into a `Cranelift` one.
pub fn f64_translation(x: wasmparser::Ieee64) -> ir::immediates::Ieee64 {
    ir::immediates::Ieee64::with_bits(x.bits())
}

/// Special VMContext value label. It is tracked as 0xffff_fffe label.
pub fn get_vmctx_value_label() -> ir::ValueLabel {
    const VMCTX_LABEL: u32 = 0xffff_fffe;
    ir::ValueLabel::from_u32(VMCTX_LABEL)
}
