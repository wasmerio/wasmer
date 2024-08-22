//! Implementation of Wasm to CLIF memory access translation.
//!
//! Given
//!
//! * a dynamic Wasm memory index operand,
//! * a static offset immediate, and
//! * a static access size,
//!
//! bounds check the memory access and translate it into a native memory access.
//!
//! !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
//! !!!                                                                      !!!
//! !!!    THIS CODE IS VERY SUBTLE, HAS MANY SPECIAL CASES, AND IS ALSO     !!!
//! !!!   ABSOLUTELY CRITICAL FOR MAINTAINING THE SAFETY OF THE WASM HEAP    !!!
//! !!!                             SANDBOX.                                 !!!
//! !!!                                                                      !!!
//! !!!    A good rule of thumb is to get two reviews on any substantive     !!!
//! !!!                         changes in here.                             !!!
//! !!!                                                                      !!!
//! !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

use super::Reachability;
use crate::{
    heap::{HeapData, HeapStyle},
    translator::func_environ::FuncEnvironment,
};
use cranelift_codegen::{
    cursor::{Cursor, FuncCursor},
    ir::{self, condcodes::IntCC, InstBuilder, RelSourceLoc},
    ir::{Expr, Fact},
};
use cranelift_frontend::FunctionBuilder;
use wasmer_types::WasmResult;
use Reachability::*;

/// Helper used to emit bounds checks (as necessary) and compute the native
/// address of a heap access.
///
/// Returns the `ir::Value` holding the native address of the heap access, or
/// `None` if the heap access will unconditionally trap.
pub fn bounds_check_and_compute_addr<Env>(
    builder: &mut FunctionBuilder,
    env: &mut Env,
    heap: &HeapData,
    // Dynamic operand indexing into the heap.
    index: ir::Value,
    // Static immediate added to the index.
    offset: u32,
    // Static size of the heap access.
    access_size: u8,
) -> WasmResult<Reachability<ir::Value>>
where
    Env: FuncEnvironment + ?Sized,
{
    let pointer_bit_width = u16::try_from(env.pointer_type().bits()).unwrap();
    let orig_index = index;
    let index = cast_index_to_pointer_ty(
        index,
        heap.index_type,
        env.pointer_type(),
        heap.memory_type.is_some(),
        &mut builder.cursor(),
    );
    let offset_and_size = offset_plus_size(offset, access_size);
    let spectre_mitigations_enabled = env.heap_access_spectre_mitigation();
    let pcc = env.proof_carrying_code();

    let host_page_size_log2 = env.target_config().page_size_align_log2;
    let can_use_virtual_memory = heap.page_size_log2 >= host_page_size_log2;

    let make_compare = |builder: &mut FunctionBuilder,
                        compare_kind: IntCC,
                        lhs: ir::Value,
                        lhs_off: Option<i64>,
                        rhs: ir::Value,
                        rhs_off: Option<i64>| {
        let result = builder.ins().icmp(compare_kind, lhs, rhs);
        if pcc {
            // Name the original value as a def of the SSA value;
            // if the value was extended, name that as well with a
            // dynamic range, overwriting the basic full-range
            // fact that we previously put on the uextend.
            builder.func.dfg.facts[orig_index] = Some(Fact::Def { value: orig_index });
            if index != orig_index {
                builder.func.dfg.facts[index] = Some(Fact::value(pointer_bit_width, orig_index));
            }

            // Create a fact on the LHS that is a "trivial symbolic
            // fact": v1 has range v1+LHS_off..=v1+LHS_off
            builder.func.dfg.facts[lhs] = Some(Fact::value_offset(
                pointer_bit_width,
                orig_index,
                lhs_off.unwrap(),
            ));
            // If the RHS is a symbolic value (v1 or gv1), we can
            // emit a Compare fact.
            if let Some(rhs) = builder.func.dfg.facts[rhs]
                .as_ref()
                .and_then(|f| f.as_symbol())
            {
                builder.func.dfg.facts[result] = Some(Fact::Compare {
                    kind: compare_kind,
                    lhs: Expr::offset(&Expr::value(orig_index), lhs_off.unwrap()).unwrap(),
                    rhs: Expr::offset(rhs, rhs_off.unwrap()).unwrap(),
                });
            }
            // Likewise, if the RHS is a constant, we can emit a
            // Compare fact.
            if let Some(k) = builder.func.dfg.facts[rhs]
                .as_ref()
                .and_then(|f| f.as_const(pointer_bit_width))
            {
                builder.func.dfg.facts[result] = Some(Fact::Compare {
                    kind: compare_kind,
                    lhs: Expr::offset(&Expr::value(orig_index), lhs_off.unwrap()).unwrap(),
                    rhs: Expr::constant((k as i64).checked_add(rhs_off.unwrap()).unwrap()),
                });
            }
        }
        result
    };

    // We need to emit code that will trap (or compute an address that will trap
    // when accessed) if
    //
    //     index + offset + access_size > bound
    //
    // or if the `index + offset + access_size` addition overflows.
    //
    // Note that we ultimately want a 64-bit integer (we only target 64-bit
    // architectures at the moment) and that `offset` is a `u32` and
    // `access_size` is a `u8`. This means that we can add the latter together
    // as `u64`s without fear of overflow, and we only have to be concerned with
    // whether adding in `index` will overflow.
    //
    // Finally, the following right-hand sides of the matches do have a little
    // bit of duplicated code across them, but I think writing it this way is
    // worth it for readability and seeing very clearly each of our cases for
    // different bounds checks and optimizations of those bounds checks. It is
    // intentionally written in a straightforward case-matching style that will
    // hopefully make it easy to port to ISLE one day.
    Ok(match heap.style {
        // ====== Dynamic Memories ======
        //
        // 1. First special case for when `offset + access_size == 1`:
        //
        //            index + 1 > bound
        //        ==> index >= bound
        HeapStyle::Dynamic { bound_gv } if offset_and_size == 1 => {
            let bound = get_dynamic_heap_bound(builder, env, heap);
            let oob = make_compare(
                builder,
                IntCC::UnsignedGreaterThanOrEqual,
                index,
                Some(0),
                bound,
                Some(0),
            );
            Reachable(explicit_check_oob_condition_and_compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                access_size,
                spectre_mitigations_enabled,
                AddrPcc::dynamic(heap.memory_type, bound_gv),
                oob,
            ))
        }

        // 2. Second special case for when we know that there are enough guard
        //    pages to cover the offset and access size.
        //
        //    The precise should-we-trap condition is
        //
        //        index + offset + access_size > bound
        //
        //    However, if we instead check only the partial condition
        //
        //        index > bound
        //
        //    then the most out of bounds that the access can be, while that
        //    partial check still succeeds, is `offset + access_size`.
        //
        //    However, when we have a guard region that is at least as large as
        //    `offset + access_size`, we can rely on the virtual memory
        //    subsystem handling these out-of-bounds errors at
        //    runtime. Therefore, the partial `index > bound` check is
        //    sufficient for this heap configuration.
        //
        //    Additionally, this has the advantage that a series of Wasm loads
        //    that use the same dynamic index operand but different static
        //    offset immediates -- which is a common code pattern when accessing
        //    multiple fields in the same struct that is in linear memory --
        //    will all emit the same `index > bound` check, which we can GVN.
        HeapStyle::Dynamic { bound_gv }
            if can_use_virtual_memory && offset_and_size <= heap.offset_guard_size =>
        {
            let bound = get_dynamic_heap_bound(builder, env, heap);
            let oob = make_compare(
                builder,
                IntCC::UnsignedGreaterThan,
                index,
                Some(0),
                bound,
                Some(0),
            );
            Reachable(explicit_check_oob_condition_and_compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                access_size,
                spectre_mitigations_enabled,
                AddrPcc::dynamic(heap.memory_type, bound_gv),
                oob,
            ))
        }

        // 3. Third special case for when `offset + access_size <= min_size`.
        //
        //    We know that `bound >= min_size`, so we can do the following
        //    comparison, without fear of the right-hand side wrapping around:
        //
        //            index + offset + access_size > bound
        //        ==> index > bound - (offset + access_size)
        HeapStyle::Dynamic { bound_gv } if offset_and_size <= heap.min_size => {
            let bound = get_dynamic_heap_bound(builder, env, heap);
            let adjustment = offset_and_size as i64;
            let adjustment_value = builder.ins().iconst(env.pointer_type(), adjustment);
            if pcc {
                builder.func.dfg.facts[adjustment_value] =
                    Some(Fact::constant(pointer_bit_width, offset_and_size));
            }
            let adjusted_bound = builder.ins().isub(bound, adjustment_value);
            if pcc {
                builder.func.dfg.facts[adjusted_bound] = Some(Fact::global_value_offset(
                    pointer_bit_width,
                    bound_gv,
                    -adjustment,
                ));
            }
            let oob = make_compare(
                builder,
                IntCC::UnsignedGreaterThan,
                index,
                Some(0),
                adjusted_bound,
                Some(adjustment),
            );
            Reachable(explicit_check_oob_condition_and_compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                access_size,
                spectre_mitigations_enabled,
                AddrPcc::dynamic(heap.memory_type, bound_gv),
                oob,
            ))
        }

        // 4. General case for dynamic memories:
        //
        //        index + offset + access_size > bound
        //
        //    And we have to handle the overflow case in the left-hand side.
        HeapStyle::Dynamic { bound_gv } => {
            let access_size_val = builder
                .ins()
                // Explicit cast from u64 to i64: we just want the raw
                // bits, and iconst takes an `Imm64`.
                .iconst(env.pointer_type(), offset_and_size as i64);
            if pcc {
                builder.func.dfg.facts[access_size_val] =
                    Some(Fact::constant(pointer_bit_width, offset_and_size));
            }
            let adjusted_index = builder.ins().uadd_overflow_trap(
                index,
                access_size_val,
                ir::TrapCode::HeapOutOfBounds,
            );
            if pcc {
                builder.func.dfg.facts[adjusted_index] = Some(Fact::value_offset(
                    pointer_bit_width,
                    index,
                    i64::try_from(offset_and_size).unwrap(),
                ));
            }
            let bound = get_dynamic_heap_bound(builder, env, heap);
            let oob = make_compare(
                builder,
                IntCC::UnsignedGreaterThan,
                adjusted_index,
                i64::try_from(offset_and_size).ok(),
                bound,
                Some(0),
            );
            Reachable(explicit_check_oob_condition_and_compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                access_size,
                spectre_mitigations_enabled,
                AddrPcc::dynamic(heap.memory_type, bound_gv),
                oob,
            ))
        }

        // ====== Static Memories ======
        //
        // With static memories we know the size of the heap bound at compile
        // time.
        //
        // 1. First special case: trap immediately if `offset + access_size >
        //    bound`, since we will end up being out-of-bounds regardless of the
        //    given `index`.
        HeapStyle::Static { bound } if offset_and_size > bound => {
            assert!(
                can_use_virtual_memory,
                "static memories require the ability to use virtual memory"
            );
            env.before_unconditionally_trapping_memory_access(builder)?;
            builder.ins().trap(ir::TrapCode::HeapOutOfBounds);
            Unreachable
        }

        // 2. Second special case for when we can completely omit explicit
        //    bounds checks for 32-bit static memories.
        //
        //    First, let's rewrite our comparison to move all of the constants
        //    to one side:
        //
        //            index + offset + access_size > bound
        //        ==> index > bound - (offset + access_size)
        //
        //    We know the subtraction on the right-hand side won't wrap because
        //    we didn't hit the first special case.
        //
        //    Additionally, we add our guard pages (if any) to the right-hand
        //    side, since we can rely on the virtual memory subsystem at runtime
        //    to catch out-of-bound accesses within the range `bound .. bound +
        //    guard_size`. So now we are dealing with
        //
        //        index > bound + guard_size - (offset + access_size)
        //
        //    Note that `bound + guard_size` cannot overflow for
        //    correctly-configured heaps, as otherwise the heap wouldn't fit in
        //    a 64-bit memory space.
        //
        //    The complement of our should-this-trap comparison expression is
        //    the should-this-not-trap comparison expression:
        //
        //        index <= bound + guard_size - (offset + access_size)
        //
        //    If we know the right-hand side is greater than or equal to
        //    `u32::MAX`, then
        //
        //        index <= u32::MAX <= bound + guard_size - (offset + access_size)
        //
        //    This expression is always true when the heap is indexed with
        //    32-bit integers because `index` cannot be larger than
        //    `u32::MAX`. This means that `index` is always either in bounds or
        //    within the guard page region, neither of which require emitting an
        //    explicit bounds check.
        HeapStyle::Static { bound }
            if can_use_virtual_memory
                && heap.index_type == ir::types::I32
                && u64::from(u32::MAX) <= bound + heap.offset_guard_size - offset_and_size =>
        {
            assert!(
                can_use_virtual_memory,
                "static memories require the ability to use virtual memory"
            );
            Reachable(compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                AddrPcc::static32(heap.memory_type, bound + heap.offset_guard_size),
            ))
        }

        // 3. General case for static memories.
        //
        //    We have to explicitly test whether
        //
        //        index > bound - (offset + access_size)
        //
        //    and trap if so.
        //
        //    Since we have to emit explicit bounds checks, we might as well be
        //    precise, not rely on the virtual memory subsystem at all, and not
        //    factor in the guard pages here.
        HeapStyle::Static { bound } => {
            assert!(
                can_use_virtual_memory,
                "static memories require the ability to use virtual memory"
            );
            // NB: this subtraction cannot wrap because we didn't hit the first
            // special case.
            let adjusted_bound = bound - offset_and_size;
            let adjusted_bound_value = builder
                .ins()
                .iconst(env.pointer_type(), adjusted_bound as i64);
            if pcc {
                builder.func.dfg.facts[adjusted_bound_value] =
                    Some(Fact::constant(pointer_bit_width, adjusted_bound));
            }
            let oob = make_compare(
                builder,
                IntCC::UnsignedGreaterThan,
                index,
                Some(0),
                adjusted_bound_value,
                Some(0),
            );
            Reachable(explicit_check_oob_condition_and_compute_addr(
                &mut builder.cursor(),
                heap,
                env.pointer_type(),
                index,
                offset,
                access_size,
                spectre_mitigations_enabled,
                AddrPcc::static32(heap.memory_type, bound),
                oob,
            ))
        }
    })
}

/// Get the bound of a dynamic heap as an `ir::Value`.
fn get_dynamic_heap_bound<Env>(
    builder: &mut FunctionBuilder,
    env: &mut Env,
    heap: &HeapData,
) -> ir::Value
where
    Env: FuncEnvironment + ?Sized,
{
    let enable_pcc = heap.memory_type.is_some();

    let (value, gv) = match (heap.max_size, &heap.style) {
        // The heap has a constant size, no need to actually load the
        // bound.  TODO: this is currently disabled for PCC because we
        // can't easily prove that the GV load indeed results in a
        // constant (that information is lost in the CLIF). We'll want
        // to create an `iconst` GV expression kind to reify this fact
        // in the GV, then re-enable this opt. (Or, alternately,
        // compile such memories with a static-bound memtype and
        // facts.)
        (Some(max_size), HeapStyle::Dynamic { bound_gv })
            if heap.min_size == max_size && !enable_pcc =>
        {
            (
                builder.ins().iconst(env.pointer_type(), max_size as i64),
                *bound_gv,
            )
        }

        // Load the heap bound from its global variable.
        (_, HeapStyle::Dynamic { bound_gv }) => (
            builder.ins().global_value(env.pointer_type(), *bound_gv),
            *bound_gv,
        ),

        (_, HeapStyle::Static { .. }) => unreachable!("not a dynamic heap"),
    };

    // If proof-carrying code is enabled, apply a fact to the range to
    // tie it to the GV.
    if enable_pcc {
        builder.func.dfg.facts[value] = Some(Fact::global_value(
            u16::try_from(env.pointer_type().bits()).unwrap(),
            gv,
        ));
    }

    value
}

fn cast_index_to_pointer_ty(
    index: ir::Value,
    index_ty: ir::Type,
    pointer_ty: ir::Type,
    pcc: bool,
    pos: &mut FuncCursor,
) -> ir::Value {
    if index_ty == pointer_ty {
        return index;
    }
    // Note that using 64-bit heaps on a 32-bit host is not currently supported,
    // would require at least a bounds check here to ensure that the truncation
    // from 64-to-32 bits doesn't lose any upper bits. For now though we're
    // mostly interested in the 32-bit-heaps-on-64-bit-hosts cast.
    assert!(index_ty.bits() < pointer_ty.bits());

    // Convert `index` to `addr_ty`.
    let extended_index = pos.ins().uextend(pointer_ty, index);

    // Add a range fact on the extended value.
    if pcc {
        pos.func.dfg.facts[extended_index] = Some(Fact::max_range_for_width_extended(
            u16::try_from(index_ty.bits()).unwrap(),
            u16::try_from(pointer_ty.bits()).unwrap(),
        ));
    }

    // Add debug value-label alias so that debuginfo can name the extended
    // value as the address
    let loc = pos.srcloc();
    let loc = RelSourceLoc::from_base_offset(pos.func.params.base_srcloc(), loc);
    pos.func
        .stencil
        .dfg
        .add_value_label_alias(extended_index, loc, index);

    extended_index
}

/// Which facts do we want to emit for proof-carrying code, if any, on
/// address computations?
#[derive(Clone, Copy, Debug)]
enum AddrPcc {
    /// A 32-bit static memory with the given size.
    Static32(ir::MemoryType, u64),
    /// Dynamic bounds-check, with actual memory size (the `GlobalValue`)
    /// expressed symbolically.
    Dynamic(ir::MemoryType, ir::GlobalValue),
}
impl AddrPcc {
    fn static32(memory_type: Option<ir::MemoryType>, size: u64) -> Option<Self> {
        memory_type.map(|ty| Self::Static32(ty, size))
    }
    fn dynamic(memory_type: Option<ir::MemoryType>, bound: ir::GlobalValue) -> Option<Self> {
        memory_type.map(|ty| Self::Dynamic(ty, bound))
    }
}

/// Emit explicit checks on the given out-of-bounds condition for the Wasm
/// address and return the native address.
///
/// This function deduplicates explicit bounds checks and Spectre mitigations
/// that inherently also implement bounds checking.
#[allow(clippy::too_many_arguments)]
fn explicit_check_oob_condition_and_compute_addr(
    pos: &mut FuncCursor,
    heap: &HeapData,
    addr_ty: ir::Type,
    index: ir::Value,
    offset: u32,
    access_size: u8,
    // Whether Spectre mitigations are enabled for heap accesses.
    spectre_mitigations_enabled: bool,
    // Whether we're emitting PCC facts.
    pcc: Option<AddrPcc>,
    // The `i8` boolean value that is non-zero when the heap access is out of
    // bounds (and therefore we should trap) and is zero when the heap access is
    // in bounds (and therefore we can proceed).
    oob_condition: ir::Value,
) -> ir::Value {
    if !spectre_mitigations_enabled {
        pos.ins()
            .trapnz(oob_condition, ir::TrapCode::HeapOutOfBounds);
    }

    let mut addr = compute_addr(pos, heap, addr_ty, index, offset, pcc);

    if spectre_mitigations_enabled {
        let null = pos.ins().iconst(addr_ty, 0);
        addr = pos.ins().select_spectre_guard(oob_condition, null, addr);

        match pcc {
            None => {}
            Some(AddrPcc::Static32(ty, size)) => {
                pos.func.dfg.facts[null] =
                    Some(Fact::constant(u16::try_from(addr_ty.bits()).unwrap(), 0));
                pos.func.dfg.facts[addr] = Some(Fact::Mem {
                    ty,
                    min_offset: 0,
                    max_offset: size.checked_sub(u64::from(access_size)).unwrap(),
                    nullable: true,
                });
            }
            Some(AddrPcc::Dynamic(ty, gv)) => {
                pos.func.dfg.facts[null] =
                    Some(Fact::constant(u16::try_from(addr_ty.bits()).unwrap(), 0));
                pos.func.dfg.facts[addr] = Some(Fact::DynamicMem {
                    ty,
                    min: Expr::constant(0),
                    max: Expr::offset(
                        &Expr::global_value(gv),
                        i64::try_from(heap.offset_guard_size)
                            .unwrap()
                            .checked_sub(i64::from(access_size))
                            .unwrap(),
                    )
                    .unwrap(),
                    nullable: true,
                });
            }
        }
    }

    addr
}

/// Emit code for the native address computation of a Wasm address,
/// without any bounds checks or overflow checks.
///
/// It is the caller's responsibility to ensure that any necessary bounds and
/// overflow checks are emitted, and that the resulting address is never used
/// unless they succeed.
fn compute_addr(
    pos: &mut FuncCursor,
    heap: &HeapData,
    addr_ty: ir::Type,
    index: ir::Value,
    offset: u32,
    pcc: Option<AddrPcc>,
) -> ir::Value {
    debug_assert_eq!(pos.func.dfg.value_type(index), addr_ty);

    let heap_base = pos.ins().global_value(addr_ty, heap.base);

    match pcc {
        None => {}
        Some(AddrPcc::Static32(ty, _size)) => {
            pos.func.dfg.facts[heap_base] = Some(Fact::Mem {
                ty,
                min_offset: 0,
                max_offset: 0,
                nullable: false,
            });
        }
        Some(AddrPcc::Dynamic(ty, _limit)) => {
            pos.func.dfg.facts[heap_base] = Some(Fact::dynamic_base_ptr(ty));
        }
    }

    let base_and_index = pos.ins().iadd(heap_base, index);

    match pcc {
        None => {}
        Some(AddrPcc::Static32(ty, _) | AddrPcc::Dynamic(ty, _)) => {
            if let Some(idx) = pos.func.dfg.facts[index]
                .as_ref()
                .and_then(|f| f.as_symbol())
                .cloned()
            {
                pos.func.dfg.facts[base_and_index] = Some(Fact::DynamicMem {
                    ty,
                    min: idx.clone(),
                    max: idx,
                    nullable: false,
                });
            } else {
                pos.func.dfg.facts[base_and_index] = Some(Fact::Mem {
                    ty,
                    min_offset: 0,
                    max_offset: u64::from(u32::MAX),
                    nullable: false,
                });
            }
        }
    }

    if offset == 0 {
        base_and_index
    } else {
        // NB: The addition of the offset immediate must happen *before* the
        // `select_spectre_guard`, if any. If it happens after, then we
        // potentially are letting speculative execution read the whole first
        // 4GiB of memory.
        let offset_val = pos.ins().iconst(addr_ty, i64::from(offset));

        if pcc.is_some() {
            pos.func.dfg.facts[offset_val] = Some(Fact::constant(
                u16::try_from(addr_ty.bits()).unwrap(),
                u64::from(offset),
            ));
        }

        let result = pos.ins().iadd(base_and_index, offset_val);

        match pcc {
            None => {}
            Some(AddrPcc::Static32(ty, _) | AddrPcc::Dynamic(ty, _)) => {
                if let Some(idx) = pos.func.dfg.facts[index]
                    .as_ref()
                    .and_then(|f| f.as_symbol())
                {
                    pos.func.dfg.facts[result] = Some(Fact::DynamicMem {
                        ty,
                        min: idx.clone(),
                        // Safety: adding an offset to an expression with
                        // zero offset -- add cannot wrap, so `unwrap()`
                        // cannot fail.
                        max: Expr::offset(idx, i64::from(offset)).unwrap(),
                        nullable: false,
                    });
                } else {
                    pos.func.dfg.facts[result] = Some(Fact::Mem {
                        ty,
                        min_offset: u64::from(offset),
                        // Safety: can't overflow -- two u32s summed in a
                        // 64-bit add. TODO: when memory64 is supported here,
                        // `u32::MAX` is no longer true, and we'll need to
                        // handle overflow here.
                        max_offset: u64::from(u32::MAX) + u64::from(offset),
                        nullable: false,
                    });
                }
            }
        }
        result
    }
}

#[inline]
fn offset_plus_size(offset: u32, size: u8) -> u64 {
    // Cannot overflow because we are widening to `u64`.
    offset as u64 + size as u64
}
