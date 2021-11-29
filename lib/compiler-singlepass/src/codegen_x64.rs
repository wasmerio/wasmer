use crate::address_map::get_function_address_map;
use crate::machine::CodegenError;
use crate::{
    common_decl::*, config::Singlepass, emitter_x64::*, location::CombinedRegister,
    machine::MachineSpecific, machine_x64::Machine, x64_decl::*,
};
use dynasmrt::{x64::Assembler, DynamicLabel};
use smallvec::{smallvec, SmallVec};
use std::collections::BTreeMap;
use std::iter;
use wasmer_compiler::wasmparser::{
    MemoryImmediate, Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType,
};
use wasmer_compiler::{
    CallingConvention, CompiledFunction, CompiledFunctionFrameInfo, CustomSection,
    CustomSectionProtection, FunctionBody, FunctionBodyData, InstructionAddressMap, Relocation,
    RelocationTarget, SectionBody, SectionIndex, SourceLoc, TrapInformation,
};
use wasmer_types::{
    entity::{EntityRef, PrimaryMap, SecondaryMap},
    FunctionType,
};
use wasmer_types::{
    FunctionIndex, GlobalIndex, LocalFunctionIndex, LocalMemoryIndex, MemoryIndex, ModuleInfo,
    SignatureIndex, TableIndex, Type,
};
use wasmer_vm::{MemoryStyle, TableStyle, TrapCode, VMBuiltinFunctionIndex, VMOffsets};

type Assembler = VecAssembler<X64Relocation>;

/// The singlepass per-function code generator.
pub struct FuncGen<'a> {
    // Immutable properties assigned at creation time.
    /// Static module information.
    module: &'a ModuleInfo,

    /// ModuleInfo compilation config.
    config: &'a Singlepass,

    /// Offsets of vmctx fields.
    vmoffsets: &'a VMOffsets,

    // // Memory plans.
    memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,

    // // Table plans.
    // table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
    /// Function signature.
    signature: FunctionType,

    // Working storage.
    /// Memory locations of local variables.
    locals: Vec<Location>,

    /// Types of local variables, including arguments.
    local_types: Vec<WpType>,

    /// Value stack.
    value_stack: Vec<Location>,

    /// Metadata about floating point values on the stack.
    fp_stack: Vec<FloatValue>,

    /// A list of frames describing the current control stack.
    control_stack: Vec<ControlFrame>,

    /// Low-level machine state.
    machine: Machine,

    /// Nesting level of unreachable code.
    unreachable_depth: usize,

    /// Function state map. Not yet used in the reborn version but let's keep it.
    fsm: FunctionStateMap,

    /// Trap table.
    trap_table: TrapTable,

    /// Relocation information.
    relocations: Vec<Relocation>,

    /// A set of special labels for trapping.
    special_labels: SpecialLabelSet,

    /// The source location for the current operator.
    src_loc: u32,

    /// Map from byte offset into wasm function to range of native instructions.
    ///
    // Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,

    /// Calling convention to use.
    calling_convention: CallingConvention,
}

struct SpecialLabelSet {
    integer_division_by_zero: DynamicLabel,
    heap_access_oob: DynamicLabel,
    table_access_oob: DynamicLabel,
    indirect_call_null: DynamicLabel,
    bad_signature: DynamicLabel,
}

/// A trap table for a `RunnableModuleInfo`.
#[derive(Clone, Debug, Default)]
pub struct TrapTable {
    /// Mappings from offsets in generated machine code to the corresponding trap code.
    pub offset_to_code: BTreeMap<usize, TrapCode>,
}

/// Metadata about a floating-point value.
#[derive(Copy, Clone, Debug)]
struct FloatValue {
    /// Do we need to canonicalize the value before its bit pattern is next observed? If so, how?
    canonicalization: Option<CanonicalizeType>,

    /// Corresponding depth in the main value stack.
    depth: usize,
}

impl FloatValue {
    fn new(depth: usize) -> Self {
        FloatValue {
            canonicalization: None,
            depth,
        }
    }

    fn cncl_f32(depth: usize) -> Self {
        FloatValue {
            canonicalization: Some(CanonicalizeType::F32),
            depth,
        }
    }

    fn cncl_f64(depth: usize) -> Self {
        FloatValue {
            canonicalization: Some(CanonicalizeType::F64),
            depth,
        }
    }

    fn promote(self, depth: usize) -> FloatValue {
        FloatValue {
            canonicalization: match self.canonicalization {
                Some(CanonicalizeType::F32) => Some(CanonicalizeType::F64),
                Some(CanonicalizeType::F64) => panic!("cannot promote F64"),
                None => None,
            },
            depth,
        }
    }

    fn demote(self, depth: usize) -> FloatValue {
        FloatValue {
            canonicalization: match self.canonicalization {
                Some(CanonicalizeType::F64) => Some(CanonicalizeType::F32),
                Some(CanonicalizeType::F32) => panic!("cannot demote F32"),
                None => None,
            },
            depth,
        }
    }
}

/// Type of a pending canonicalization floating point value.
/// Sometimes we don't have the type information elsewhere and therefore we need to track it here.
#[derive(Copy, Clone, Debug)]
enum CanonicalizeType {
    F32,
    F64,
}

impl CanonicalizeType {
    fn to_size(&self) -> Size {
        match self {
            CanonicalizeType::F32 => Size::S32,
            CanonicalizeType::F64 => Size::S64,
        }
    }
}

trait PopMany<T> {
    fn peek1(&self) -> Result<&T, CodegenError>;
    fn pop1(&mut self) -> Result<T, CodegenError>;
    fn pop2(&mut self) -> Result<(T, T), CodegenError>;
}

impl<T> PopMany<T> for Vec<T> {
    fn peek1(&self) -> Result<&T, CodegenError> {
        self.last().ok_or_else(|| CodegenError {
            message: "peek1() expects at least 1 element".into(),
        })
    }
    fn pop1(&mut self) -> Result<T, CodegenError> {
        self.pop().ok_or_else(|| CodegenError {
            message: "pop1() expects at least 1 element".into(),
        })
    }
    fn pop2(&mut self) -> Result<(T, T), CodegenError> {
        if self.len() < 2 {
            return Err(CodegenError {
                message: "pop2() expects at least 2 elements".into(),
            });
        }

        let right = self.pop().unwrap();
        let left = self.pop().unwrap();
        Ok((left, right))
    }
}

trait WpTypeExt {
    fn is_float(&self) -> bool;
}

impl WpTypeExt for WpType {
    fn is_float(&self) -> bool {
        match self {
            WpType::F32 | WpType::F64 => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: SmallVec<[WpType; 1]>,
    pub value_stack_depth: usize,
    pub fp_stack_depth: usize,
    pub state: MachineState,
    pub state_diff_id: usize,
}

#[derive(Debug, Copy, Clone)]
pub enum IfElseState {
    None,
    If(DynamicLabel),
    Else,
}

/// Abstraction for a 2-input, 1-output operator. Can be an integer/floating-point
/// binop/cmpop.
struct I2O1 {
    loc_a: Location,
    loc_b: Location,
    ret: Location,
}

impl<'a> FuncGen<'a> {
    /// Set the source location of the Wasm to the given offset.
    pub fn set_srcloc(&mut self, offset: u32) {
        self.src_loc = offset;
    }

    fn get_location_released(&mut self, loc: Location) -> Location {
        self.machine.release_locations(&[loc]);
        loc
    }

    fn pop_value_released(&mut self) -> Location {
        let loc = self
            .value_stack
            .pop()
            .expect("pop_value_released: value stack is empty");
        self.get_location_released(loc)
    }

    /// Prepare data for binary operator with 2 inputs and 1 output.
    fn i2o1_prepare(&mut self, ty: WpType) -> I2O1 {
        let loc_b = self.pop_value_released();
        let loc_a = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &[(ty, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        self.value_stack.push(ret);
        I2O1 { loc_a, loc_b, ret }
    }

    fn mark_trappable(&mut self) {
        let state_diff_id = self.get_state_diff();
        let offset = self.machine.assembler_get_offset().0;
        self.fsm.trappable_offsets.insert(
            offset,
            OffsetInfo {
                end_offset: offset + 1,
                activate_offset: offset,
                diff_id: state_diff_id,
            },
        );
        self.fsm.wasm_offset_to_target_offset.insert(
            self.machine.state.wasm_inst_offset,
            SuspendOffset::Trappable(offset),
        );
    }

    /// Marks each address in the code range emitted by `f` with the trap code `code`.
    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize) {
        for i in begin..end {
            self.trap_table.offset_to_code.insert(i, code);
        }
        self.mark_instruction_address_end(begin);
    }

    /// Marks one address as trappable with trap code `code`.
    fn mark_address_with_trap_code(&mut self, code: TrapCode) {
        let offset = self.machine.assembler_get_offset().0;
        self.trap_table.offset_to_code.insert(offset, code);
        self.mark_instruction_address_end(offset);
    }

    /// Moves `loc` to a valid location for `div`/`idiv`.
    fn emit_relaxed_xdiv(
        &mut self,
        op: fn(&mut Assembler, Size, Location),
        sz: Size,
        loc: Location,
    ) {
        self.machine
            .specific
            .assembler
            .emit_cmp(sz, Location::Imm32(0), loc);
        self.machine.specific.assembler.emit_jmp(
            Condition::Equal,
            self.special_labels.integer_division_by_zero,
        );

        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                self.machine
                    .specific
                    .move_location(sz, loc, Location::GPR(GPR::RCX)); // must not be used during div (rax, rdx)
                self.mark_trappable();
                let offset = self.machine.assembler_get_offset().0;
                self.trap_table
                    .offset_to_code
                    .insert(offset, TrapCode::IntegerOverflow);
                op(
                    &mut self.machine.specific.assembler,
                    sz,
                    Location::GPR(GPR::RCX),
                );
                self.mark_instruction_address_end(offset);
            }
            _ => {
                self.mark_trappable();
                let offset = self.machine.assembler_get_offset().0;
                self.trap_table
                    .offset_to_code
                    .insert(offset, TrapCode::IntegerOverflow);
                op(&mut self.machine.specific.assembler, sz, loc);
                self.mark_instruction_address_end(offset);
            }
        }
    }

    /// Moves `src` and `dst` to valid locations for generic instructions.
    fn emit_relaxed_binop(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Location),
        sz: Size,
        src: Location,
        dst: Location,
    ) {
        enum RelaxMode {
            Direct,
            SrcToGPR,
            DstToGPR,
            BothToGPR,
        }
        let mode = match (src, dst) {
            (Location::GPR(_), Location::GPR(_))
                if (op as *const u8 == Assembler::emit_imul as *const u8) =>
            {
                RelaxMode::Direct
            }
            _ if (op as *const u8 == Assembler::emit_imul as *const u8) => RelaxMode::BothToGPR,

            (Location::Memory(_, _), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::Imm64(_)) | (Location::Imm64(_), Location::Imm32(_)) => {
                RelaxMode::BothToGPR
            }
            (_, Location::Imm32(_)) | (_, Location::Imm64(_)) => RelaxMode::DstToGPR,
            (Location::Imm64(_), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::GPR(_))
                if (op as *const u8 != Assembler::emit_mov as *const u8) =>
            {
                RelaxMode::SrcToGPR
            }
            (_, Location::SIMD(_)) => RelaxMode::SrcToGPR,
            _ => RelaxMode::Direct,
        };

        match mode {
            RelaxMode::SrcToGPR => {
                let temp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(sz, src, Location::GPR(temp));
                op(
                    &mut self.machine.specific.assembler,
                    sz,
                    Location::GPR(temp),
                    dst,
                );
                self.machine.release_temp_gpr(temp);
            }
            RelaxMode::DstToGPR => {
                let temp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(sz, dst, Location::GPR(temp));
                op(
                    &mut self.machine.specific.assembler,
                    sz,
                    src,
                    Location::GPR(temp),
                );
                self.machine.release_temp_gpr(temp);
            }
            RelaxMode::BothToGPR => {
                let temp_src = self.machine.acquire_temp_gpr().unwrap();
                let temp_dst = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(sz, src, Location::GPR(temp_src));
                self.machine
                    .specific
                    .move_location(sz, dst, Location::GPR(temp_dst));
                op(
                    &mut self.machine.specific.assembler,
                    sz,
                    Location::GPR(temp_src),
                    Location::GPR(temp_dst),
                );
                match dst {
                    Location::Memory(_, _) | Location::GPR(_) => {
                        self.machine
                            .specific
                            .move_location(sz, Location::GPR(temp_dst), dst);
                    }
                    _ => {}
                }
                self.machine.release_temp_gpr(temp_dst);
                self.machine.release_temp_gpr(temp_src);
            }
            RelaxMode::Direct => {
                op(&mut self.machine.specific.assembler, sz, src, dst);
            }
        }
    }

    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx(
        &mut self,
        op: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CodegenError> {
        self.emit_relaxed_avx_base(
            |this, src1, src2, dst| op(&mut this.machine.specific.assembler, src1, src2, dst),
            src1,
            src2,
            dst,
        )?;
        Ok(())
    }

    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx_base<F: FnOnce(&mut Self, XMM, XMMOrMemory, XMM)>(
        &mut self,
        op: F,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CodegenError> {
        let tmp1 = self.machine.acquire_temp_simd().unwrap();
        let tmp2 = self.machine.acquire_temp_simd().unwrap();
        let tmp3 = self.machine.acquire_temp_simd().unwrap();
        let tmpg = self.machine.acquire_temp_gpr().unwrap();

        let src1 = match src1 {
            Location::SIMD(x) => x,
            Location::GPR(_) | Location::Memory(_, _) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, src1, Location::SIMD(tmp1));
                tmp1
            }
            Location::Imm32(_) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, src1, Location::GPR(tmpg));
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(tmpg),
                    Location::SIMD(tmp1),
                );
                tmp1
            }
            Location::Imm64(_) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, src1, Location::GPR(tmpg));
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(tmpg),
                    Location::SIMD(tmp1),
                );
                tmp1
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_relaxed_avx_base src1: unreachable code".to_string(),
                })
            }
        };

        let src2 = match src2 {
            Location::SIMD(x) => XMMOrMemory::XMM(x),
            Location::Memory(base, disp) => XMMOrMemory::Memory(base, disp),
            Location::GPR(_) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, src2, Location::SIMD(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm32(_) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, src2, Location::GPR(tmpg));
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(tmpg),
                    Location::SIMD(tmp2),
                );
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm64(_) => {
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, src2, Location::GPR(tmpg));
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(tmpg),
                    Location::SIMD(tmp2),
                );
                XMMOrMemory::XMM(tmp2)
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_relaxed_avx_base src2: unreachable code".to_string(),
                })
            }
        };

        match dst {
            Location::SIMD(x) => {
                op(self, src1, src2, x);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                op(self, src1, src2, tmp3);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::SIMD(tmp3), dst);
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_relaxed_avx_base dst: unreachable code".to_string(),
                })
            }
        }

        self.machine.release_temp_gpr(tmpg);
        self.machine.release_temp_simd(tmp3);
        self.machine.release_temp_simd(tmp2);
        self.machine.release_temp_simd(tmp1);
        Ok(())
    }

    /// I32 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i32(&mut self, f: fn(&mut Assembler, Size, Location, Location)) {
        // Using Red Zone here.
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
        if loc_a != ret {
            let tmp = self.machine.acquire_temp_gpr().unwrap();
            self.machine
                .specific
                .emit_relaxed_mov(Size::S32, loc_a, Location::GPR(tmp));
            self.emit_relaxed_binop(f, Size::S32, loc_b, Location::GPR(tmp));
            self.machine
                .specific
                .emit_relaxed_mov(Size::S32, Location::GPR(tmp), ret);
            self.machine.release_temp_gpr(tmp);
        } else {
            self.emit_relaxed_binop(f, Size::S32, loc_b, ret);
        }
    }

    /// I64 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i64(&mut self, f: fn(&mut Assembler, Size, Location, Location)) {
        // Using Red Zone here.
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);

        if loc_a != ret {
            let tmp = self.machine.acquire_temp_gpr().unwrap();
            self.machine
                .specific
                .emit_relaxed_mov(Size::S64, loc_a, Location::GPR(tmp));
            self.emit_relaxed_binop(f, Size::S64, loc_b, Location::GPR(tmp));
            self.machine
                .specific
                .emit_relaxed_mov(Size::S64, Location::GPR(tmp), ret);
            self.machine.release_temp_gpr(tmp);
        } else {
            self.emit_relaxed_binop(f, Size::S64, loc_b, ret);
        }
    }

    /// I32 comparison with `loc_b` from input.
    fn emit_cmpop_i32_dynamic_b(
        &mut self,
        c: Condition,
        loc_b: Location,
    ) -> Result<(), CodegenError> {
        // Using Red Zone here.
        let loc_a = self.pop_value_released();

        let ret = self.machine.acquire_locations(
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, loc_b, loc_a);
                self.machine.specific.assembler.emit_set(c, x);
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0xff),
                    Location::GPR(x),
                );
            }
            Location::Memory(_, _) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, loc_b, loc_a);
                self.machine.specific.assembler.emit_set(c, tmp);
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0xff),
                    Location::GPR(tmp),
                );
                self.machine
                    .specific
                    .move_location(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_cmpop_i32_dynamic_b ret: unreachable code".to_string(),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I32 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i32(&mut self, c: Condition) -> Result<(), CodegenError> {
        let loc_b = self.pop_value_released();
        self.emit_cmpop_i32_dynamic_b(c, loc_b)?;
        Ok(())
    }

    /// I64 comparison with `loc_b` from input.
    fn emit_cmpop_i64_dynamic_b(
        &mut self,
        c: Condition,
        loc_b: Location,
    ) -> Result<(), CodegenError> {
        // Using Red Zone here.
        let loc_a = self.pop_value_released();

        let ret = self.machine.acquire_locations(
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S64, loc_b, loc_a);
                self.machine.specific.assembler.emit_set(c, x);
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0xff),
                    Location::GPR(x),
                );
            }
            Location::Memory(_, _) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S64, loc_b, loc_a);
                self.machine.specific.assembler.emit_set(c, tmp);
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0xff),
                    Location::GPR(tmp),
                );
                self.machine
                    .specific
                    .move_location(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_cmpop_i64_dynamic_b ret: unreachable code".to_string(),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I64 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i64(&mut self, c: Condition) -> Result<(), CodegenError> {
        let loc_b = self.pop_value_released();
        self.emit_cmpop_i64_dynamic_b(c, loc_b)?;
        Ok(())
    }

    /// I32 `lzcnt`/`tzcnt`/`popcnt` with operand popped from the virtual stack.
    fn emit_xcnt_i32(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) -> Result<(), CodegenError> {
        let loc = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm32(_) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(Size::S32, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S32,
                        Location::GPR(tmp),
                        Location::GPR(out_tmp),
                    );
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S32,
                        Location::GPR(tmp),
                        ret,
                    );
                }
                self.machine.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S32,
                        loc,
                        Location::GPR(out_tmp),
                    );
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.machine.specific.assembler, Size::S32, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_xcnt_i32 loc: unreachable code".to_string(),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I64 `lzcnt`/`tzcnt`/`popcnt` with operand popped from the virtual stack.
    fn emit_xcnt_i64(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) -> Result<(), CodegenError> {
        let loc = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(Size::S64, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S64,
                        Location::GPR(tmp),
                        Location::GPR(out_tmp),
                    );
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S64,
                        Location::GPR(tmp),
                        ret,
                    );
                }
                self.machine.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(
                        &mut self.machine.specific.assembler,
                        Size::S64,
                        loc,
                        Location::GPR(out_tmp),
                    );
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.machine.specific.assembler, Size::S64, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: "emit_xcnt_i64 loc: unreachable code".to_string(),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I32 shift with both operands popped from the virtual stack.
    fn emit_shift_i32(&mut self, f: fn(&mut Assembler, Size, Location, Location)) {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);

        self.machine
            .specific
            .assembler
            .emit_mov(Size::S32, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            self.machine
                .specific
                .emit_relaxed_mov(Size::S32, loc_a, ret);
        }

        f(
            &mut self.machine.specific.assembler,
            Size::S32,
            Location::GPR(GPR::RCX),
            ret,
        );
    }

    /// I64 shift with both operands popped from the virtual stack.
    fn emit_shift_i64(&mut self, f: fn(&mut Assembler, Size, Location, Location)) {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
        self.machine
            .specific
            .assembler
            .emit_mov(Size::S64, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            self.machine
                .specific
                .emit_relaxed_mov(Size::S64, loc_a, ret);
        }

        f(
            &mut self.machine.specific.assembler,
            Size::S64,
            Location::GPR(GPR::RCX),
            ret,
        );
    }

    /// Floating point (AVX) binary operation with both operands popped from the virtual stack.
    fn emit_fp_binop_avx(
        &mut self,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

        self.emit_relaxed_avx(f, loc_a, loc_b, ret)?;
        Ok(())
    }

    /// Floating point (AVX) comparison with both operands popped from the virtual stack.
    fn emit_fp_cmpop_avx(
        &mut self,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);

        self.emit_relaxed_avx(f, loc_a, loc_b, ret)?;

        // Workaround for behavior inconsistency among different backing implementations.
        // (all bits or only the least significant bit are set to one?)
        self.machine
            .specific
            .assembler
            .emit_and(Size::S32, Location::Imm32(1), ret);
        Ok(())
    }

    /// Floating point (AVX) unop with both operands popped from the virtual stack.
    fn emit_fp_unop_avx(
        &mut self,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let loc = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        self.value_stack.push(ret);
        self.emit_relaxed_avx(f, loc, loc, ret)?;
        Ok(())
    }

    /// Emits a System V / Windows call sequence.
    ///
    /// This function will not use RAX before `cb` is called.
    ///
    /// The caller MUST NOT hold any temporary registers allocated by `acquire_temp_gpr` when calling
    /// this function.
    fn emit_call_native<I: Iterator<Item = Location>, F: FnOnce(&mut Self)>(
        &mut self,
        cb: F,
        params: I,
    ) -> Result<(), CodegenError> {
        // Values pushed in this function are above the shadow region.
        self.machine
            .state
            .stack_values
            .push(MachineValue::ExplicitShadow);

        let params: Vec<_> = params.collect();

        // Save used GPRs.
        let used_gprs = self.machine.get_used_gprs();
        for r in used_gprs.iter() {
            self.machine
                .specific
                .assembler
                .emit_push(Size::S64, Location::GPR(*r));
            let content =
                self.machine.state.register_values[X64Register::GPR(*r).to_index().0].clone();
            if content == MachineValue::Undefined {
                return Err(CodegenError {
                    message: "emit_call_native: Undefined used_gprs content".to_string(),
                });
            }
            self.machine.state.stack_values.push(content);
        }

        // Save used XMM registers.
        let used_xmms = self.machine.get_used_simd();
        if used_xmms.len() > 0 {
            self.machine.specific.assembler.emit_sub(
                Size::S64,
                Location::Imm32((used_xmms.len() * 8) as u32),
                Location::GPR(GPR::RSP),
            );

            for (i, r) in used_xmms.iter().enumerate() {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::SIMD(*r),
                    Location::Memory(GPR::RSP, (i * 8) as i32),
                );
            }
            for r in used_xmms.iter().rev() {
                let content =
                    self.machine.state.register_values[X64Register::XMM(*r).to_index().0].clone();
                if content == MachineValue::Undefined {
                    return Err(CodegenError {
                        message: "emit_call_native: Undefined used_xmms content".to_string(),
                    });
                }
                self.machine.state.stack_values.push(content);
            }
        }
        let calling_convention = self.calling_convention;

        let stack_padding: usize = match calling_convention {
            CallingConvention::WindowsFastcall => 32,
            _ => 0,
        };

        let mut stack_offset: usize = 0;

        // Calculate stack offset.
        for (i, _param) in params.iter().enumerate() {
            if let Location::Memory(_, _) = Machine::get_param_location(1 + i, calling_convention) {
                stack_offset += 8;
            }
        }

        // Align stack to 16 bytes.
        if (self.machine.get_stack_offset()
            + used_gprs.len() * 8
            + used_xmms.len() * 8
            + stack_offset)
            % 16
            != 0
        {
            self.machine.specific.assembler.emit_sub(
                Size::S64,
                Location::Imm32(8),
                Location::GPR(GPR::RSP),
            );
            stack_offset += 8;
            self.machine
                .state
                .stack_values
                .push(MachineValue::Undefined);
        }

        let mut call_movs: Vec<(Location, GPR)> = vec![];
        // Prepare register & stack parameters.
        for (i, param) in params.iter().enumerate().rev() {
            let loc = Machine::get_param_location(1 + i, calling_convention);
            match loc {
                Location::GPR(x) => {
                    call_movs.push((*param, x));
                }
                Location::Memory(_, _) => {
                    match *param {
                        Location::GPR(x) => {
                            let content = self.machine.state.register_values
                                [X64Register::GPR(x).to_index().0]
                                .clone();
                            // FIXME: There might be some corner cases (release -> emit_call_native -> acquire?) that cause this assertion to fail.
                            // Hopefully nothing would be incorrect at runtime.

                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::SIMD(x) => {
                            let content = self.machine.state.register_values
                                [X64Register::XMM(x).to_index().0]
                                .clone();
                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::Memory(reg, offset) => {
                            if reg != GPR::RBP {
                                return Err(CodegenError {
                                    message: "emit_call_native loc param: unreachable code"
                                        .to_string(),
                                });
                            }
                            self.machine
                                .state
                                .stack_values
                                .push(MachineValue::CopyStackBPRelative(offset));
                            // TODO: Read value at this offset
                        }
                        _ => {
                            self.machine
                                .state
                                .stack_values
                                .push(MachineValue::Undefined);
                        }
                    }
                    match *param {
                        Location::Imm64(_) => {
                            // Dummy value slot to be filled with `mov`.
                            self.machine
                                .specific
                                .assembler
                                .emit_push(Size::S64, Location::GPR(GPR::RAX));

                            // Use R9 as the temporary register here, since:
                            // - It is a temporary register that is not used for any persistent value.
                            // - This register as an argument location is only written to after `sort_call_movs`.'
                            self.machine.reserve_unused_temp_gpr(GPR::R9);
                            self.machine.specific.move_location(
                                Size::S64,
                                *param,
                                Location::GPR(GPR::R9),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(GPR::R9),
                                Location::Memory(GPR::RSP, 0),
                            );
                            self.machine.release_temp_gpr(GPR::R9);
                        }
                        Location::SIMD(_) => {
                            // Dummy value slot to be filled with `mov`.
                            self.machine
                                .specific
                                .assembler
                                .emit_push(Size::S64, Location::GPR(GPR::RAX));

                            // XMM registers can be directly stored to memory.
                            self.machine.specific.move_location(
                                Size::S64,
                                *param,
                                Location::Memory(GPR::RSP, 0),
                            );
                        }
                        _ => self.machine.specific.assembler.emit_push(Size::S64, *param),
                    }
                }
                _ => {
                    return Err(CodegenError {
                        message: "emit_call_native loc: unreachable code".to_string(),
                    })
                }
            }
        }

        // Sort register moves so that register are not overwritten before read.
        sort_call_movs(&mut call_movs);

        // Emit register moves.
        for (loc, gpr) in call_movs {
            if loc != Location::GPR(gpr) {
                self.machine
                    .specific
                    .move_location(Size::S64, loc, Location::GPR(gpr));
            }
        }

        // Put vmctx as the first parameter.
        self.machine.specific.move_location(
            Size::S64,
            Location::GPR(Machine::get_vmctx_reg()),
            Machine::get_param_location(0, calling_convention),
        ); // vmctx

        if (self.machine.state.stack_values.len() % 2) != 1 {
            return Err(CodegenError {
                message: "emit_call_native: explicit shadow takes one slot".to_string(),
            });
        }

        if stack_padding > 0 {
            self.machine.specific.assembler.emit_sub(
                Size::S64,
                Location::Imm32(stack_padding as u32),
                Location::GPR(GPR::RSP),
            );
        }

        cb(self);

        // Offset needs to be after the 'call' instruction.
        // TODO: Now the state information is also inserted for internal calls (e.g. MemoryGrow). Is this expected?
        {
            let state_diff_id = self.get_state_diff();
            let offset = self.machine.assembler_get_offset().0;
            self.fsm.call_offsets.insert(
                offset,
                OffsetInfo {
                    end_offset: offset + 1,
                    activate_offset: offset,
                    diff_id: state_diff_id,
                },
            );
            self.fsm.wasm_offset_to_target_offset.insert(
                self.machine.state.wasm_inst_offset,
                SuspendOffset::Call(offset),
            );
        }

        // Restore stack.
        if stack_offset + stack_padding > 0 {
            self.machine.specific.assembler.emit_add(
                Size::S64,
                Location::Imm32((stack_offset + stack_padding) as u32),
                Location::GPR(GPR::RSP),
            );
            if (stack_offset % 8) != 0 {
                return Err(CodegenError {
                    message: "emit_call_native: Bad restoring stack alignement".to_string(),
                });
            }
            for _ in 0..stack_offset / 8 {
                self.machine.state.stack_values.pop().unwrap();
            }
        }

        // Restore XMMs.
        if !used_xmms.is_empty() {
            for (i, r) in used_xmms.iter().enumerate() {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(GPR::RSP, (i * 8) as i32),
                    Location::SIMD(*r),
                );
            }
            self.machine.specific.assembler.emit_add(
                Size::S64,
                Location::Imm32((used_xmms.len() * 8) as u32),
                Location::GPR(GPR::RSP),
            );
            for _ in 0..used_xmms.len() {
                self.machine.state.stack_values.pop().unwrap();
            }
        }

        // Restore GPRs.
        for r in used_gprs.iter().rev() {
            self.machine
                .specific
                .assembler
                .emit_pop(Size::S64, Location::GPR(*r));
            self.machine.state.stack_values.pop().unwrap();
        }

        if self.machine.state.stack_values.pop().unwrap() != MachineValue::ExplicitShadow {
            return Err(CodegenError {
                message: "emit_call_native: Popped value is not ExplicitShadow".to_string(),
            });
        }
        Ok(())
    }

    /// Emits a System V call sequence, specialized for labels as the call target.
    fn _emit_call_native_label<I: Iterator<Item = Location>>(
        &mut self,
        label: DynamicLabel,
        params: I,
    ) -> Result<(), CodegenError> {
        self.emit_call_native(
            |this| this.machine.specific.assembler.emit_call_label(label),
            params,
        )?;
        Ok(())
    }

    /// Emits a memory operation.
    fn memory_op<F: FnOnce(&mut Self, GPR) -> Result<(), CodegenError>>(
        &mut self,
        addr: Location,
        memarg: &MemoryImmediate,
        check_alignment: bool,
        value_size: usize,
        cb: F,
    ) -> Result<(), CodegenError> {
        let need_check = match self.memory_styles[MemoryIndex::new(0)] {
            MemoryStyle::Static { .. } => false,
            MemoryStyle::Dynamic { .. } => true,
        };

        let offset = if self.module.num_imported_memories != 0 {
            self.vmoffsets
                .vmctx_vmmemory_import_definition(MemoryIndex::new(0))
        } else {
            self.vmoffsets
                .vmctx_vmmemory_definition(LocalMemoryIndex::new(0))
        };
        let tmp_addr = self.machine.specific.pick_temp_gpr().unwrap();
        let begin = self.machine.specific.memory_op_begin(
            addr,
            memarg,
            check_alignment,
            value_size,
            need_check,
            self.module.num_imported_memories != 0,
            offset as i32,
            self.special_labels.heap_access_oob,
            tmp_addr,
        );
        cb(self, tmp_addr)?;
        let end = self.machine.specific.memory_op_end(tmp_addr);

        self.mark_address_range_with_trap_code(TrapCode::HeapAccessOutOfBounds, begin, end);
        Ok(())
    }

    /// Emits a memory operation.
    fn emit_compare_and_swap<F: FnOnce(&mut Self, GPR, GPR)>(
        &mut self,
        loc: Location,
        target: Location,
        ret: Location,
        memarg: &MemoryImmediate,
        value_size: usize,
        memory_sz: Size,
        stack_sz: Size,
        cb: F,
    ) -> Result<(), CodegenError> {
        if memory_sz > stack_sz {
            return Err(CodegenError {
                message: "emit_compare_and_swap: memory size > stack size".to_string(),
            });
        }

        let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
        let value = if loc == Location::GPR(GPR::R14) {
            GPR::R13
        } else {
            GPR::R14
        };
        self.machine
            .specific
            .assembler
            .emit_push(Size::S64, Location::GPR(value));

        self.machine
            .specific
            .move_location(stack_sz, loc, Location::GPR(value));

        let retry = self.machine.specific.assembler.get_label();
        self.machine.specific.emit_label(retry);

        self.memory_op(target, memarg, true, value_size, |this, addr| {
            this.machine.specific.load_address(
                memory_sz,
                Location::GPR(compare),
                Location::Memory(addr, 0),
            );
            this.machine
                .specific
                .move_location(stack_sz, Location::GPR(compare), ret);
            cb(this, compare, value);
            this.machine.specific.assembler.emit_lock_cmpxchg(
                memory_sz,
                Location::GPR(value),
                Location::Memory(addr, 0),
            );
            Ok(())
        })?;

        self.machine.specific.jmp_on_different(retry);

        self.machine
            .specific
            .assembler
            .emit_pop(Size::S64, Location::GPR(value));
        self.machine.release_temp_gpr(compare);
        Ok(())
    }

    // Checks for underflow/overflow/nan.
    fn emit_f32_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_label: Label,
        overflow_label: Label,
        nan_label: Label,
        succeed_label: Label,
    ) {
        let lower_bound = f32::to_bits(lower_bound);
        let upper_bound = f32::to_bits(upper_bound);

        let tmp = self.machine.acquire_temp_gpr().unwrap();
        let tmp_x = self.machine.acquire_temp_simd().unwrap();

        // Underflow.
        self.machine.specific.move_location(
            Size::S32,
            Location::Imm32(lower_bound),
            Location::GPR(tmp),
        );
        self.machine
            .specific
            .move_location(Size::S32, Location::GPR(tmp), Location::SIMD(tmp_x));
        self.machine
            .specific
            .assembler
            .emit_vcmpless(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        self.machine.specific.move_location(
            Size::S32,
            Location::Imm32(upper_bound),
            Location::GPR(tmp),
        );
        self.machine
            .specific
            .move_location(Size::S32, Location::GPR(tmp), Location::SIMD(tmp_x));
        self.machine
            .specific
            .assembler
            .emit_vcmpgess(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        self.machine
            .specific
            .assembler
            .emit_vcmpeqss(reg, XMMOrMemory::XMM(reg), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::Equal, nan_label);

        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, succeed_label);

        self.machine.release_temp_simd(tmp_x);
        self.machine.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F32.
    fn emit_f32_int_conv_check_trap(&mut self, reg: XMM, lower_bound: f32, upper_bound: f32) {
        let trap_overflow = self.machine.specific.assembler.get_label();
        let trap_badconv = self.machine.specific.assembler.get_label();
        let end = self.machine.specific.assembler.get_label();

        self.emit_f32_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            trap_overflow,
            trap_overflow,
            trap_badconv,
            end,
        );

        self.machine.specific.emit_label(trap_overflow);
        let offset = self.machine.assembler_get_offset().0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::IntegerOverflow);
        self.machine.specific.emit_illegal_op();
        self.mark_instruction_address_end(offset);

        self.machine.specific.emit_label(trap_badconv);

        let offset = self.machine.assembler_get_offset().0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::BadConversionToInteger);
        self.machine.specific.emit_illegal_op();
        self.mark_instruction_address_end(offset);

        self.machine.specific.emit_label(end);
    }

    fn emit_f32_int_conv_check_sat<
        F1: FnOnce(&mut Self),
        F2: FnOnce(&mut Self),
        F3: FnOnce(&mut Self),
        F4: FnOnce(&mut Self),
    >(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) {
        // As an optimization nan_cb is optional, and when set to None we turn
        // use 'underflow' as the 'nan' label. This is useful for callers who
        // set the return value to zero for both underflow and nan.

        let underflow = self.machine.specific.assembler.get_label();
        let overflow = self.machine.specific.assembler.get_label();
        let nan = if nan_cb.is_some() {
            self.machine.specific.assembler.get_label()
        } else {
            underflow
        };
        let convert = self.machine.specific.assembler.get_label();
        let end = self.machine.specific.assembler.get_label();

        self.emit_f32_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        self.machine.specific.emit_label(underflow);
        underflow_cb(self);
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, end);

        self.machine.specific.emit_label(overflow);
        overflow_cb(self);
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            self.machine.specific.emit_label(nan);
            cb(self);
            self.machine
                .specific
                .assembler
                .emit_jmp(Condition::None, end);
        }

        self.machine.specific.emit_label(convert);
        convert_cb(self);
        self.machine.specific.emit_label(end);
    }

    // Checks for underflow/overflow/nan.
    fn emit_f64_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_label: Label,
        overflow_label: Label,
        nan_label: Label,
        succeed_label: Label,
    ) {
        let lower_bound = f64::to_bits(lower_bound);
        let upper_bound = f64::to_bits(upper_bound);

        let tmp = self.machine.acquire_temp_gpr().unwrap();
        let tmp_x = self.machine.acquire_temp_simd().unwrap();

        // Underflow.
        self.machine.specific.move_location(
            Size::S64,
            Location::Imm64(lower_bound),
            Location::GPR(tmp),
        );
        self.machine
            .specific
            .move_location(Size::S64, Location::GPR(tmp), Location::SIMD(tmp_x));
        self.machine
            .specific
            .assembler
            .emit_vcmplesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        self.machine.specific.move_location(
            Size::S64,
            Location::Imm64(upper_bound),
            Location::GPR(tmp),
        );
        self.machine
            .specific
            .move_location(Size::S64, Location::GPR(tmp), Location::SIMD(tmp_x));
        self.machine
            .specific
            .assembler
            .emit_vcmpgesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        self.machine
            .specific
            .assembler
            .emit_vcmpeqsd(reg, XMMOrMemory::XMM(reg), tmp_x);
        self.machine
            .specific
            .move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::Equal, nan_label);

        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, succeed_label);

        self.machine.release_temp_simd(tmp_x);
        self.machine.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F64.
    fn emit_f64_int_conv_check_trap(&mut self, reg: XMM, lower_bound: f64, upper_bound: f64) {
        let trap_overflow = self.machine.specific.assembler.get_label();
        let trap_badconv = self.machine.specific.assembler.get_label();
        let end = self.machine.specific.assembler.get_label();

        self.emit_f64_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            trap_overflow,
            trap_overflow,
            trap_badconv,
            end,
        );

        self.machine.specific.emit_label(trap_overflow);
        let offset = self.machine.assembler_get_offset().0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::IntegerOverflow);
        self.machine.specific.emit_illegal_op();
        self.mark_instruction_address_end(offset);

        self.machine.specific.emit_label(trap_badconv);
        let offset = self.machine.assembler_get_offset().0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::BadConversionToInteger);
        self.machine.specific.emit_illegal_op();
        self.mark_instruction_address_end(offset);

        self.machine.specific.emit_label(end);
    }

    fn emit_f64_int_conv_check_sat<
        F1: FnOnce(&mut Self),
        F2: FnOnce(&mut Self),
        F3: FnOnce(&mut Self),
        F4: FnOnce(&mut Self),
    >(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) {
        // As an optimization nan_cb is optional, and when set to None we turn
        // use 'underflow' as the 'nan' label. This is useful for callers who
        // set the return value to zero for both underflow and nan.

        let underflow = self.machine.specific.assembler.get_label();
        let overflow = self.machine.specific.assembler.get_label();
        let nan = if nan_cb.is_some() {
            self.machine.specific.assembler.get_label()
        } else {
            underflow
        };
        let convert = self.machine.specific.assembler.get_label();
        let end = self.machine.specific.assembler.get_label();

        self.emit_f64_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        self.machine.specific.emit_label(underflow);
        underflow_cb(self);
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, end);

        self.machine.specific.emit_label(overflow);
        overflow_cb(self);
        self.machine
            .specific
            .assembler
            .emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            self.machine.specific.emit_label(nan);
            cb(self);
            self.machine
                .specific
                .assembler
                .emit_jmp(Condition::None, end);
        }

        self.machine.specific.emit_label(convert);
        convert_cb(self);
        self.machine.specific.emit_label(end);
    }

    pub fn get_state_diff(&mut self) -> usize {
        if !self.machine.track_state {
            return std::usize::MAX;
        }
        let last_frame = self.control_stack.last_mut().unwrap();
        let mut diff = self.machine.state.diff(&last_frame.state);
        diff.last = Some(last_frame.state_diff_id);
        let id = self.fsm.diffs.len();
        last_frame.state = self.machine.state.clone();
        last_frame.state_diff_id = id;
        self.fsm.diffs.push(diff);
        id
    }

    fn emit_head(&mut self) -> Result<(), CodegenError> {
        // TODO: Patchpoint is not emitted for now, and ARM trampoline is not prepended.

        // Normal x86 entry prologue.
        self.machine.specific.emit_function_prolog();

        // Initialize locals.
        self.locals = self.machine.init_locals(
            self.local_types.len(),
            self.signature.params().len(),
            self.calling_convention,
        );

        // Mark vmctx register. The actual loading of the vmctx value is handled by init_local.
        self.machine.state.register_values
            [X64Register::GPR(Machine::get_vmctx_reg()).to_index().0] = MachineValue::Vmctx;

        // TODO: Explicit stack check is not supported for now.
        let diff = self.machine.state.diff(&new_machine_state());
        let state_diff_id = self.fsm.diffs.len();
        self.fsm.diffs.push(diff);

        self.machine.specific.assembler.emit_sub(
            Size::S64,
            Location::Imm32(32),
            Location::GPR(GPR::RSP),
        ); // simulate "red zone" if not supported by the platform

        self.control_stack.push(ControlFrame {
            label: self.machine.specific.assembler.get_label(),
            loop_like: false,
            if_else: IfElseState::None,
            returns: self
                .signature
                .results()
                .iter()
                .map(|&x| type_to_wp_type(x))
                .collect(),
            value_stack_depth: 0,
            fp_stack_depth: 0,
            state: self.machine.state.clone(),
            state_diff_id,
        });

        // TODO: Full preemption by explicit signal checking

        // We insert set StackOverflow as the default trap that can happen
        // anywhere in the function prologue.
        let offset = 0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::StackOverflow);
        self.mark_instruction_address_end(offset);

        if self.machine.state.wasm_inst_offset != std::usize::MAX {
            return Err(CodegenError {
                message: "emit_head: wasm_inst_offset not std::usize::MAX".to_string(),
            });
        }
        Ok(())
    }

    /// Pushes the instruction to the address map, calculating the offset from a
    /// provided beginning address.
    fn mark_instruction_address_end(&mut self, begin: usize) {
        self.instructions_address_map.push(InstructionAddressMap {
            srcloc: SourceLoc::new(self.src_loc),
            code_offset: begin,
            code_len: self.machine.specific.assembler.get_offset().0 - begin,
        });
    }

    pub fn new(
        module: &'a ModuleInfo,
        config: &'a Singlepass,
        vmoffsets: &'a VMOffsets,
        memory_styles: &'a PrimaryMap<MemoryIndex, MemoryStyle>,
        _table_styles: &'a PrimaryMap<TableIndex, TableStyle>,
        local_func_index: LocalFunctionIndex,
        local_types_excluding_arguments: &[WpType],
        calling_convention: CallingConvention,
    ) -> Result<FuncGen<'a>, CodegenError> {
        let func_index = module.func_index(local_func_index);
        let sig_index = module.functions[func_index];
        let signature = module.signatures[sig_index].clone();

        let mut local_types: Vec<_> = signature
            .params()
            .iter()
            .map(|&x| type_to_wp_type(x))
            .collect();
        local_types.extend_from_slice(&local_types_excluding_arguments);

        let fsm = FunctionStateMap::new(
            new_machine_state(),
            local_func_index.index() as usize,
            32,
            (0..local_types.len())
                .map(|_| WasmAbstractValue::Runtime)
                .collect(),
        );

        let mut machine = Machine::new();
        let special_labels = SpecialLabelSet {
            integer_division_by_zero: machine.specific.get_label(),
            heap_access_oob: machine.specific.get_label(),
            table_access_oob: machine.specific.get_label(),
            indirect_call_null: machine.specific.get_label(),
            bad_signature: machine.specific.get_label(),
        };

        let mut fg = FuncGen {
            module,
            config,
            vmoffsets,
            memory_styles,
            // table_styles,
            signature,
            locals: vec![], // initialization deferred to emit_head
            local_types,
            value_stack: vec![],
            fp_stack: vec![],
            control_stack: vec![],
            machine: machine,
            unreachable_depth: 0,
            fsm,
            trap_table: TrapTable::default(),
            relocations: vec![],
            special_labels,
            src_loc: 0,
            instructions_address_map: vec![],
            calling_convention,
        };
        fg.emit_head()?;
        Ok(fg)
    }

    pub fn has_control_frames(&self) -> bool {
        !self.control_stack.is_empty()
    }

    pub fn feed_operator(&mut self, op: Operator) -> Result<(), CodegenError> {
        assert!(self.fp_stack.len() <= self.value_stack.len());

        self.machine.state.wasm_inst_offset = self.machine.state.wasm_inst_offset.wrapping_add(1);

        //println!("{:?} {}", op, self.value_stack.len());
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;

            match op {
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.unreachable_depth += 1;
                }
                Operator::End => {
                    self.unreachable_depth -= 1;
                }
                Operator::Else => {
                    // We are in a reachable true branch
                    if self.unreachable_depth == 1 {
                        if let Some(IfElseState::If(_)) =
                            self.control_stack.last().map(|x| x.if_else)
                        {
                            self.unreachable_depth -= 1;
                        }
                    }
                }
                _ => {}
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }

        match op {
            Operator::GlobalGet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);

                let ty = type_to_wp_type(self.module.globals[global_index].ty);
                if ty.is_float() {
                    self.fp_stack.push(FloatValue::new(self.value_stack.len()));
                }
                let loc = self.machine.acquire_locations(
                    &[(ty, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(loc);

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                let src = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                };

                self.machine.specific.emit_relaxed_mov(Size::S64, src, loc);

                self.machine.release_temp_gpr(tmp);
            }
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                let dst = if let Some(local_global_index) =
                    self.module.local_global_index(global_index)
                {
                    let offset = self.vmoffsets.vmctx_vmglobal_definition(local_global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                } else {
                    // Imported globals require one level of indirection.
                    let offset = self
                        .vmoffsets
                        .vmctx_vmglobal_import_definition(global_index);
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), offset as i32),
                        Location::GPR(tmp),
                    );
                    Location::Memory(tmp, 0)
                };
                let ty = type_to_wp_type(self.module.globals[global_index].ty);
                let loc = self.pop_value_released();
                if ty.is_float() {
                    let fp = self.fp_stack.pop1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match ty {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            dst,
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(Size::S64, loc, dst);
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(Size::S64, loc, dst);
                }
                self.machine.release_temp_gpr(tmp);
            }
            Operator::LocalGet { local_index } => {
                let local_index = local_index as usize;
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, self.locals[local_index], ret);
                self.value_stack.push(ret);
                if self.local_types[local_index].is_float() {
                    self.fp_stack
                        .push(FloatValue::new(self.value_stack.len() - 1));
                }
            }
            Operator::LocalSet { local_index } => {
                let local_index = local_index as usize;
                let loc = self.pop_value_released();

                if self.local_types[local_index].is_float() {
                    let fp = self.fp_stack.pop1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match self.local_types[local_index] {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            self.locals[local_index],
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            self.locals[local_index],
                        );
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        loc,
                        self.locals[local_index],
                    );
                }
            }
            Operator::LocalTee { local_index } => {
                let local_index = local_index as usize;
                let loc = *self.value_stack.last().unwrap();

                if self.local_types[local_index].is_float() {
                    let fp = self.fp_stack.peek1()?;
                    if self.machine.arch_supports_canonicalize_nan()
                        && self.config.enable_nan_canonicalization
                        && fp.canonicalization.is_some()
                    {
                        self.machine.canonicalize_nan(
                            match self.local_types[local_index] {
                                WpType::F32 => Size::S32,
                                WpType::F64 => Size::S64,
                                _ => unreachable!(),
                            },
                            loc,
                            self.locals[local_index],
                        );
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            self.locals[local_index],
                        );
                    }
                } else {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        loc,
                        self.locals[local_index],
                    );
                }
            }
            Operator::I32Const { value } => {
                self.value_stack.push(Location::Imm32(value as u32));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value as u32 as u64));
            }
            Operator::I32Add => self.emit_binop_i32(Assembler::emit_add),
            Operator::I32Sub => self.emit_binop_i32(Assembler::emit_sub),
            Operator::I32Mul => self.emit_binop_i32(Assembler::emit_imul),
            Operator::I32DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_xor(
                    Size::S32,
                    Location::GPR(GPR::RDX),
                    Location::GPR(GPR::RDX),
                );
                self.emit_relaxed_xdiv(Assembler::emit_div, Size::S32, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
            }
            Operator::I32DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_cdq();
                self.emit_relaxed_xdiv(Assembler::emit_idiv, Size::S32, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
            }
            Operator::I32RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_xor(
                    Size::S32,
                    Location::GPR(GPR::RDX),
                    Location::GPR(GPR::RDX),
                );
                self.emit_relaxed_xdiv(Assembler::emit_div, Size::S32, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);
            }
            Operator::I32RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);

                let normal_path = self.machine.specific.assembler.get_label();
                let end = self.machine.specific.assembler.get_label();

                self.machine.specific.emit_relaxed_cmp(
                    Size::S32,
                    Location::Imm32(0x80000000),
                    loc_a,
                );
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::NotEqual, normal_path);
                self.machine.specific.emit_relaxed_cmp(
                    Size::S32,
                    Location::Imm32(0xffffffff),
                    loc_b,
                );
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::NotEqual, normal_path);
                self.machine
                    .specific
                    .move_location(Size::S32, Location::Imm32(0), ret);
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::None, end);

                self.machine.specific.emit_label(normal_path);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_cdq();
                self.emit_relaxed_xdiv(Assembler::emit_idiv, Size::S32, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);

                self.machine.specific.emit_label(end);
            }
            Operator::I32And => self.emit_binop_i32(Assembler::emit_and),
            Operator::I32Or => self.emit_binop_i32(Assembler::emit_or),
            Operator::I32Xor => self.emit_binop_i32(Assembler::emit_xor),
            Operator::I32Eq => self.emit_cmpop_i32(Condition::Equal)?,
            Operator::I32Ne => self.emit_cmpop_i32(Condition::NotEqual)?,
            Operator::I32Eqz => {
                self.emit_cmpop_i32_dynamic_b(Condition::Equal, Location::Imm32(0))?
            }
            Operator::I32Clz => {
                let loc = self.pop_value_released();
                let src = match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        self.machine
                            .specific
                            .move_location(Size::S32, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I32Clz src: unreachable code".to_string(),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I32Clz dst: unreachable code".to_string(),
                        })
                    }
                };

                if self.machine.specific.assembler.arch_has_xzcnt() {
                    self.machine.specific.assembler.arch_emit_lzcnt(
                        Size::S32,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                } else {
                    let zero_path = self.machine.specific.assembler.get_label();
                    let end = self.machine.specific.assembler.get_label();

                    self.machine.specific.assembler.emit_test_gpr_64(src);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Equal, zero_path);
                    self.machine.specific.assembler.emit_bsr(
                        Size::S32,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                    self.machine.specific.assembler.emit_xor(
                        Size::S32,
                        Location::Imm32(31),
                        Location::GPR(dst),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end);
                    self.machine.specific.emit_label(zero_path);
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Imm32(32),
                        Location::GPR(dst),
                    );
                    self.machine.specific.emit_label(end);
                }

                match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                if let Location::Memory(_, _) = ret {
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(dst), ret);
                    self.machine.release_temp_gpr(dst);
                };
            }
            Operator::I32Ctz => {
                let loc = self.pop_value_released();
                let src = match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        self.machine
                            .specific
                            .move_location(Size::S32, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I32Ctz src: unreachable code".to_string(),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I32Ctz dst: unreachable code".to_string(),
                        })
                    }
                };

                if self.machine.specific.assembler.arch_has_xzcnt() {
                    self.machine.specific.assembler.arch_emit_tzcnt(
                        Size::S32,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                } else {
                    let zero_path = self.machine.specific.assembler.get_label();
                    let end = self.machine.specific.assembler.get_label();

                    self.machine.specific.assembler.emit_test_gpr_64(src);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Equal, zero_path);
                    self.machine.specific.assembler.emit_bsf(
                        Size::S32,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end);
                    self.machine.specific.emit_label(zero_path);
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Imm32(32),
                        Location::GPR(dst),
                    );
                    self.machine.specific.emit_label(end);
                }

                match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                if let Location::Memory(_, _) = ret {
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(dst), ret);
                    self.machine.release_temp_gpr(dst);
                };
            }
            Operator::I32Popcnt => self.emit_xcnt_i32(Assembler::emit_popcnt)?,
            Operator::I32Shl => self.emit_shift_i32(Assembler::emit_shl),
            Operator::I32ShrU => self.emit_shift_i32(Assembler::emit_shr),
            Operator::I32ShrS => self.emit_shift_i32(Assembler::emit_sar),
            Operator::I32Rotl => self.emit_shift_i32(Assembler::emit_rol),
            Operator::I32Rotr => self.emit_shift_i32(Assembler::emit_ror),
            Operator::I32LtU => self.emit_cmpop_i32(Condition::Below)?,
            Operator::I32LeU => self.emit_cmpop_i32(Condition::BelowEqual)?,
            Operator::I32GtU => self.emit_cmpop_i32(Condition::Above)?,
            Operator::I32GeU => self.emit_cmpop_i32(Condition::AboveEqual)?,
            Operator::I32LtS => {
                self.emit_cmpop_i32(Condition::Less)?;
            }
            Operator::I32LeS => self.emit_cmpop_i32(Condition::LessEqual)?,
            Operator::I32GtS => self.emit_cmpop_i32(Condition::Greater)?,
            Operator::I32GeS => self.emit_cmpop_i32(Condition::GreaterEqual)?,
            Operator::I64Const { value } => {
                let value = value as u64;
                self.value_stack.push(Location::Imm64(value));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value));
            }
            Operator::I64Add => self.emit_binop_i64(Assembler::emit_add),
            Operator::I64Sub => self.emit_binop_i64(Assembler::emit_sub),
            Operator::I64Mul => self.emit_binop_i64(Assembler::emit_imul),
            Operator::I64DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_xor(
                    Size::S64,
                    Location::GPR(GPR::RDX),
                    Location::GPR(GPR::RDX),
                );
                self.emit_relaxed_xdiv(Assembler::emit_div, Size::S64, loc_b);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::I64DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_cqo();
                self.emit_relaxed_xdiv(Assembler::emit_idiv, Size::S64, loc_b);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::I64RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_xor(
                    Size::S64,
                    Location::GPR(GPR::RDX),
                    Location::GPR(GPR::RDX),
                );
                self.emit_relaxed_xdiv(Assembler::emit_div, Size::S64, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(GPR::RDX), ret);
            }
            Operator::I64RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);

                let normal_path = self.machine.specific.assembler.get_label();
                let end = self.machine.specific.assembler.get_label();

                self.machine.specific.emit_relaxed_cmp(
                    Size::S64,
                    Location::Imm64(0x8000000000000000u64),
                    loc_a,
                );
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::NotEqual, normal_path);
                self.machine.specific.emit_relaxed_cmp(
                    Size::S64,
                    Location::Imm64(0xffffffffffffffffu64),
                    loc_b,
                );
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::NotEqual, normal_path);
                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, Location::Imm64(0), ret);
                self.machine
                    .specific
                    .assembler
                    .emit_jmp(Condition::None, end);

                self.machine.specific.emit_label(normal_path);

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                self.machine.specific.assembler.emit_cqo();
                self.emit_relaxed_xdiv(Assembler::emit_idiv, Size::S64, loc_b);
                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(GPR::RDX), ret);
                self.machine.specific.emit_label(end);
            }
            Operator::I64And => self.emit_binop_i64(Assembler::emit_and),
            Operator::I64Or => self.emit_binop_i64(Assembler::emit_or),
            Operator::I64Xor => self.emit_binop_i64(Assembler::emit_xor),
            Operator::I64Eq => self.emit_cmpop_i64(Condition::Equal)?,
            Operator::I64Ne => self.emit_cmpop_i64(Condition::NotEqual)?,
            Operator::I64Eqz => {
                self.emit_cmpop_i64_dynamic_b(Condition::Equal, Location::Imm64(0))?
            }
            Operator::I64Clz => {
                let loc = self.pop_value_released();
                let src = match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        self.machine
                            .specific
                            .move_location(Size::S64, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I64Clz src: unreachable code".to_string(),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I64Clz dst: unreachable code".to_string(),
                        })
                    }
                };

                if self.machine.specific.assembler.arch_has_xzcnt() {
                    self.machine.specific.assembler.arch_emit_lzcnt(
                        Size::S64,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                } else {
                    let zero_path = self.machine.specific.assembler.get_label();
                    let end = self.machine.specific.assembler.get_label();

                    self.machine.specific.assembler.emit_test_gpr_64(src);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Equal, zero_path);
                    self.machine.specific.assembler.emit_bsr(
                        Size::S64,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                    self.machine.specific.assembler.emit_xor(
                        Size::S64,
                        Location::Imm32(63),
                        Location::GPR(dst),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end);
                    self.machine.specific.emit_label(zero_path);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm32(64),
                        Location::GPR(dst),
                    );
                    self.machine.specific.emit_label(end);
                }

                match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                if let Location::Memory(_, _) = ret {
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(dst), ret);
                    self.machine.release_temp_gpr(dst);
                };
            }
            Operator::I64Ctz => {
                let loc = self.pop_value_released();
                let src = match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        self.machine
                            .specific
                            .move_location(Size::S64, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I64Ctz src: unreachable code".to_string(),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: "I64Ctz dst: unreachable code".to_string(),
                        })
                    }
                };

                if self.machine.specific.assembler.arch_has_xzcnt() {
                    self.machine.specific.assembler.arch_emit_tzcnt(
                        Size::S64,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                } else {
                    let zero_path = self.machine.specific.assembler.get_label();
                    let end = self.machine.specific.assembler.get_label();

                    self.machine.specific.assembler.emit_test_gpr_64(src);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Equal, zero_path);
                    self.machine.specific.assembler.emit_bsf(
                        Size::S64,
                        Location::GPR(src),
                        Location::GPR(dst),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end);
                    self.machine.specific.emit_label(zero_path);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm32(64),
                        Location::GPR(dst),
                    );
                    self.machine.specific.emit_label(end);
                }

                match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                if let Location::Memory(_, _) = ret {
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(dst), ret);
                    self.machine.release_temp_gpr(dst);
                };
            }
            Operator::I64Popcnt => self.emit_xcnt_i64(Assembler::emit_popcnt)?,
            Operator::I64Shl => self.emit_shift_i64(Assembler::emit_shl),
            Operator::I64ShrU => self.emit_shift_i64(Assembler::emit_shr),
            Operator::I64ShrS => self.emit_shift_i64(Assembler::emit_sar),
            Operator::I64Rotl => self.emit_shift_i64(Assembler::emit_rol),
            Operator::I64Rotr => self.emit_shift_i64(Assembler::emit_ror),
            Operator::I64LtU => self.emit_cmpop_i64(Condition::Below)?,
            Operator::I64LeU => self.emit_cmpop_i64(Condition::BelowEqual)?,
            Operator::I64GtU => self.emit_cmpop_i64(Condition::Above)?,
            Operator::I64GeU => self.emit_cmpop_i64(Condition::AboveEqual)?,
            Operator::I64LtS => {
                self.emit_cmpop_i64(Condition::Less)?;
            }
            Operator::I64LeS => self.emit_cmpop_i64(Condition::LessEqual)?,
            Operator::I64GtS => self.emit_cmpop_i64(Condition::Greater)?,
            Operator::I64GeS => self.emit_cmpop_i64(Condition::GreaterEqual)?,
            Operator::I64ExtendI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);

                // A 32-bit memory write does not automatically clear the upper 32 bits of a 64-bit word.
                // So, we need to explicitly write zero to the upper half here.
                if let Location::Memory(base, off) = ret {
                    self.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Imm32(0),
                        Location::Memory(base, off + 4),
                    );
                }
            }
            Operator::I64ExtendI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret);
            }
            Operator::I32Extend8S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S32, ret);
            }
            Operator::I32Extend16S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S32, ret);
            }
            Operator::I64Extend8S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S8, loc, Size::S64, ret);
            }
            Operator::I64Extend16S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S16, loc, Size::S64, ret);
            }
            Operator::I64Extend32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine
                    .specific
                    .emit_relaxed_sign_extension(Size::S32, loc, Size::S64, ret);
            }
            Operator::I32WrapI64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
            }

            Operator::F32Const { value } => {
                self.value_stack.push(Location::Imm32(value.bits()));
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits() as u64));
            }
            Operator::F32Add => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vaddss)?;
            }
            Operator::F32Sub => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vsubss)?
            }
            Operator::F32Mul => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vmulss)?
            }
            Operator::F32Div => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vdivss)?
            }
            Operator::F32Max => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                if !self.machine.arch_supports_canonicalize_nan() {
                    self.emit_fp_binop_avx(Assembler::emit_vmaxss)?;
                } else {
                    let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                    let tmp1 = self.machine.acquire_temp_simd().unwrap();
                    let tmp2 = self.machine.acquire_temp_simd().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match loc_a {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Max src1: unreachable code".to_string(),
                            })
                        }
                    };
                    let src2 = match loc_b {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Max src2: unreachable code".to_string(),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    self.machine.specific.move_location(
                        Size::S32,
                        Location::SIMD(src1),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::SIMD(src2),
                        Location::GPR(tmpg2),
                    );
                    self.machine.specific.assembler.emit_cmp(
                        Size::S32,
                        Location::GPR(tmpg2),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.assembler.emit_vmaxss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                    );
                    let label1 = self.machine.specific.assembler.get_label();
                    let label2 = self.machine.specific.assembler.get_label();
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::NotEqual, label1);
                    self.machine
                        .specific
                        .assembler
                        .emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, label2);
                    self.machine.specific.emit_label(label1);
                    self.machine.specific.assembler.emit_vxorps(
                        tmp_xmm2,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm2,
                    );
                    self.machine.specific.emit_label(label2);
                    self.machine.specific.assembler.emit_vcmpeqss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm3,
                    );
                    self.machine.specific.assembler.emit_vblendvps(
                        tmp_xmm3,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm1,
                        tmp_xmm1,
                    );
                    self.machine.specific.assembler.emit_vcmpunordss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        src1,
                    );
                    // load float canonical nan
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm32(0x7FC0_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(src2),
                    );
                    self.machine.specific.assembler.emit_vblendvps(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                        src1,
                    );
                    match ret {
                        Location::SIMD(x) => {
                            self.machine
                                .specific
                                .assembler
                                .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::SIMD(src1),
                                ret,
                            );
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Max ret: unreachable code".to_string(),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_simd(tmp2);
                    self.machine.release_temp_simd(tmp1);
                }
            }
            Operator::F32Min => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));
                if !self.machine.arch_supports_canonicalize_nan() {
                    self.emit_fp_binop_avx(Assembler::emit_vminss)?;
                } else {
                    let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                    let tmp1 = self.machine.acquire_temp_simd().unwrap();
                    let tmp2 = self.machine.acquire_temp_simd().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match loc_a {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Min src1: unreachable code".to_string(),
                            })
                        }
                    };
                    let src2 = match loc_b {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Min src2: unreachable code".to_string(),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    self.machine.specific.move_location(
                        Size::S32,
                        Location::SIMD(src1),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::SIMD(src2),
                        Location::GPR(tmpg2),
                    );
                    self.machine.specific.assembler.emit_cmp(
                        Size::S32,
                        Location::GPR(tmpg2),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.assembler.emit_vminss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                    );
                    let label1 = self.machine.specific.assembler.get_label();
                    let label2 = self.machine.specific.assembler.get_label();
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::NotEqual, label1);
                    self.machine
                        .specific
                        .assembler
                        .emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, label2);
                    self.machine.specific.emit_label(label1);
                    // load float -0.0
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm32(0x8000_0000), // Negative zero
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(tmp_xmm2),
                    );
                    self.machine.specific.emit_label(label2);
                    self.machine.specific.assembler.emit_vcmpeqss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm3,
                    );
                    self.machine.specific.assembler.emit_vblendvps(
                        tmp_xmm3,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm1,
                        tmp_xmm1,
                    );
                    self.machine.specific.assembler.emit_vcmpunordss(
                        src1,
                        XMMOrMemory::XMM(src2),
                        src1,
                    );
                    // load float canonical nan
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm32(0x7FC0_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(src2),
                    );
                    self.machine.specific.assembler.emit_vblendvps(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                        src1,
                    );
                    match ret {
                        Location::SIMD(x) => {
                            self.machine
                                .specific
                                .assembler
                                .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::SIMD(src1),
                                ret,
                            );
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F32Min ret: unreachable code".to_string(),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_simd(tmp2);
                    self.machine.release_temp_simd(tmp1);
                }
            }
            Operator::F32Eq => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpeqss)?
            }
            Operator::F32Ne => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpneqss)?
            }
            Operator::F32Lt => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpltss)?
            }
            Operator::F32Le => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpless)?
            }
            Operator::F32Gt => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpgtss)?
            }
            Operator::F32Ge => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpgess)?
            }
            Operator::F32Nearest => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundss_nearest)?
            }
            Operator::F32Floor => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundss_floor)?
            }
            Operator::F32Ceil => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundss_ceil)?
            }
            Operator::F32Trunc => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundss_trunc)?
            }
            Operator::F32Sqrt => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f32(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vsqrtss)?
            }

            Operator::F32Copysign => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F32);

                let (fp_src1, fp_src2) = self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (fp, loc, tmp) in [(fp_src1, loc_a, tmp1), (fp_src2, loc_b, tmp2)].iter() {
                        match fp.canonicalization {
                            Some(_) => {
                                self.machine
                                    .canonicalize_nan(Size::S32, *loc, Location::GPR(*tmp));
                            }
                            None => {
                                self.machine.specific.move_location(
                                    Size::S32,
                                    *loc,
                                    Location::GPR(*tmp),
                                );
                            }
                        }
                    }
                } else {
                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc_a, Location::GPR(tmp1));
                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc_b, Location::GPR(tmp2));
                }
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0x7fffffffu32),
                    Location::GPR(tmp1),
                );
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0x80000000u32),
                    Location::GPR(tmp2),
                );
                self.machine.specific.assembler.emit_or(
                    Size::S32,
                    Location::GPR(tmp2),
                    Location::GPR(tmp1),
                );
                self.machine
                    .specific
                    .move_location(Size::S32, Location::GPR(tmp1), ret);
                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F32Abs => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.machine
                    .specific
                    .move_location(Size::S32, loc, Location::GPR(tmp));
                self.machine.specific.assembler.emit_and(
                    Size::S32,
                    Location::Imm32(0x7fffffffu32),
                    Location::GPR(tmp),
                );
                self.machine
                    .specific
                    .move_location(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }

            Operator::F32Neg => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.assembler.arch_has_fneg() {
                    let tmp = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp));
                    self.machine.specific.assembler.arch_emit_f32_neg(tmp, tmp);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::SIMD(tmp), ret);
                    self.machine.release_temp_simd(tmp);
                } else {
                    let tmp = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .move_location(Size::S32, loc, Location::GPR(tmp));
                    self.machine
                        .specific
                        .assembler
                        .emit_btc_gpr_imm8_32(31, tmp);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(tmp), ret);
                    self.machine.release_temp_gpr(tmp);
                }
            }

            Operator::F64Const { value } => {
                self.value_stack.push(Location::Imm64(value.bits()));
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits()));
            }
            Operator::F64Add => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vaddsd)?
            }
            Operator::F64Sub => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vsubsd)?
            }
            Operator::F64Mul => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vmulsd)?
            }
            Operator::F64Div => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 2));
                self.emit_fp_binop_avx(Assembler::emit_vdivsd)?
            }
            Operator::F64Max => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));

                if !self.machine.arch_supports_canonicalize_nan() {
                    self.emit_fp_binop_avx(Assembler::emit_vmaxsd)?;
                } else {
                    let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                    let tmp1 = self.machine.acquire_temp_simd().unwrap();
                    let tmp2 = self.machine.acquire_temp_simd().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match loc_a {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Max src1: unreachable code".to_string(),
                            })
                        }
                    };
                    let src2 = match loc_b {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Max src2: unreachable code".to_string(),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    self.machine.specific.move_location(
                        Size::S64,
                        Location::SIMD(src1),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::SIMD(src2),
                        Location::GPR(tmpg2),
                    );
                    self.machine.specific.assembler.emit_cmp(
                        Size::S64,
                        Location::GPR(tmpg2),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.assembler.emit_vmaxsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                    );
                    let label1 = self.machine.specific.assembler.get_label();
                    let label2 = self.machine.specific.assembler.get_label();
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::NotEqual, label1);
                    self.machine
                        .specific
                        .assembler
                        .emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, label2);
                    self.machine.specific.emit_label(label1);
                    self.machine.specific.assembler.emit_vxorpd(
                        tmp_xmm2,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm2,
                    );
                    self.machine.specific.emit_label(label2);
                    self.machine.specific.assembler.emit_vcmpeqsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm3,
                    );
                    self.machine.specific.assembler.emit_vblendvpd(
                        tmp_xmm3,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm1,
                        tmp_xmm1,
                    );
                    self.machine.specific.assembler.emit_vcmpunordsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        src1,
                    );
                    // load float canonical nan
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(src2),
                    );
                    self.machine.specific.assembler.emit_vblendvpd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                        src1,
                    );
                    match ret {
                        Location::SIMD(x) => {
                            self.machine
                                .specific
                                .assembler
                                .emit_vmovapd(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::SIMD(src1),
                                ret,
                            );
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Max ret: unreachable code".to_string(),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_simd(tmp2);
                    self.machine.release_temp_simd(tmp1);
                }
            }
            Operator::F64Min => {
                self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 2));

                if !self.machine.arch_supports_canonicalize_nan() {
                    self.emit_fp_binop_avx(Assembler::emit_vminsd)?;
                } else {
                    let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                    let tmp1 = self.machine.acquire_temp_simd().unwrap();
                    let tmp2 = self.machine.acquire_temp_simd().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match loc_a {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_a,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp1),
                            );
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Min src1: unreachable code".to_string(),
                            })
                        }
                    };
                    let src2 = match loc_b {
                        Location::SIMD(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm32(_) => {
                            self.machine.specific.move_location(
                                Size::S32,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S32,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc_b,
                                Location::GPR(tmpg1),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmpg1),
                                Location::SIMD(tmp2),
                            );
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Min src2: unreachable code".to_string(),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    self.machine.specific.move_location(
                        Size::S64,
                        Location::SIMD(src1),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::SIMD(src2),
                        Location::GPR(tmpg2),
                    );
                    self.machine.specific.assembler.emit_cmp(
                        Size::S64,
                        Location::GPR(tmpg2),
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.assembler.emit_vminsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                    );
                    let label1 = self.machine.specific.assembler.get_label();
                    let label2 = self.machine.specific.assembler.get_label();
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::NotEqual, label1);
                    self.machine
                        .specific
                        .assembler
                        .emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, label2);
                    self.machine.specific.emit_label(label1);
                    // load float -0.0
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(0x8000_0000_0000_0000), // Negative zero
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(tmp_xmm2),
                    );
                    self.machine.specific.emit_label(label2);
                    self.machine.specific.assembler.emit_vcmpeqsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm3,
                    );
                    self.machine.specific.assembler.emit_vblendvpd(
                        tmp_xmm3,
                        XMMOrMemory::XMM(tmp_xmm2),
                        tmp_xmm1,
                        tmp_xmm1,
                    );
                    self.machine.specific.assembler.emit_vcmpunordsd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        src1,
                    );
                    // load float canonical nan
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmpg1),
                        Location::SIMD(src2),
                    );
                    self.machine.specific.assembler.emit_vblendvpd(
                        src1,
                        XMMOrMemory::XMM(src2),
                        tmp_xmm1,
                        src1,
                    );
                    match ret {
                        Location::SIMD(x) => {
                            self.machine
                                .specific
                                .assembler
                                .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::SIMD(src1),
                                ret,
                            );
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "F64Min ret: unreachable code".to_string(),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_simd(tmp2);
                    self.machine.release_temp_simd(tmp1);
                }
            }
            Operator::F64Eq => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpeqsd)?
            }
            Operator::F64Ne => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpneqsd)?
            }
            Operator::F64Lt => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpltsd)?
            }
            Operator::F64Le => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmplesd)?
            }
            Operator::F64Gt => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpgtsd)?
            }
            Operator::F64Ge => {
                self.fp_stack.pop2()?;
                self.emit_fp_cmpop_avx(Assembler::emit_vcmpgesd)?
            }
            Operator::F64Nearest => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundsd_nearest)?
            }
            Operator::F64Floor => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundsd_floor)?
            }
            Operator::F64Ceil => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundsd_ceil)?
            }
            Operator::F64Trunc => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vroundsd_trunc)?
            }
            Operator::F64Sqrt => {
                self.fp_stack.pop1()?;
                self.fp_stack
                    .push(FloatValue::cncl_f64(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vsqrtsd)?
            }

            Operator::F64Copysign => {
                let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

                let (fp_src1, fp_src2) = self.fp_stack.pop2()?;
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();

                if self.machine.arch_supports_canonicalize_nan()
                    && self.config.enable_nan_canonicalization
                {
                    for (fp, loc, tmp) in [(fp_src1, loc_a, tmp1), (fp_src2, loc_b, tmp2)].iter() {
                        match fp.canonicalization {
                            Some(_) => {
                                self.machine
                                    .canonicalize_nan(Size::S64, *loc, Location::GPR(*tmp));
                            }
                            None => {
                                self.machine.specific.move_location(
                                    Size::S64,
                                    *loc,
                                    Location::GPR(*tmp),
                                );
                            }
                        }
                    }
                } else {
                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc_a, Location::GPR(tmp1));
                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc_b, Location::GPR(tmp2));
                }

                let c = self.machine.acquire_temp_gpr().unwrap();

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Imm64(0x7fffffffffffffffu64),
                    Location::GPR(c),
                );
                self.machine.specific.assembler.emit_and(
                    Size::S64,
                    Location::GPR(c),
                    Location::GPR(tmp1),
                );

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Imm64(0x8000000000000000u64),
                    Location::GPR(c),
                );
                self.machine.specific.assembler.emit_and(
                    Size::S64,
                    Location::GPR(c),
                    Location::GPR(tmp2),
                );

                self.machine.specific.assembler.emit_or(
                    Size::S64,
                    Location::GPR(tmp2),
                    Location::GPR(tmp1),
                );
                self.machine
                    .specific
                    .move_location(Size::S64, Location::GPR(tmp1), ret);

                self.machine.release_temp_gpr(c);
                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F64Abs => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp = self.machine.acquire_temp_gpr().unwrap();
                let c = self.machine.acquire_temp_gpr().unwrap();

                self.machine
                    .specific
                    .move_location(Size::S64, loc, Location::GPR(tmp));
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Imm64(0x7fffffffffffffffu64),
                    Location::GPR(c),
                );
                self.machine.specific.assembler.emit_and(
                    Size::S64,
                    Location::GPR(c),
                    Location::GPR(tmp),
                );
                self.machine
                    .specific
                    .move_location(Size::S64, Location::GPR(tmp), ret);

                self.machine.release_temp_gpr(c);
                self.machine.release_temp_gpr(tmp);
            }

            Operator::F64Neg => {
                // Preserve canonicalization state.

                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                if self.machine.specific.assembler.arch_has_fneg() {
                    let tmp = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp));
                    self.machine.specific.assembler.arch_emit_f64_neg(tmp, tmp);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::SIMD(tmp), ret);
                    self.machine.release_temp_simd(tmp);
                } else {
                    let tmp = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .move_location(Size::S64, loc, Location::GPR(tmp));
                    self.machine
                        .specific
                        .assembler
                        .emit_btc_gpr_imm8_64(63, tmp);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(tmp), ret);
                    self.machine.release_temp_gpr(tmp);
                }
            }

            Operator::F64PromoteF32 => {
                let fp = self.fp_stack.pop1()?;
                self.fp_stack.push(fp.promote(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vcvtss2sd)?
            }
            Operator::F32DemoteF64 => {
                let fp = self.fp_stack.pop1()?;
                self.fp_stack.push(fp.demote(self.value_stack.len() - 1));
                self.emit_fp_unop_avx(Assembler::emit_vcvtsd2ss)?
            }

            Operator::I32ReinterpretF32 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let fp = self.fp_stack.pop1()?;

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || fp.canonicalization.is_none()
                {
                    if loc != ret {
                        self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S32, loc, ret);
                }
            }
            Operator::F32ReinterpretI32 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                if loc != ret {
                    self.machine.specific.emit_relaxed_mov(Size::S32, loc, ret);
                }
            }

            Operator::I64ReinterpretF64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let fp = self.fp_stack.pop1()?;

                if !self.machine.arch_supports_canonicalize_nan()
                    || !self.config.enable_nan_canonicalization
                    || fp.canonicalization.is_none()
                {
                    if loc != ret {
                        self.machine.specific.emit_relaxed_mov(Size::S64, loc, ret);
                    }
                } else {
                    self.machine.canonicalize_nan(Size::S64, loc, ret);
                }
            }
            Operator::F64ReinterpretI64 => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                if loc != ret {
                    self.machine.specific.emit_relaxed_mov(Size::S64, loc, ret);
                }
            }

            Operator::I32TruncF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i32_trunc_uf32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_U32_MIN, LEF32_GT_U32_MAX);

                    self.machine
                        .specific
                        .assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();
                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                self.emit_f32_int_conv_check_sat(
                    tmp_in,
                    GEF32_LT_U32_MIN,
                    LEF32_GT_U32_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(0),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::u32::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(this: &mut Self)>,
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i32_trunc_uf32(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i32_trunc_sf32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_I32_MIN, LEF32_GT_I32_MAX);

                    self.machine
                        .specific
                        .assembler
                        .emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }
            Operator::I32TruncSatF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                self.emit_f32_int_conv_check_sat(
                    tmp_in,
                    GEF32_LT_I32_MIN,
                    LEF32_GT_I32_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MIN as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MAX as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|this: &mut Self| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(0),
                            Location::GPR(tmp_out),
                        );
                    }),
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i32_trunc_sf32(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i64_trunc_sf32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_I64_MIN, LEF32_GT_I64_MAX);
                    self.machine
                        .specific
                        .assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                self.emit_f32_int_conv_check_sat(
                    tmp_in,
                    GEF32_LT_I64_MIN,
                    LEF32_GT_I64_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MIN as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MAX as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|this: &mut Self| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(0),
                            Location::GPR(tmp_out),
                        );
                    }),
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i64_trunc_sf32(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i64_trunc_uf32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap(); // xmm2

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                    self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_U64_MIN, LEF32_GT_U64_MAX);

                    let tmp = self.machine.acquire_temp_gpr().unwrap(); // r15
                    let tmp_x1 = self.machine.acquire_temp_simd().unwrap(); // xmm1
                    let tmp_x2 = self.machine.acquire_temp_simd().unwrap(); // xmm3

                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Imm32(1593835520u32),
                        Location::GPR(tmp),
                    ); //float 9.22337203E+18
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::GPR(tmp),
                        Location::SIMD(tmp_x1),
                    );
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::SIMD(tmp_in),
                        Location::SIMD(tmp_x2),
                    );
                    self.machine.specific.assembler.emit_vsubss(
                        tmp_in,
                        XMMOrMemory::XMM(tmp_x1),
                        tmp_in,
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_xor(
                        Size::S64,
                        Location::GPR(tmp_out),
                        Location::GPR(tmp),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                    self.machine
                        .specific
                        .assembler
                        .emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                    self.machine
                        .specific
                        .assembler
                        .emit_cmovae_gpr_64(tmp, tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_x2);
                    self.machine.release_temp_simd(tmp_x1);
                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }
            Operator::I64TruncSatF32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in));
                self.emit_f32_int_conv_check_sat(
                    tmp_in,
                    GEF32_LT_U64_MIN,
                    LEF32_GT_U64_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(0),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::u64::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(this: &mut Self)>,
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i64_trunc_uf32(tmp_in, tmp_out);
                        } else {
                            let tmp = this.machine.acquire_temp_gpr().unwrap();
                            let tmp_x1 = this.machine.acquire_temp_simd().unwrap();
                            let tmp_x2 = this.machine.acquire_temp_simd().unwrap();

                            this.machine.specific.assembler.emit_mov(
                                Size::S32,
                                Location::Imm32(1593835520u32),
                                Location::GPR(tmp),
                            ); //float 9.22337203E+18
                            this.machine.specific.assembler.emit_mov(
                                Size::S32,
                                Location::GPR(tmp),
                                Location::SIMD(tmp_x1),
                            );
                            this.machine.specific.assembler.emit_mov(
                                Size::S32,
                                Location::SIMD(tmp_in),
                                Location::SIMD(tmp_x2),
                            );
                            this.machine.specific.assembler.emit_vsubss(
                                tmp_in,
                                XMMOrMemory::XMM(tmp_x1),
                                tmp_in,
                            );
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                            this.machine.specific.assembler.emit_mov(
                                Size::S64,
                                Location::Imm64(0x8000000000000000u64),
                                Location::GPR(tmp),
                            );
                            this.machine.specific.assembler.emit_xor(
                                Size::S64,
                                Location::GPR(tmp_out),
                                Location::GPR(tmp),
                            );
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                            this.machine
                                .specific
                                .assembler
                                .emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                            this.machine
                                .specific
                                .assembler
                                .emit_cmovae_gpr_64(tmp, tmp_out);

                            this.machine.release_temp_simd(tmp_x2);
                            this.machine.release_temp_simd(tmp_x1);
                            this.machine.release_temp_gpr(tmp);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i32_trunc_uf64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_U32_MIN, LEF64_GT_U32_MAX);

                    self.machine
                        .specific
                        .assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                self.emit_f64_int_conv_check_sat(
                    tmp_in,
                    GEF64_LT_U32_MIN,
                    LEF64_GT_U32_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(0),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::u32::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(this: &mut Self)>,
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i32_trunc_uf64(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i32_trunc_sf64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();

                    let real_in = match loc {
                        Location::Imm32(_) | Location::Imm64(_) => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc,
                                Location::GPR(tmp_out),
                            );
                            self.machine.specific.move_location(
                                Size::S64,
                                Location::GPR(tmp_out),
                                Location::SIMD(tmp_in),
                            );
                            tmp_in
                        }
                        Location::SIMD(x) => x,
                        _ => {
                            self.machine.specific.move_location(
                                Size::S64,
                                loc,
                                Location::SIMD(tmp_in),
                            );
                            tmp_in
                        }
                    };

                    self.emit_f64_int_conv_check_trap(real_in, GEF64_LT_I32_MIN, LEF64_GT_I32_MAX);

                    self.machine
                        .specific
                        .assembler
                        .emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                let real_in = match loc {
                    Location::Imm32(_) | Location::Imm64(_) => {
                        self.machine
                            .specific
                            .move_location(Size::S64, loc, Location::GPR(tmp_out));
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(tmp_out),
                            Location::SIMD(tmp_in),
                        );
                        tmp_in
                    }
                    Location::SIMD(x) => x,
                    _ => {
                        self.machine
                            .specific
                            .move_location(Size::S64, loc, Location::SIMD(tmp_in));
                        tmp_in
                    }
                };

                self.emit_f64_int_conv_check_sat(
                    real_in,
                    GEF64_LT_I32_MIN,
                    LEF64_GT_I32_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MIN as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MAX as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|this: &mut Self| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(0),
                            Location::GPR(tmp_out),
                        );
                    }),
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i32_trunc_sf64(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i64_trunc_sf64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_I64_MIN, LEF64_GT_I64_MAX);

                    self.machine
                        .specific
                        .assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                self.emit_f64_int_conv_check_sat(
                    tmp_in,
                    GEF64_LT_I64_MIN,
                    LEF64_GT_I64_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MIN as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MAX as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|this: &mut Self| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(0),
                            Location::GPR(tmp_out),
                        );
                    }),
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i64_trunc_sf64(tmp_in, tmp_out);
                        } else {
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                if self.machine.specific.assembler.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_i64_trunc_uf64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_simd().unwrap(); // xmm2

                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                    self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_U64_MIN, LEF64_GT_U64_MAX);

                    let tmp = self.machine.acquire_temp_gpr().unwrap(); // r15
                    let tmp_x1 = self.machine.acquire_temp_simd().unwrap(); // xmm1
                    let tmp_x2 = self.machine.acquire_temp_simd().unwrap(); // xmm3

                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(4890909195324358656u64),
                        Location::GPR(tmp),
                    ); //double 9.2233720368547758E+18
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmp),
                        Location::SIMD(tmp_x1),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::SIMD(tmp_in),
                        Location::SIMD(tmp_x2),
                    );
                    self.machine.specific.assembler.emit_vsubsd(
                        tmp_in,
                        XMMOrMemory::XMM(tmp_x1),
                        tmp_in,
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_xor(
                        Size::S64,
                        Location::GPR(tmp_out),
                        Location::GPR(tmp),
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                    self.machine
                        .specific
                        .assembler
                        .emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                    self.machine
                        .specific
                        .assembler
                        .emit_cmovae_gpr_64(tmp, tmp_out);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_simd(tmp_x2);
                    self.machine.release_temp_simd(tmp_x1);
                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_simd(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack.pop1()?;

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_simd().unwrap();

                self.machine
                    .specific
                    .emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in));
                self.emit_f64_int_conv_check_sat(
                    tmp_in,
                    GEF64_LT_U64_MIN,
                    LEF64_GT_U64_MAX,
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(0),
                            Location::GPR(tmp_out),
                        );
                    },
                    |this| {
                        this.machine.specific.assembler.emit_mov(
                            Size::S64,
                            Location::Imm64(std::u64::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(this: &mut Self)>,
                    |this| {
                        if this.machine.specific.assembler.arch_has_itruncf() {
                            this.machine
                                .specific
                                .assembler
                                .arch_emit_i64_trunc_uf64(tmp_in, tmp_out);
                        } else {
                            let tmp = this.machine.acquire_temp_gpr().unwrap();
                            let tmp_x1 = this.machine.acquire_temp_simd().unwrap();
                            let tmp_x2 = this.machine.acquire_temp_simd().unwrap();

                            this.machine.specific.assembler.emit_mov(
                                Size::S64,
                                Location::Imm64(4890909195324358656u64),
                                Location::GPR(tmp),
                            ); //double 9.2233720368547758E+18
                            this.machine.specific.assembler.emit_mov(
                                Size::S64,
                                Location::GPR(tmp),
                                Location::SIMD(tmp_x1),
                            );
                            this.machine.specific.assembler.emit_mov(
                                Size::S64,
                                Location::SIMD(tmp_in),
                                Location::SIMD(tmp_x2),
                            );
                            this.machine.specific.assembler.emit_vsubsd(
                                tmp_in,
                                XMMOrMemory::XMM(tmp_x1),
                                tmp_in,
                            );
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                            this.machine.specific.assembler.emit_mov(
                                Size::S64,
                                Location::Imm64(0x8000000000000000u64),
                                Location::GPR(tmp),
                            );
                            this.machine.specific.assembler.emit_xor(
                                Size::S64,
                                Location::GPR(tmp_out),
                                Location::GPR(tmp),
                            );
                            this.machine
                                .specific
                                .assembler
                                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                            this.machine
                                .specific
                                .assembler
                                .emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                            this.machine
                                .specific
                                .assembler
                                .emit_cmovae_gpr_64(tmp, tmp_out);

                            this.machine.release_temp_simd(tmp_x2);
                            this.machine.release_temp_simd(tmp_x1);
                            this.machine.release_temp_gpr(tmp);
                        }
                    },
                );

                self.machine
                    .specific
                    .assembler
                    .emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_simd(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::F32ConvertI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f32 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f32_convert_si32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2ss_32(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F32ConvertI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f32 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f32_convert_ui32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2ss_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F32ConvertI64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f32 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f32_convert_si64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2ss_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F32ConvertI64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f32 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f32_convert_ui64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    let tmp = self.machine.acquire_temp_gpr().unwrap();

                    let do_convert = self.machine.specific.assembler.get_label();
                    let end_convert = self.machine.specific.assembler.get_label();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_test_gpr_64(tmp_in);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Signed, do_convert);
                    self.machine.specific.assembler.emit_vcvtsi2ss_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end_convert);
                    self.machine.specific.emit_label(do_convert);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmp_in),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_and(
                        Size::S64,
                        Location::Imm32(1),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_shr(
                        Size::S64,
                        Location::Imm8(1),
                        Location::GPR(tmp_in),
                    );
                    self.machine.specific.assembler.emit_or(
                        Size::S64,
                        Location::GPR(tmp),
                        Location::GPR(tmp_in),
                    );
                    self.machine.specific.assembler.emit_vcvtsi2ss_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine.specific.assembler.emit_vaddss(
                        tmp_out,
                        XMMOrMemory::XMM(tmp_out),
                        tmp_out,
                    );
                    self.machine.specific.emit_label(end_convert);
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }

            Operator::F64ConvertI32S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f64 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f64_convert_si32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2sd_32(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F64ConvertI32U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i32 to f64 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f64_convert_ui32(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2sd_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F64ConvertI64S => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f64 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f64_convert_si64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_vcvtsi2sd_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }
            Operator::F64ConvertI64U => {
                let loc = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1)); // Converting i64 to f64 never results in NaN.

                if self.machine.specific.assembler.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine
                        .specific
                        .assembler
                        .arch_emit_f64_convert_ui64(tmp_in, tmp_out);
                    self.machine
                        .specific
                        .emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_simd().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    let tmp = self.machine.acquire_temp_gpr().unwrap();

                    let do_convert = self.machine.specific.assembler.get_label();
                    let end_convert = self.machine.specific.assembler.get_label();

                    self.machine
                        .specific
                        .assembler
                        .emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    self.machine.specific.assembler.emit_test_gpr_64(tmp_in);
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::Signed, do_convert);
                    self.machine.specific.assembler.emit_vcvtsi2sd_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine
                        .specific
                        .assembler
                        .emit_jmp(Condition::None, end_convert);
                    self.machine.specific.emit_label(do_convert);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(tmp_in),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_and(
                        Size::S64,
                        Location::Imm32(1),
                        Location::GPR(tmp),
                    );
                    self.machine.specific.assembler.emit_shr(
                        Size::S64,
                        Location::Imm8(1),
                        Location::GPR(tmp_in),
                    );
                    self.machine.specific.assembler.emit_or(
                        Size::S64,
                        Location::GPR(tmp),
                        Location::GPR(tmp_in),
                    );
                    self.machine.specific.assembler.emit_vcvtsi2sd_64(
                        tmp_out,
                        GPROrMemory::GPR(tmp_in),
                        tmp_out,
                    );
                    self.machine.specific.assembler.emit_vaddsd(
                        tmp_out,
                        XMMOrMemory::XMM(tmp_out),
                        tmp_out,
                    );
                    self.machine.specific.emit_label(end_convert);
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::SIMD(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_simd(tmp_out);
                }
            }

            Operator::Call { function_index } => {
                let function_index = function_index as usize;

                let sig_index = *self
                    .module
                    .functions
                    .get(FunctionIndex::new(function_index))
                    .unwrap();
                let sig = self.module.signatures.get(sig_index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().cloned().map(type_to_wp_type).collect();

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                self.machine.release_locations_only_osr_state(params.len());

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                while let Some(fp) = self.fp_stack.last() {
                    if fp.depth >= self.value_stack.len() {
                        let index = fp.depth - self.value_stack.len();
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            let size = fp.canonicalization.unwrap().to_size();
                            self.machine
                                .canonicalize_nan(size, params[index], params[index]);
                        }
                        self.fp_stack.pop().unwrap();
                    } else {
                        break;
                    }
                }

                // Imported functions are called through trampolines placed as custom sections.
                let reloc_target = if function_index < self.module.num_imported_functions {
                    RelocationTarget::CustomSection(SectionIndex::new(function_index))
                } else {
                    RelocationTarget::LocalFunc(LocalFunctionIndex::new(
                        function_index - self.module.num_imported_functions,
                    ))
                };
                self.machine
                    .specific
                    .move_with_reloc(reloc_target, &mut self.relocations);

                self.emit_call_native(
                    |this| {
                        let offset = this.machine.assembler_get_offset().0;
                        this.trap_table
                            .offset_to_code
                            .insert(offset, TrapCode::StackOverflow);
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                        this.mark_instruction_address_end(offset);
                    },
                    params.iter().copied(),
                )?;

                self.machine.release_locations_only_stack(&params);

                if !return_types.is_empty() {
                    let ret = self.machine.acquire_locations(
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    if return_types[0].is_float() {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::SIMD(self.machine.specific.get_simd_for_ret()),
                            ret,
                        );
                        self.fp_stack
                            .push(FloatValue::new(self.value_stack.len() - 1));
                    } else {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            ret,
                        );
                    }
                }
            }
            Operator::CallIndirect { index, table_index } => {
                // TODO: removed restriction on always being table idx 0;
                // does any code depend on this?
                let table_index = TableIndex::new(table_index as _);
                let index = SignatureIndex::new(index as usize);
                let sig = self.module.signatures.get(index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.results().iter().cloned().map(type_to_wp_type).collect();

                let func_index = self.pop_value_released();

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                // Pop arguments off the FP stack and canonicalize them if needed.
                //
                // Canonicalization state will be lost across function calls, so early canonicalization
                // is necessary here.
                while let Some(fp) = self.fp_stack.last() {
                    if fp.depth >= self.value_stack.len() {
                        let index = fp.depth - self.value_stack.len();
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            let size = fp.canonicalization.unwrap().to_size();
                            self.machine
                                .canonicalize_nan(size, params[index], params[index]);
                        }
                        self.fp_stack.pop().unwrap();
                    } else {
                        break;
                    }
                }

                let table_base = self.machine.acquire_temp_gpr().unwrap();
                let table_count = self.machine.acquire_temp_gpr().unwrap();
                let sigidx = self.machine.acquire_temp_gpr().unwrap();

                if let Some(local_table_index) = self.module.local_table_index(table_index) {
                    let (vmctx_offset_base, vmctx_offset_len) = (
                        self.vmoffsets.vmctx_vmtable_definition(local_table_index),
                        self.vmoffsets
                            .vmctx_vmtable_definition_current_elements(local_table_index),
                    );
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), vmctx_offset_base as i32),
                        Location::GPR(table_base),
                    );
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Memory(Machine::get_vmctx_reg(), vmctx_offset_len as i32),
                        Location::GPR(table_count),
                    );
                } else {
                    // Do an indirection.
                    let import_offset = self.vmoffsets.vmctx_vmtable_import(table_index);
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(Machine::get_vmctx_reg(), import_offset as i32),
                        Location::GPR(table_base),
                    );

                    // Load len.
                    self.machine.specific.move_location(
                        Size::S32,
                        Location::Memory(
                            table_base,
                            self.vmoffsets.vmtable_definition_current_elements() as _,
                        ),
                        Location::GPR(table_count),
                    );

                    // Load base.
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::Memory(table_base, self.vmoffsets.vmtable_definition_base() as _),
                        Location::GPR(table_base),
                    );
                }

                self.machine.specific.location_cmp(
                    Size::S32,
                    func_index,
                    Location::GPR(table_count),
                );
                self.machine
                    .specific
                    .jmp_on_belowequal(self.special_labels.table_access_oob);
                self.machine.specific.move_location(
                    Size::S32,
                    func_index,
                    Location::GPR(table_count),
                );
                self.machine.specific.emit_imul_imm32(
                    Size::S64,
                    self.vmoffsets.size_of_vm_funcref() as u32,
                    table_count,
                );
                self.machine.specific.location_add(
                    Size::S64,
                    Location::GPR(table_base),
                    Location::GPR(table_count),
                    false,
                );

                // deref the table to get a VMFuncRef
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(table_count, self.vmoffsets.vm_funcref_anyfunc_ptr() as i32),
                    Location::GPR(table_count),
                );
                // Trap if the FuncRef is null
                self.machine.specific.location_cmp(
                    Size::S64,
                    Location::Imm32(0),
                    Location::GPR(table_count),
                );
                self.machine
                    .specific
                    .jmp_on_equal(self.special_labels.indirect_call_null);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_vmshared_signature_id(index) as i32,
                    ),
                    Location::GPR(sigidx),
                );

                // Trap if signature mismatches.
                self.machine.specific.location_cmp(
                    Size::S32,
                    Location::GPR(sigidx),
                    Location::Memory(
                        table_count,
                        (self.vmoffsets.vmcaller_checked_anyfunc_type_index() as usize) as i32,
                    ),
                );
                self.machine
                    .specific
                    .jmp_on_different(self.special_labels.bad_signature);

                self.machine.release_temp_gpr(sigidx);
                self.machine.release_temp_gpr(table_count);
                self.machine.release_temp_gpr(table_base);

                let gpr_for_call = self.machine.specific.get_grp_for_call();
                if table_count != gpr_for_call {
                    self.machine.specific.move_location(
                        Size::S64,
                        Location::GPR(table_count),
                        Location::GPR(gpr_for_call),
                    );
                }

                self.machine.release_locations_only_osr_state(params.len());

                let vmcaller_checked_anyfunc_func_ptr =
                    self.vmoffsets.vmcaller_checked_anyfunc_func_ptr() as usize;
                let vmcaller_checked_anyfunc_vmctx =
                    self.vmoffsets.vmcaller_checked_anyfunc_vmctx() as usize;
                let calling_convention = self.calling_convention;

                self.emit_call_native(
                    |this| {
                        if this
                            .machine
                            .specific
                            .arch_requires_indirect_call_trampoline()
                        {
                            this.machine
                                .specific
                                .arch_emit_indirect_call_with_trampoline(Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_func_ptr as i32,
                                ));
                        } else {
                            let offset = this.machine.assembler_get_offset().0;
                            this.trap_table
                                .offset_to_code
                                .insert(offset, TrapCode::StackOverflow);

                            // We set the context pointer
                            this.machine.specific.move_location(
                                Size::S64,
                                Location::Memory(
                                    gpr_for_call,
                                    vmcaller_checked_anyfunc_vmctx as i32,
                                ),
                                Machine::get_param_location(0, calling_convention),
                            );

                            this.machine.specific.emit_call_location(Location::Memory(
                                gpr_for_call,
                                vmcaller_checked_anyfunc_func_ptr as i32,
                            ));
                            this.mark_instruction_address_end(offset);
                        }
                    },
                    params.iter().copied(),
                )?;

                self.machine.release_locations_only_stack(&params);

                if !return_types.is_empty() {
                    let ret = self.machine.acquire_locations(
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    if return_types[0].is_float() {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::SIMD(self.machine.specific.get_simd_for_ret()),
                            ret,
                        );
                        self.fp_stack
                            .push(FloatValue::new(self.value_stack.len() - 1));
                    } else {
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            ret,
                        );
                    }
                }
            }
            Operator::If { ty } => {
                let label_end = self.machine.specific.get_label();
                let label_else = self.machine.specific.get_label();

                let cond = self.pop_value_released();

                let frame = ControlFrame {
                    label: label_end,
                    loop_like: false,
                    if_else: IfElseState::If(label_else),
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "If: multi-value returns not yet implemented".to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: self.get_state_diff(),
                };
                self.control_stack.push(frame);
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(label_else);
            }
            Operator::Else => {
                let frame = self.control_stack.last_mut().unwrap();

                if !was_unreachable && !frame.returns.is_empty() {
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                        {
                            self.machine.canonicalize_nan(
                                match first_return {
                                    WpType::F32 => Size::S32,
                                    WpType::F64 => Size::S64,
                                    _ => unreachable!(),
                                },
                                loc,
                                Location::GPR(GPR::RAX),
                            );
                        } else {
                            self.machine.specific.emit_relaxed_mov(
                                Size::S64,
                                loc,
                                Location::GPR(GPR::RAX),
                            );
                        }
                    } else {
                        self.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            loc,
                            Location::GPR(GPR::RAX),
                        );
                    }
                }

                let mut frame = self.control_stack.last_mut().unwrap();

                let released: &[Location] = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations(released);
                self.value_stack.truncate(frame.value_stack_depth);
                self.fp_stack.truncate(frame.fp_stack_depth);

                match frame.if_else {
                    IfElseState::If(label) => {
                        self.machine
                            .specific
                            .assembler
                            .emit_jmp(Condition::None, frame.label);
                        self.machine.specific.emit_label(label);
                        frame.if_else = IfElseState::Else;
                    }
                    _ => {
                        return Err(CodegenError {
                            message: "Else: frame.if_else unreachable code".to_string(),
                        })
                    }
                }
            }
            // `TypedSelect` must be used for extern refs so ref counting should
            // be done with TypedSelect. But otherwise they're the same.
            Operator::TypedSelect { .. } | Operator::Select => {
                let cond = self.pop_value_released();
                let v_b = self.pop_value_released();
                let v_a = self.pop_value_released();
                let cncl: Option<(Option<CanonicalizeType>, Option<CanonicalizeType>)> =
                    if self.fp_stack.len() >= 2
                        && self.fp_stack[self.fp_stack.len() - 2].depth == self.value_stack.len()
                        && self.fp_stack[self.fp_stack.len() - 1].depth
                            == self.value_stack.len() + 1
                    {
                        let (left, right) = self.fp_stack.pop2()?;
                        self.fp_stack.push(FloatValue::new(self.value_stack.len()));
                        Some((left.canonicalization, right.canonicalization))
                    } else {
                        None
                    };
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let end_label = self.machine.specific.get_label();
                let zero_label = self.machine.specific.get_label();

                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(zero_label);
                match cncl {
                    Some((Some(fp), _))
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization =>
                    {
                        self.machine.canonicalize_nan(fp.to_size(), v_a, ret);
                    }
                    _ => {
                        if v_a != ret {
                            self.machine.specific.emit_relaxed_mov(Size::S64, v_a, ret);
                        }
                    }
                }
                self.machine.specific.jmp_unconditionnal(end_label);
                self.machine.specific.emit_label(zero_label);
                match cncl {
                    Some((_, Some(fp)))
                        if self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization =>
                    {
                        self.machine.canonicalize_nan(fp.to_size(), v_b, ret);
                    }
                    _ => {
                        if v_b != ret {
                            self.machine.specific.emit_relaxed_mov(Size::S64, v_b, ret);
                        }
                    }
                }
                self.machine.specific.emit_label(end_label);
            }
            Operator::Block { ty } => {
                let frame = ControlFrame {
                    label: self.machine.specific.get_label(),
                    loop_like: false,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "Block: multi-value returns not yet implemented"
                                    .to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: self.get_state_diff(),
                };
                self.control_stack.push(frame);
            }
            Operator::Loop { ty } => {
                self.machine.specific.align_for_loop();
                let label = self.machine.specific.get_label();
                let state_diff_id = self.get_state_diff();
                let _activate_offset = self.machine.assembler_get_offset().0;

                self.control_stack.push(ControlFrame {
                    label,
                    loop_like: true,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: "Loop: multi-value returns not yet implemented"
                                    .to_string(),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    fp_stack_depth: self.fp_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id,
                });
                self.machine.specific.emit_label(label);

                // TODO: Re-enable interrupt signal check without branching
            }
            Operator::Nop => {}
            Operator::MemorySize { mem, mem_byte: _ } => {
                let memory_index = MemoryIndex::new(mem as usize);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index]
                    iter::once(Location::Imm32(memory_index.index() as u32)),
                )?;
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::MemoryInit { segment, mem } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dst]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_memory_init_index())
                            as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, segment_index, dst, src, len]
                    [
                        Location::Imm32(mem),
                        Location::Imm32(segment),
                        dst,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;
                self.machine.release_locations_only_stack(&[dst, src, len]);
            }
            Operator::DataDrop { segment } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_data_drop_index())
                            as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, segment_index]
                    iter::once(Location::Imm32(segment)),
                )?;
            }
            Operator::MemoryCopy { src, dst } => {
                // ignore until we support multiple memories
                let _dst = dst;
                let len = self.value_stack.pop().unwrap();
                let src_pos = self.value_stack.pop().unwrap();
                let dst_pos = self.value_stack.pop().unwrap();
                self.machine
                    .release_locations_only_regs(&[len, src_pos, dst_pos]);

                let memory_index = MemoryIndex::new(src as usize);
                let (memory_copy_index, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_copy_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_copy_index(),
                            memory_index,
                        )
                    };

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_copy_index) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [
                        Location::Imm32(memory_index.index() as u32),
                        dst_pos,
                        src_pos,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;
                self.machine
                    .release_locations_only_stack(&[dst_pos, src_pos, len]);
            }
            Operator::MemoryFill { mem } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dst = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, val, dst]);

                let memory_index = MemoryIndex::new(mem as usize);
                let (memory_fill_index, memory_index) =
                    if self.module.local_memory_index(memory_index).is_some() {
                        (
                            VMBuiltinFunctionIndex::get_memory_fill_index(),
                            memory_index,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory_fill_index(),
                            memory_index,
                        )
                    };

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(memory_fill_index) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, memory_index, dst, src, len]
                    [Location::Imm32(memory_index.index() as u32), dst, val, len]
                        .iter()
                        .cloned(),
                )?;
                self.machine.release_locations_only_stack(&[dst, val, len]);
            }
            Operator::MemoryGrow { mem, mem_byte: _ } => {
                let memory_index = MemoryIndex::new(mem as usize);
                let param_pages = self.value_stack.pop().unwrap();

                self.machine.release_locations_only_regs(&[param_pages]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_memory_index(memory_index).is_some() {
                                VMBuiltinFunctionIndex::get_memory32_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_memory32_grow_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );

                self.machine.release_locations_only_osr_state(1);

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, val, memory_index]
                    iter::once(param_pages)
                        .chain(iter::once(Location::Imm32(memory_index.index() as u32))),
                )?;

                self.machine.release_locations_only_stack(&[param_pages]);

                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::I32Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::F32Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                self.memory_op(target, memarg, false, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32Load8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32Load8S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_sign_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32Load16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32Load16S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_sign_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::F32Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                let fp = self.fp_stack.pop1()?;
                let config_nan_canonicalization = self.config.enable_nan_canonicalization;

                self.memory_op(target_addr, memarg, false, 4, |this, addr| {
                    if !this.machine.arch_supports_canonicalize_nan()
                        || !config_nan_canonicalization
                        || fp.canonicalization.is_none()
                    {
                        this.machine.specific.emit_relaxed_mov(
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                    } else {
                        this.machine.canonicalize_nan(
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                    }

                    Ok(())
                })?;
            }
            Operator::I32Store8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S8,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I32Store16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S16,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 8, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::F64Load { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.fp_stack
                    .push(FloatValue::new(self.value_stack.len() - 1));

                self.memory_op(target, memarg, false, 8, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load8S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_sign_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load16S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_sign_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load32U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 4, |this, addr| {
                    match ret {
                        Location::GPR(_) => {}
                        Location::Memory(base, offset) => {
                            this.machine.specific.move_location(
                                Size::S32,
                                Location::Imm32(0),
                                Location::Memory(base, offset + 4),
                            ); // clear upper bits
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "I64Load32U ret: unreachable code".to_string(),
                            })
                        }
                    }
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Load32S { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, false, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_sign_extension(
                        Size::S32,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 8, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::F64Store { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();
                let fp = self.fp_stack.pop1()?;
                let config_nan_canonicalization = self.config.enable_nan_canonicalization;

                self.memory_op(target_addr, memarg, false, 8, |this, addr| {
                    if !this.machine.arch_supports_canonicalize_nan()
                        || !config_nan_canonicalization
                        || fp.canonicalization.is_none()
                    {
                        this.machine.specific.emit_relaxed_mov(
                            Size::S64,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                    } else {
                        this.machine.canonicalize_nan(
                            Size::S64,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                    }
                    Ok(())
                })?;
            }
            Operator::I64Store8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S8,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64Store16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S16,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64Store32 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, false, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::Unreachable => {
                self.mark_trappable();
                let offset = self.machine.assembler_get_offset().0;
                self.trap_table
                    .offset_to_code
                    .insert(offset, TrapCode::UnreachableCodeReached);
                self.machine.specific.emit_illegal_op();
                self.mark_instruction_address_end(offset);
                self.unreachable_depth = 1;
            }
            Operator::Return => {
                let frame = &self.control_stack[0];
                if !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "Return: incorrect frame.returns".to_string(),
                        });
                    }
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame = &self.control_stack[0];
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);
                self.unreachable_depth = 1;
            }
            Operator::Br { relative_depth } => {
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "Br: incorrect frame.returns".to_string(),
                        });
                    }
                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];

                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth } => {
                let after = self.machine.specific.get_label();
                let cond = self.pop_value_released();
                self.machine
                    .specific
                    .emit_relaxed_cmp(Size::S32, Location::Imm32(0), cond);
                self.machine.specific.jmp_on_equal(after);

                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && !frame.returns.is_empty() {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: "BrIf: incorrect frame.returns".to_string(),
                        });
                    }

                    let first_return = frame.returns[0];
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if first_return.is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        first_return,
                        canonicalize,
                        loc,
                    );
                }
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(released);
                self.machine.specific.jmp_unconditionnal(frame.label);

                self.machine.specific.emit_label(after);
            }
            Operator::BrTable { ref table } => {
                let mut targets = table
                    .targets()
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| CodegenError {
                        message: format!("BrTable read_table: {:?}", e),
                    })?;
                let default_target = targets.pop().unwrap().0;
                let cond = self.pop_value_released();
                let table_label = self.machine.specific.get_label();
                let mut table: Vec<DynamicLabel> = vec![];
                let default_br = self.machine.specific.get_label();
                self.machine.specific.emit_relaxed_cmp(
                    Size::S32,
                    Location::Imm32(targets.len() as u32),
                    cond,
                );
                self.machine.specific.jmp_on_aboveequal(default_br);

                self.machine
                    .specific
                    .emit_jmp_to_jumptable(table_label, cond);

                for (target, _) in targets.iter() {
                    let label = self.machine.specific.get_label();
                    self.machine.specific.emit_label(label);
                    table.push(label);
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    if !frame.loop_like && !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: format!(
                                    "BrTable: incorrect frame.returns for {:?}",
                                    target
                                ),
                            });
                        }

                        let first_return = frame.returns[0];
                        let loc = *self.value_stack.last().unwrap();
                        let canonicalize = if first_return.is_float() {
                            let fp = self.fp_stack.peek1()?;
                            self.machine.arch_supports_canonicalize_nan()
                                && self.config.enable_nan_canonicalization
                                && fp.canonicalization.is_some()
                        } else {
                            false
                        };
                        self.machine.specific.emit_function_return_value(
                            first_return,
                            canonicalize,
                            loc,
                        );
                    }
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(released);
                    self.machine.specific.jmp_unconditionnal(frame.label);
                }
                self.machine.specific.emit_label(default_br);

                {
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    if !frame.loop_like && !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: "BrTable: incorrect frame.returns".to_string(),
                            });
                        }

                        let first_return = frame.returns[0];
                        let loc = *self.value_stack.last().unwrap();
                        let canonicalize = if first_return.is_float() {
                            let fp = self.fp_stack.peek1()?;
                            self.machine.arch_supports_canonicalize_nan()
                                && self.config.enable_nan_canonicalization
                                && fp.canonicalization.is_some()
                        } else {
                            false
                        };
                        self.machine.specific.emit_function_return_value(
                            first_return,
                            canonicalize,
                            loc,
                        );
                    }
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(released);
                    self.machine.specific.jmp_unconditionnal(frame.label);
                }

                self.machine.specific.emit_label(table_label);
                for x in table {
                    self.machine.specific.jmp_unconditionnal(x);
                }
                self.unreachable_depth = 1;
            }
            Operator::Drop => {
                self.pop_value_released();
                if let Some(x) = self.fp_stack.last() {
                    if x.depth == self.value_stack.len() {
                        self.fp_stack.pop1()?;
                    }
                }
            }
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();

                if !was_unreachable && !frame.returns.is_empty() {
                    let loc = *self.value_stack.last().unwrap();
                    let canonicalize = if frame.returns[0].is_float() {
                        let fp = self.fp_stack.peek1()?;
                        self.machine.arch_supports_canonicalize_nan()
                            && self.config.enable_nan_canonicalization
                            && fp.canonicalization.is_some()
                    } else {
                        false
                    };
                    self.machine.specific.emit_function_return_value(
                        frame.returns[0],
                        canonicalize,
                        loc,
                    );
                }

                if self.control_stack.is_empty() {
                    self.machine.specific.emit_label(frame.label);
                    self.machine
                        .finalize_locals(&self.locals, self.config.calling_convention);
                    self.machine.specific.emit_function_epilog();

                    // Make a copy of the return value in XMM0, as required by the SysV CC.
                    match self.signature.results() {
                        [x] if *x == Type::F32 || *x == Type::F64 => {
                            self.machine.specific.emit_function_return_float();
                        }
                        _ => {}
                    }
                    self.machine.specific.emit_ret();
                } else {
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations(released);
                    self.value_stack.truncate(frame.value_stack_depth);
                    self.fp_stack.truncate(frame.fp_stack_depth);

                    if !frame.loop_like {
                        self.machine.specific.emit_label(frame.label);
                    }

                    if let IfElseState::If(label) = frame.if_else {
                        self.machine.specific.emit_label(label);
                    }

                    if !frame.returns.is_empty() {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: "End: incorrect frame.returns".to_string(),
                            });
                        }
                        let loc = self.machine.acquire_locations(
                            &[(
                                frame.returns[0],
                                MachineValue::WasmStack(self.value_stack.len()),
                            )],
                            false,
                        )[0];
                        self.machine.specific.move_location(
                            Size::S64,
                            Location::GPR(self.machine.specific.get_gpr_for_ret()),
                            loc,
                        );
                        self.value_stack.push(loc);
                        if frame.returns[0].is_float() {
                            self.fp_stack
                                .push(FloatValue::new(self.value_stack.len() - 1));
                            // we already canonicalized at the `Br*` instruction or here previously.
                        }
                    }
                }
            }
            Operator::AtomicFence { flags: _ } => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S32,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S32,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S32,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S8,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S16,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 8, |this, addr| {
                    this.machine.specific.emit_relaxed_mov(
                        Size::S64,
                        Location::Memory(addr, 0),
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S8,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S16,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.memory_op(target, memarg, true, 4, |this, addr| {
                    match ret {
                        Location::GPR(_) => {}
                        Location::Memory(base, offset) => {
                            this.machine.specific.move_location(
                                Size::S32,
                                Location::Imm32(0),
                                Location::Memory(base, offset + 4),
                            ); // clear upper bits
                        }
                        _ => {
                            return Err(CodegenError {
                                message: "I64AtomicLoad32U ret: unreachable code".to_string(),
                            })
                        }
                    }
                    this.machine.specific.emit_relaxed_zero_extension(
                        Size::S32,
                        Location::Memory(addr, 0),
                        Size::S64,
                        ret,
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicStore { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 8, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S64,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicStore8 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 1, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S8,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicStore16 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 2, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S16,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let target_value = self.pop_value_released();
                let target_addr = self.pop_value_released();

                self.memory_op(target_addr, memarg, true, 4, |this, addr| {
                    this.machine.specific.emit_relaxed_atomic_xchg(
                        Size::S32,
                        target_value,
                        Location::Memory(addr, 0),
                    );
                    Ok(())
                })?;
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .move_location(Size::S32, loc, Location::GPR(value));
                    self.memory_op(target, memarg, true, 4, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S32,
                        Size::S32,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S32,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .move_location(Size::S64, loc, Location::GPR(value));
                    self.memory_op(target, memarg, true, 8, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S64,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S64,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S64,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.move_location_extend(
                        Size::S8,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 1, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S8,
                        Size::S32,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S8,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.move_location_extend(
                        Size::S16,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 2, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S16,
                        Size::S32,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S16,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.move_location_extend(
                        Size::S8,
                        false,
                        loc,
                        Size::S64,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 1, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S8,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S8,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.move_location_extend(
                        Size::S16,
                        false,
                        loc,
                        Size::S64,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 2, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S16,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S16,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine
                        .specific
                        .move_location(Size::S32, loc, Location::GPR(value));
                    self.memory_op(target, memarg, true, 4, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S32,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_add(
                                Size::S32,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S32,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 4, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S32,
                        Size::S32,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S32,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S64,
                        false,
                        loc,
                        Size::S64,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 8, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S64,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S64,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S64,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S8,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 1, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S8,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S8,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S16,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 2, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S32, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S16,
                        Size::S32,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S16,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S8,
                        false,
                        loc,
                        Size::S64,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 1, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S8,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S8,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S16,
                        false,
                        loc,
                        Size::S64,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 2, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S16,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S16,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if self.machine.specific.has_atomic_xadd() {
                    let value = self.machine.acquire_temp_gpr().unwrap();
                    self.machine.specific.location_neg(
                        Size::S32,
                        false,
                        loc,
                        Size::S32,
                        Location::GPR(value),
                    );
                    self.memory_op(target, memarg, true, 2, |this, addr| {
                        this.machine.specific.emit_atomic_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    })?;
                    self.machine
                        .specific
                        .move_location(Size::S64, Location::GPR(value), ret);
                    self.machine.release_temp_gpr(value);
                } else {
                    self.emit_compare_and_swap(
                        loc,
                        target,
                        ret,
                        memarg,
                        4,
                        Size::S32,
                        Size::S64,
                        |this, src, dst| {
                            this.machine.specific.location_sub(
                                Size::S32,
                                Location::GPR(src),
                                Location::GPR(dst),
                                false,
                            );
                        },
                    )?;
                }
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_and(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_or(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S32,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.emit_compare_and_swap(
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |this, src, dst| {
                        this.machine.specific.location_xor(
                            Size::S64,
                            Location::GPR(src),
                            Location::GPR(dst),
                            false,
                        );
                    },
                )?;
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 4, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S32, Size::S32, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 8, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S64, Size::S64, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S8, Size::S32, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 2, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S16, Size::S32, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S8, Size::S64, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 2, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S16, Size::S64, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let loc = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_xchg_temp_gpr();
                self.memory_op(target, memarg, true, 4, |this, addr| {
                    this.machine
                        .emit_atomic_xchg(Size::S32, Size::S64, false, loc, addr, ret);
                    Ok(())
                })?;
                self.machine.release_xchg_temp_gpr();
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 4, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S32,
                        Size::S32,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 8, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S64,
                        Size::S64,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S32,
                        Size::S8,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S32,
                        Size::S16,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S64,
                        Size::S8,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S64,
                        Size::S16,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let new = self.pop_value_released();
                let cmp = self.pop_value_released();
                let target = self.pop_value_released();
                let ret = self.machine.acquire_locations(
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                self.machine.reserve_cmpxchg_temp_gpr();
                self.memory_op(target, memarg, true, 1, |this, addr| {
                    this.machine.emit_atomic_cmpxchg(
                        Size::S64,
                        Size::S32,
                        false,
                        new,
                        cmp,
                        addr,
                        ret,
                    );
                    Ok(())
                })?;
                self.machine.release_cmpxchg_temp_gpr();
            }

            Operator::RefNull { .. } => {
                self.value_stack.push(Location::Imm64(0));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(0));
            }
            Operator::RefFunc { function_index } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_func_ref_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: unclear if we need this? check other new insts with no stack ops
                // self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, func_index] -> funcref
                    iter::once(Location::Imm32(function_index as u32)),
                )?;

                let ret = self.machine.acquire_locations(
                    &[(
                        WpType::FuncRef,
                        MachineValue::WasmStack(self.value_stack.len()),
                    )],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::RefIsNull => {
                self.emit_cmpop_i64_dynamic_b(Condition::Equal, Location::Imm64(0))?;
            }
            Operator::TableSet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let value = self.value_stack.pop().unwrap();
                let index = self.value_stack.pop().unwrap();
                // double check this does what I think it does
                self.machine.release_locations_only_regs(&[value, index]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_set_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_set_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 2?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index, reftype]
                    [Location::Imm32(table_index.index() as u32), index, value]
                        .iter()
                        .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[index, value]);
            }
            Operator::TableGet { table: index } => {
                let table_index = TableIndex::new(index as _);
                let index = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[index]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_get_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_get_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index] -> reftype
                    [Location::Imm32(table_index.index() as u32), index]
                        .iter()
                        .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[index]);

                let ret = self.machine.acquire_locations(
                    &[(
                        WpType::FuncRef,
                        MachineValue::WasmStack(self.value_stack.len()),
                    )],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S64,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableSize { table: index } => {
                let table_index = TableIndex::new(index as _);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_size_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_size_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index] -> i32
                    iter::once(Location::Imm32(table_index.index() as u32)),
                )?;

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableGrow { table: index } => {
                let table_index = TableIndex::new(index as _);
                let delta = self.value_stack.pop().unwrap();
                let init_value = self.value_stack.pop().unwrap();
                self.machine
                    .release_locations_only_regs(&[delta, init_value]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets.vmctx_builtin_function(
                            if self.module.local_table_index(table_index).is_some() {
                                VMBuiltinFunctionIndex::get_table_grow_index()
                            } else {
                                VMBuiltinFunctionIndex::get_imported_table_get_index()
                            },
                        ) as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 2?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, init_value, delta, table_index] -> u32
                    [
                        init_value,
                        delta,
                        Location::Imm32(table_index.index() as u32),
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine
                    .release_locations_only_stack(&[init_value, delta]);

                let ret = self.machine.acquire_locations(
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                self.machine.specific.move_location(
                    Size::S32,
                    Location::GPR(self.machine.specific.get_gpr_for_ret()),
                    ret,
                );
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_copy_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, dst_table_index, src_table_index, dst, src, len]
                    [
                        Location::Imm32(dst_table),
                        Location::Imm32(src_table),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, src, len]);
            }

            Operator::TableFill { table } => {
                let len = self.value_stack.pop().unwrap();
                let val = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, val, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_fill_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, start_idx, item, len]
                    [Location::Imm32(table), dest, val, len].iter().cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, val, len]);
            }
            Operator::TableInit { segment, table } => {
                let len = self.value_stack.pop().unwrap();
                let src = self.value_stack.pop().unwrap();
                let dest = self.value_stack.pop().unwrap();
                self.machine.release_locations_only_regs(&[len, src, dest]);

                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_table_init_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: should this be 3?
                self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, table_index, elem_index, dst, src, len]
                    [
                        Location::Imm32(table),
                        Location::Imm32(segment),
                        dest,
                        src,
                        len,
                    ]
                    .iter()
                    .cloned(),
                )?;

                self.machine.release_locations_only_stack(&[dest, src, len]);
            }
            Operator::ElemDrop { segment } => {
                self.machine.specific.move_location(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        self.vmoffsets
                            .vmctx_builtin_function(VMBuiltinFunctionIndex::get_elem_drop_index())
                            as i32,
                    ),
                    Location::GPR(self.machine.specific.get_grp_for_call()),
                );

                // TODO: do we need this?
                //self.machine.release_locations_only_osr_state(1);
                self.emit_call_native(
                    |this| {
                        this.machine
                            .specific
                            .emit_call_register(this.machine.specific.get_grp_for_call());
                    },
                    // [vmctx, elem_index]
                    [Location::Imm32(segment)].iter().cloned(),
                )?;
            }
            _ => {
                return Err(CodegenError {
                    message: format!("not yet implemented: {:?}", op),
                });
            }
        }

        Ok(())
    }

    pub fn finalize(mut self, data: &FunctionBodyData) -> CompiledFunction {
        // Generate actual code for special labels.
        self.machine
            .specific
            .emit_label(self.special_labels.integer_division_by_zero);
        self.mark_address_with_trap_code(TrapCode::IntegerDivisionByZero);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.heap_access_oob);
        self.mark_address_with_trap_code(TrapCode::HeapAccessOutOfBounds);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.table_access_oob);
        self.mark_address_with_trap_code(TrapCode::TableAccessOutOfBounds);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.indirect_call_null);
        self.mark_address_with_trap_code(TrapCode::IndirectCallToNull);
        self.machine.specific.emit_illegal_op();

        self.machine
            .specific
            .emit_label(self.special_labels.bad_signature);
        self.mark_address_with_trap_code(TrapCode::BadSignature);
        self.machine.specific.emit_illegal_op();

        // Notify the assembler backend to generate necessary code at end of function.
        self.machine.specific.finalize_function();

        let body_len = self.machine.assembler_get_offset().0;
        let instructions_address_map = self.instructions_address_map;
        let address_map = get_function_address_map(instructions_address_map, data, body_len);

        CompiledFunction {
            body: FunctionBody {
                body: self.machine.assembler_finalize(),
                unwind_info: None,
            },
            relocations: self.relocations,
            jt_offsets: SecondaryMap::new(),
            frame_info: CompiledFunctionFrameInfo {
                traps: self
                    .trap_table
                    .offset_to_code
                    .into_iter()
                    .map(|(offset, code)| TrapInformation {
                        code_offset: offset as u32,
                        trap_code: code,
                    })
                    .collect(),
                address_map,
            },
        }
    }
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
        Type::ExternRef => WpType::ExternRef,
        Type::FuncRef => WpType::FuncRef, // TODO: FuncRef or Func?
    }
}

// FIXME: This implementation seems to be not enough to resolve all kinds of register dependencies
// at call place.
fn sort_call_movs(movs: &mut [(Location, GPR)]) {
    for i in 0..movs.len() {
        for j in (i + 1)..movs.len() {
            if let Location::GPR(src_gpr) = movs[j].0 {
                if src_gpr == movs[i].1 {
                    movs.swap(i, j);
                }
            }
        }
    }

    // Cycle detector. Uncomment this to debug possibly incorrect call-mov sequences.
    /*
    {
        use std::collections::{HashMap, HashSet, VecDeque};
        let mut mov_map: HashMap<GPR, HashSet<GPR>> = HashMap::new();
        for mov in movs.iter() {
            if let Location::GPR(src_gpr) = mov.0 {
                if src_gpr != mov.1 {
                    mov_map.entry(src_gpr).or_insert_with(|| HashSet::new()).insert(mov.1);
                }
            }
        }

        for (start, _) in mov_map.iter() {
            let mut q: VecDeque<GPR> = VecDeque::new();
            let mut black: HashSet<GPR> = HashSet::new();

            q.push_back(*start);
            black.insert(*start);

            while q.len() > 0 {
                let reg = q.pop_front().unwrap();
                let empty_set = HashSet::new();
                for x in mov_map.get(&reg).unwrap_or(&empty_set).iter() {
                    if black.contains(x) {
                        panic!("cycle detected");
                    }
                    q.push_back(*x);
                    black.insert(*x);
                }
            }
        }
    }
    */
}

// Standard entry trampoline.
pub fn gen_std_trampoline(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);

    // Calculate stack offset.
    let mut stack_offset: u32 = 0;
    for (i, _param) in sig.params().iter().enumerate() {
        if let Location::Memory(_, _) = Machine::get_param_location(1 + i, calling_convention) {
            stack_offset += 8;
        }
    }
    let stack_padding: u32 = match calling_convention {
        CallingConvention::WindowsFastcall => 32,
        _ => 0,
    };

    // Align to 16 bytes. We push two 8-byte registers below, so here we need to ensure stack_offset % 16 == 8.
    if stack_offset % 16 != 8 {
        stack_offset += 8;
    }

    // Used callee-saved registers
    a.emit_push(Size::S64, Location::GPR(GPR::R15));
    a.emit_push(Size::S64, Location::GPR(GPR::R14));

    // Prepare stack space.
    a.emit_sub(
        Size::S64,
        Location::Imm32(stack_offset + stack_padding),
        Location::GPR(GPR::RSP),
    );

    // Arguments
    a.emit_mov(
        Size::S64,
        Machine::get_param_location(1, calling_convention),
        Location::GPR(GPR::R15),
    ); // func_ptr
    a.emit_mov(
        Size::S64,
        Machine::get_param_location(2, calling_convention),
        Location::GPR(GPR::R14),
    ); // args_rets

    // Move arguments to their locations.
    // `callee_vmctx` is already in the first argument register, so no need to move.
    {
        let mut n_stack_args: usize = 0;
        for (i, _param) in sig.params().iter().enumerate() {
            let src_loc = Location::Memory(GPR::R14, (i * 16) as _); // args_rets[i]
            let dst_loc = Machine::get_param_location(1 + i, calling_convention);

            match dst_loc {
                Location::GPR(_) => {
                    a.emit_mov(Size::S64, src_loc, dst_loc);
                }
                Location::Memory(_, _) => {
                    // This location is for reading arguments but we are writing arguments here.
                    // So recalculate it.
                    a.emit_mov(Size::S64, src_loc, Location::GPR(GPR::RAX));
                    a.emit_mov(
                        Size::S64,
                        Location::GPR(GPR::RAX),
                        Location::Memory(
                            GPR::RSP,
                            (stack_padding as usize + n_stack_args * 8) as _,
                        ),
                    );
                    n_stack_args += 1;
                }
                _ => unreachable!(),
            }
        }
    }

    // Call.
    a.emit_call_location(Location::GPR(GPR::R15));

    // Restore stack.
    a.emit_add(
        Size::S64,
        Location::Imm32(stack_offset + stack_padding),
        Location::GPR(GPR::RSP),
    );

    // Write return value.
    if !sig.results().is_empty() {
        a.emit_mov(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::R14, 0),
        );
    }

    // Restore callee-saved registers.
    a.emit_pop(Size::S64, Location::GPR(GPR::R14));
    a.emit_pop(Size::S64, Location::GPR(GPR::R15));

    a.emit_ret();

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}

/// Generates dynamic import function call trampoline for a function type.
pub fn gen_std_dynamic_import_trampoline(
    vmoffsets: &VMOffsets,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> FunctionBody {
    let mut a = Assembler::new(0);

    // Allocate argument array.
    let stack_offset: usize = 16 * std::cmp::max(sig.params().len(), sig.results().len()) + 8; // 16 bytes each + 8 bytes sysv call padding
    let stack_padding: usize = match calling_convention {
        CallingConvention::WindowsFastcall => 32,
        _ => 0,
    };
    a.emit_sub(
        Size::S64,
        Location::Imm32((stack_offset + stack_padding) as _),
        Location::GPR(GPR::RSP),
    );

    // Copy arguments.
    if !sig.params().is_empty() {
        let mut argalloc = ArgumentRegisterAllocator::default();
        argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext

        let mut stack_param_count: usize = 0;

        for (i, ty) in sig.params().iter().enumerate() {
            let source_loc = match argalloc.next(*ty, calling_convention) {
                Some(X64Register::GPR(gpr)) => Location::GPR(gpr),
                Some(X64Register::XMM(xmm)) => Location::SIMD(xmm),
                None => {
                    a.emit_mov(
                        Size::S64,
                        Location::Memory(
                            GPR::RSP,
                            (stack_padding * 2 + stack_offset + 8 + stack_param_count * 8) as _,
                        ),
                        Location::GPR(GPR::RAX),
                    );
                    stack_param_count += 1;
                    Location::GPR(GPR::RAX)
                }
            };
            a.emit_mov(
                Size::S64,
                source_loc,
                Location::Memory(GPR::RSP, (stack_padding + i * 16) as _),
            );

            // Zero upper 64 bits.
            a.emit_mov(
                Size::S64,
                Location::Imm32(0),
                Location::Memory(GPR::RSP, (stack_padding + i * 16 + 8) as _),
            );
        }
    }

    match calling_convention {
        CallingConvention::WindowsFastcall => {
            // Load target address.
            a.emit_mov(
                Size::S64,
                Location::Memory(
                    GPR::RCX,
                    vmoffsets.vmdynamicfunction_import_context_address() as i32,
                ),
                Location::GPR(GPR::RAX),
            );
            // Load values array.
            a.emit_lea(
                Size::S64,
                Location::Memory(GPR::RSP, stack_padding as i32),
                Location::GPR(GPR::RDX),
            );
        }
        _ => {
            // Load target address.
            a.emit_mov(
                Size::S64,
                Location::Memory(
                    GPR::RDI,
                    vmoffsets.vmdynamicfunction_import_context_address() as i32,
                ),
                Location::GPR(GPR::RAX),
            );
            // Load values array.
            a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RSI));
        }
    };

    // Call target.
    a.emit_call_location(Location::GPR(GPR::RAX));

    // Fetch return value.
    if !sig.results().is_empty() {
        assert_eq!(sig.results().len(), 1);
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RSP, stack_padding as i32),
            Location::GPR(GPR::RAX),
        );
    }

    // Release values array.
    a.emit_add(
        Size::S64,
        Location::Imm32((stack_offset + stack_padding) as _),
        Location::GPR(GPR::RSP),
    );

    // Return.
    a.emit_ret();

    FunctionBody {
        body: a.finalize().unwrap().to_vec(),
        unwind_info: None,
    }
}

// Singlepass calls import functions through a trampoline.
pub fn gen_import_call_trampoline(
    vmoffsets: &VMOffsets,
    index: FunctionIndex,
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> CustomSection {
    let mut a = Assembler::new(0);

    // TODO: ARM entry trampoline is not emitted.

    // Singlepass internally treats all arguments as integers
    // For the standard Windows calling convention requires
    //  floating point arguments to be passed in XMM registers for the 4 first arguments only
    //  That's the only change to do, other arguments are not to be changed
    // For the standard System V calling convention requires
    //  floating point arguments to be passed in XMM registers.
    //  Translation is expensive, so only do it if needed.
    if sig
        .params()
        .iter()
        .any(|&x| x == Type::F32 || x == Type::F64)
    {
        match calling_convention {
            CallingConvention::WindowsFastcall => {
                let mut param_locations: Vec<Location> = vec![];
                for i in 0..sig.params().len() {
                    let loc = match i {
                        0..=2 => {
                            static PARAM_REGS: &[GPR] = &[GPR::RDX, GPR::R8, GPR::R9];
                            Location::GPR(PARAM_REGS[i])
                        }
                        _ => Location::Memory(GPR::RSP, 32 + 8 + ((i - 3) * 8) as i32), // will not be used anyway
                    };
                    param_locations.push(loc);
                }
                // Copy Float arguments to XMM from GPR.
                let mut argalloc = ArgumentRegisterAllocator::default();
                for (i, ty) in sig.params().iter().enumerate() {
                    let prev_loc = param_locations[i];
                    match argalloc.next(*ty, calling_convention) {
                        Some(X64Register::GPR(_gpr)) => continue,
                        Some(X64Register::XMM(xmm)) => {
                            a.emit_mov(Size::S64, prev_loc, Location::SIMD(xmm))
                        }
                        None => continue,
                    };
                }
            }
            _ => {
                let mut param_locations: Vec<Location> = vec![];

                // Allocate stack space for arguments.
                let stack_offset: i32 = if sig.params().len() > 5 {
                    5 * 8
                } else {
                    (sig.params().len() as i32) * 8
                };
                if stack_offset > 0 {
                    a.emit_sub(
                        Size::S64,
                        Location::Imm32(stack_offset as u32),
                        Location::GPR(GPR::RSP),
                    );
                }

                // Store all arguments to the stack to prevent overwrite.
                for i in 0..sig.params().len() {
                    let loc = match i {
                        0..=4 => {
                            static PARAM_REGS: &[GPR] =
                                &[GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9];
                            let loc = Location::Memory(GPR::RSP, (i * 8) as i32);
                            a.emit_mov(Size::S64, Location::GPR(PARAM_REGS[i]), loc);
                            loc
                        }
                        _ => Location::Memory(GPR::RSP, stack_offset + 8 + ((i - 5) * 8) as i32),
                    };
                    param_locations.push(loc);
                }

                // Copy arguments.
                let mut argalloc = ArgumentRegisterAllocator::default();
                argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext
                let mut caller_stack_offset: i32 = 0;
                for (i, ty) in sig.params().iter().enumerate() {
                    let prev_loc = param_locations[i];
                    let targ = match argalloc.next(*ty, calling_convention) {
                        Some(X64Register::GPR(gpr)) => Location::GPR(gpr),
                        Some(X64Register::XMM(xmm)) => Location::SIMD(xmm),
                        None => {
                            // No register can be allocated. Put this argument on the stack.
                            //
                            // Since here we never use fewer registers than by the original call, on the caller's frame
                            // we always have enough space to store the rearranged arguments, and the copy "backward" between different
                            // slots in the caller argument region will always work.
                            a.emit_mov(Size::S64, prev_loc, Location::GPR(GPR::RAX));
                            a.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::RAX),
                                Location::Memory(GPR::RSP, stack_offset + 8 + caller_stack_offset),
                            );
                            caller_stack_offset += 8;
                            continue;
                        }
                    };
                    a.emit_mov(Size::S64, prev_loc, targ);
                }

                // Restore stack pointer.
                if stack_offset > 0 {
                    a.emit_add(
                        Size::S64,
                        Location::Imm32(stack_offset as u32),
                        Location::GPR(GPR::RSP),
                    );
                }
            }
        }
    }

    // Emits a tail call trampoline that loads the address of the target import function
    // from Ctx and jumps to it.

    let offset = vmoffsets.vmctx_vmfunction_import(index);

    match calling_convention {
        CallingConvention::WindowsFastcall => {
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RCX, offset as i32), // function pointer
                Location::GPR(GPR::RAX),
            );
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RCX, offset as i32 + 8), // target vmctx
                Location::GPR(GPR::RCX),
            );
        }
        _ => {
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RDI, offset as i32), // function pointer
                Location::GPR(GPR::RAX),
            );
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RDI, offset as i32 + 8), // target vmctx
                Location::GPR(GPR::RDI),
            );
        }
    }
    a.emit_host_redirection(GPR::RAX);

    let section_body = SectionBody::new_with_vec(a.finalize().unwrap().to_vec());

    CustomSection {
        protection: CustomSectionProtection::ReadExecute,
        bytes: section_body,
        relocations: vec![],
    }
}

// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation less-than (for
// least) or greater-than (for greatest) the i32 or i64 or u32 or u64
// min (for least) or max (for greatest), when rounding towards zero.

/// Greatest Exact Float (32 bits) less-than i32::MIN when rounding towards zero.
const GEF32_LT_I32_MIN: f32 = -2147483904.0;
/// Least Exact Float (32 bits) greater-than i32::MAX when rounding towards zero.
const LEF32_GT_I32_MAX: f32 = 2147483648.0;
/// Greatest Exact Float (32 bits) less-than i64::MIN when rounding towards zero.
const GEF32_LT_I64_MIN: f32 = -9223373136366403584.0;
/// Least Exact Float (32 bits) greater-than i64::MAX when rounding towards zero.
const LEF32_GT_I64_MAX: f32 = 9223372036854775808.0;
/// Greatest Exact Float (32 bits) less-than u32::MIN when rounding towards zero.
const GEF32_LT_U32_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u32::MAX when rounding towards zero.
const LEF32_GT_U32_MAX: f32 = 4294967296.0;
/// Greatest Exact Float (32 bits) less-than u64::MIN when rounding towards zero.
const GEF32_LT_U64_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u64::MAX when rounding towards zero.
const LEF32_GT_U64_MAX: f32 = 18446744073709551616.0;

/// Greatest Exact Float (64 bits) less-than i32::MIN when rounding towards zero.
const GEF64_LT_I32_MIN: f64 = -2147483649.0;
/// Least Exact Float (64 bits) greater-than i32::MAX when rounding towards zero.
const LEF64_GT_I32_MAX: f64 = 2147483648.0;
/// Greatest Exact Float (64 bits) less-than i64::MIN when rounding towards zero.
const GEF64_LT_I64_MIN: f64 = -9223372036854777856.0;
/// Least Exact Float (64 bits) greater-than i64::MAX when rounding towards zero.
const LEF64_GT_I64_MAX: f64 = 9223372036854775808.0;
/// Greatest Exact Float (64 bits) less-than u32::MIN when rounding towards zero.
const GEF64_LT_U32_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u32::MAX when rounding towards zero.
const LEF64_GT_U32_MAX: f64 = 4294967296.0;
/// Greatest Exact Float (64 bits) less-than u64::MIN when rounding towards zero.
const GEF64_LT_U64_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u64::MAX when rounding towards zero.
const LEF64_GT_U64_MAX: f64 = 18446744073709551616.0;
