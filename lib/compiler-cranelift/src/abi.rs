//! Native return-value ABI lowering for Cranelift.

use cranelift_codegen::{
    ir::{
        self, AbiParam, ArgumentPurpose, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind,
    },
    isa::TargetFrontendConfig,
};
use cranelift_frontend::FunctionBuilder;
use smallvec::{SmallVec, smallvec};
use target_lexicon::Architecture;
use wasmer_compiler::abi::{
    PairSlot, ReturnAbi, ReturnSlot, classify_return_type_aarch64, classify_return_type_riscv,
    classify_return_type_x86_64,
};
use wasmer_types::{FunctionType, Type};

use crate::translator::type_to_irtype;

/// Classify return values for a target architecture.
pub(crate) fn classify_returns(arch: Architecture, types: &[Type]) -> ReturnAbi {
    match arch {
        Architecture::X86_64 => classify_return_type_x86_64(types),
        Architecture::Aarch64(_) => classify_return_type_aarch64(types),
        Architecture::Riscv64(_) => classify_return_type_riscv(types, true),
        _ => unreachable!("unexpected architecture: {arch}"),
    }
}

fn natural_type(ty: Type, config: TargetFrontendConfig) -> ir::Type {
    type_to_irtype(ty, config).expect("supported WebAssembly signature type")
}

fn slot_type(slot: ReturnSlot, config: TargetFrontendConfig) -> ir::Type {
    match slot {
        ReturnSlot::Natural(ty) => natural_type(ty, config),
        ReturnSlot::Raw(Type::F32) => ir::types::I32,
        ReturnSlot::Raw(Type::F64) => ir::types::I64,
        ReturnSlot::Raw(ty) => natural_type(ty, config),
    }
}

fn pair_type(pair: PairSlot) -> ir::Type {
    match pair {
        PairSlot::F32Vector(_, _) => ir::types::F32X2,
        PairSlot::Raw(_, _) => ir::types::I64,
    }
}

/// Lower a WebAssembly signature to the native signature described by `ReturnAbi`.
pub(crate) fn signature_to_ir(
    signature: &FunctionType,
    config: TargetFrontendConfig,
    arch: Architecture,
) -> ir::Signature {
    let return_abi = classify_returns(arch, signature.results());
    let mut sig = ir::Signature::new(config.default_call_conv);

    if matches!(return_abi, ReturnAbi::Sret(_)) {
        sig.params.push(AbiParam::special(
            config.pointer_type(),
            ArgumentPurpose::StructReturn,
        ));
    }
    sig.params.push(AbiParam::special(
        config.pointer_type(),
        ArgumentPurpose::VMContext,
    ));
    sig.params.extend(
        signature
            .params()
            .iter()
            .map(|&ty| AbiParam::new(natural_type(ty, config))),
    );

    match return_abi {
        ReturnAbi::Void => {}
        ReturnAbi::Single(ty) => sig.returns.push(AbiParam::new(natural_type(ty, config))),
        ReturnAbi::Pair(a, b) => {
            sig.returns.push(AbiParam::new(slot_type(a, config)));
            sig.returns.push(AbiParam::new(slot_type(b, config)));
        }
        ReturnAbi::PackedPair(pair) => sig.returns.push(AbiParam::new(pair_type(pair))),
        ReturnAbi::PackedFirst(pair, slot) => {
            sig.returns.push(AbiParam::new(pair_type(pair)));
            sig.returns.push(AbiParam::new(slot_type(slot, config)));
        }
        ReturnAbi::PackedLast(slot, pair) => {
            sig.returns.push(AbiParam::new(slot_type(slot, config)));
            sig.returns.push(AbiParam::new(pair_type(pair)));
        }
        ReturnAbi::PackedQuads(a, b) => {
            sig.returns.push(AbiParam::new(pair_type(a)));
            sig.returns.push(AbiParam::new(pair_type(b)));
        }
        ReturnAbi::Unpacked(types) => sig.returns.extend(
            types
                .into_iter()
                .map(|ty| AbiParam::new(natural_type(ty, config))),
        ),
        // sret got already added as the very first argument
        ReturnAbi::Sret(_) => {}
    }
    sig
}

fn bitcast(builder: &mut FunctionBuilder, ty: ir::Type, value: ir::Value) -> ir::Value {
    if builder.func.dfg.value_type(value) == ty {
        value
    } else {
        let mut flags = MemFlagsData::new();
        flags.set_endianness(ir::Endianness::Little);
        builder.ins().bitcast(ty, flags, value)
    }
}

fn pack_slot(builder: &mut FunctionBuilder, value: ir::Value, slot: ReturnSlot) -> ir::Value {
    match slot {
        ReturnSlot::Natural(Type::V128) => bitcast(builder, ir::types::I8X16, value),
        ReturnSlot::Natural(_) => value,
        ReturnSlot::Raw(Type::F32) => bitcast(builder, ir::types::I32, value),
        ReturnSlot::Raw(Type::F64) => bitcast(builder, ir::types::I64, value),
        ReturnSlot::Raw(_) => value,
    }
}

fn unpack_slot(builder: &mut FunctionBuilder, value: ir::Value, slot: ReturnSlot) -> ir::Value {
    match slot {
        ReturnSlot::Natural(_) => value,
        ReturnSlot::Raw(Type::F32) => bitcast(builder, ir::types::F32, value),
        ReturnSlot::Raw(Type::F64) => bitcast(builder, ir::types::F64, value),
        ReturnSlot::Raw(_) => value,
    }
}

fn pack_pair(
    builder: &mut FunctionBuilder,
    first: ir::Value,
    second: ir::Value,
    pair: PairSlot,
) -> ir::Value {
    match pair {
        PairSlot::Raw(_, _) => {
            let low = bitcast(builder, ir::types::I32, first);
            let high = bitcast(builder, ir::types::I32, second);
            let low = builder.ins().uextend(ir::types::I64, low);
            let high = builder.ins().uextend(ir::types::I64, high);
            let high = builder.ins().ishl_imm(high, 32);
            builder.ins().bor(low, high)
        }
        PairSlot::F32Vector(_, _) => {
            let low = bitcast(builder, ir::types::I32, first);
            let high = bitcast(builder, ir::types::I32, second);
            let low = builder.ins().uextend(ir::types::I64, low);
            let high = builder.ins().uextend(ir::types::I64, high);
            let high = builder.ins().ishl_imm(high, 32);
            let bits = builder.ins().bor(low, high);
            bitcast(builder, ir::types::F32X2, bits)
        }
    }
}

/// Layout of the explicit structure-return area.
#[derive(Clone, Debug)]
pub(crate) struct ReturnAreaLayout {
    /// Byte offset of each result.
    pub(crate) offsets: Vec<i32>,
    /// Total allocation size.
    pub(crate) size: u32,
    /// Base-two logarithm of the allocation alignment.
    pub(crate) align_shift: u8,
}

fn type_size(ty: Type) -> u32 {
    match ty {
        Type::I32 | Type::F32 | Type::ExceptionRef => 4,
        Type::I64 | Type::F64 => 8,
        // Only 64-bit architectures are supported by Cranelift.
        Type::ExternRef | Type::FuncRef => 8,
        Type::V128 => 16,
    }
}

/// Compute the natural struct layout used for an explicit return area.
pub(crate) fn return_area_layout(types: &[Type]) -> ReturnAreaLayout {
    let mut offset = 0u32;
    let mut align = 1u32;
    let mut offsets = Vec::with_capacity(types.len());
    for &ty in types {
        let size = type_size(ty);
        align = align.max(size);
        offset = offset.next_multiple_of(size);
        offsets.push(i32::try_from(offset).unwrap());
        offset += size;
    }
    let size = offset.next_multiple_of(align);
    ReturnAreaLayout {
        offsets,
        size,
        align_shift: align.trailing_zeros() as u8,
    }
}

/// Allocate an explicit return area and return its address and layout.
pub(crate) fn allocate_return_area(
    builder: &mut FunctionBuilder,
    types: &[Type],
    config: TargetFrontendConfig,
) -> (ir::Value, ReturnAreaLayout) {
    let layout = return_area_layout(types);
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        layout.size,
        layout.align_shift,
    ));
    let ptr = builder.ins().stack_addr(config.pointer_type(), slot, 0);
    (ptr, layout)
}

/// Store natural Wasm return values in an explicit return area.
pub(crate) fn store_sret(
    builder: &mut FunctionBuilder,
    ptr: ir::Value,
    layout: &ReturnAreaLayout,
    values: &[ir::Value],
) {
    let flags = MemFlagsData::trusted();
    for (&value, &offset) in values.iter().zip(&layout.offsets) {
        builder.ins().store(flags, value, ptr, offset);
    }
}

/// Load natural Wasm return values from an explicit return area.
pub(crate) fn load_sret(
    builder: &mut FunctionBuilder,
    ptr: ir::Value,
    layout: &ReturnAreaLayout,
    types: &[Type],
    config: TargetFrontendConfig,
) -> SmallVec<[ir::Value; 4]> {
    let flags = MemFlagsData::trusted();
    types
        .iter()
        .zip(&layout.offsets)
        .map(|(&ty, &offset)| {
            builder
                .ins()
                .load(natural_type(ty, config), flags, ptr, offset)
        })
        .collect()
}

/// Pack natural Wasm values into native register-return carriers.
pub(crate) fn pack_register_returns(
    builder: &mut FunctionBuilder,
    abi: &ReturnAbi,
    values: &[ir::Value],
) -> SmallVec<[ir::Value; 4]> {
    match abi {
        ReturnAbi::Void => SmallVec::new(),
        ReturnAbi::Single(ty) => smallvec![if *ty == Type::V128 {
            bitcast(builder, ir::types::I8X16, values[0])
        } else {
            values[0]
        }],
        ReturnAbi::Pair(a, b) => smallvec![
            pack_slot(builder, values[0], *a),
            pack_slot(builder, values[1], *b)
        ],
        ReturnAbi::PackedPair(pair) => {
            smallvec![pack_pair(builder, values[0], values[1], *pair)]
        }
        ReturnAbi::PackedFirst(pair, slot) => smallvec![
            pack_pair(builder, values[0], values[1], *pair),
            pack_slot(builder, values[2], *slot)
        ],
        ReturnAbi::PackedLast(slot, pair) => smallvec![
            pack_slot(builder, values[0], *slot),
            pack_pair(builder, values[1], values[2], *pair)
        ],
        ReturnAbi::PackedQuads(a, b) => smallvec![
            pack_pair(builder, values[0], values[1], *a),
            pack_pair(builder, values[2], values[3], *b)
        ],
        ReturnAbi::Unpacked(_) => values.iter().copied().collect(),
        ReturnAbi::Sret(_) => panic!("sret values must be stored, not packed"),
    }
}

/// Unpack native register-return carriers into natural Wasm values.
pub(crate) fn unpack_register_returns(
    builder: &mut FunctionBuilder,
    abi: &ReturnAbi,
    values: &[ir::Value],
    config: TargetFrontendConfig,
) -> SmallVec<[ir::Value; 4]> {
    let unpack_pair_with_config = |builder: &mut FunctionBuilder, value, pair| match pair {
        PairSlot::Raw(first, second) => {
            let low = builder.ins().ireduce(ir::types::I32, value);
            let high = builder.ins().ushr_imm(value, 32);
            let high = builder.ins().ireduce(ir::types::I32, high);
            (
                bitcast(builder, natural_type(first, config), low),
                bitcast(builder, natural_type(second, config), high),
            )
        }
        PairSlot::F32Vector(_, _) => {
            let bits = bitcast(builder, ir::types::I64, value);
            let low = builder.ins().ireduce(ir::types::I32, bits);
            let high = builder.ins().ushr_imm(bits, 32);
            let high = builder.ins().ireduce(ir::types::I32, high);
            (
                bitcast(builder, ir::types::F32, low),
                bitcast(builder, ir::types::F32, high),
            )
        }
    };
    match abi {
        ReturnAbi::Void => SmallVec::new(),
        ReturnAbi::Single(_) => smallvec![values[0]],
        ReturnAbi::Pair(a, b) => smallvec![
            unpack_slot(builder, values[0], *a),
            unpack_slot(builder, values[1], *b)
        ],
        ReturnAbi::PackedPair(pair) => {
            let (a, b) = unpack_pair_with_config(builder, values[0], *pair);
            smallvec![a, b]
        }
        ReturnAbi::PackedFirst(pair, slot) => {
            let (a, b) = unpack_pair_with_config(builder, values[0], *pair);
            smallvec![a, b, unpack_slot(builder, values[1], *slot)]
        }
        ReturnAbi::PackedLast(slot, pair) => {
            let (a, b) = unpack_pair_with_config(builder, values[1], *pair);
            smallvec![unpack_slot(builder, values[0], *slot), a, b]
        }
        ReturnAbi::PackedQuads(a, b) => {
            let (a0, a1) = unpack_pair_with_config(builder, values[0], *a);
            let (b0, b1) = unpack_pair_with_config(builder, values[1], *b);
            smallvec![a0, a1, b0, b1]
        }
        ReturnAbi::Unpacked(_) => values.iter().copied().collect(),
        ReturnAbi::Sret(_) => panic!("sret values must be loaded, not unpacked"),
    }
}
