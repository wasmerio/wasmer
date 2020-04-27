use wasm_common::{
    FuncType,
    entity::{PrimaryMap, EntityRef},
};
use wasmer_runtime::{Module, MemoryPlan, TablePlan, MemoryStyle, TableStyle};
use dynasmrt::{x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};
use crate::{
    exception::{ExceptionTable, ExceptionCode},
    machine::Machine,
    config::SinglepassConfig,
    common_decl::*,
    x64_decl::*,
    emitter_x64::*,
};
use wasmparser::{
    Type as WpType,
    Operator,
    MemoryImmediate,
};
use smallvec::{smallvec, SmallVec};
use wasm_common::{
    DataIndex, DataInitializer, DataInitializerLocation, ElemIndex, ExportIndex, FuncIndex,
    GlobalIndex, GlobalType, ImportIndex, LocalFuncIndex, MemoryIndex, MemoryType, SignatureIndex,
    TableIndex, TableType, Type,
};

/// The singlepass per-function code generator.
pub struct FuncGen<'a> {
    // Immutable properties assigned at creation time.

    /// Static module information.
    module: &'a Module,

    /// Module compilation config.
    config: &'a SinglepassConfig,

    // Memory plans.
    memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,

    // Table plans.
    table_plans: PrimaryMap<TableIndex, TablePlan>,

    /// Function signature.
    signature: FuncType,

    // Working storage.

    /// The assembler.
    /// 
    /// This should be changed to `Vec<u8>` for platform independency, but dynasm doesn't (yet)
    /// support automatic relative relocations for `Vec<u8>`.
    assembler: Assembler,

    /// Memory locations of local variables.
    locals: Vec<Location>,

    /// Types of local variables.
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

    /// Exception table.
    exception_table: ExceptionTable,
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
        match self.last() {
            Some(x) => Ok(x),
            None => Err(CodegenError {
                message: "peek1() expects at least 1 element".into(),
            }),
        }
    }
    fn pop1(&mut self) -> Result<T, CodegenError> {
        match self.pop() {
            Some(x) => Ok(x),
            None => Err(CodegenError {
                message: "pop1() expects at least 1 element".into(),
            }),
        }
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

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}


struct I2O1 {
    loc_a: Location,
    loc_b: Location,
    ret: Location,
}

impl<'a> FuncGen<'a> {
    fn get_location_released(&mut self, loc: Location) -> Location {
        self.machine.release_locations(&mut self.assembler, &[loc]);
        loc
    }

    fn pop_value_released(&mut self) -> Location {
        let loc = self.value_stack.pop().expect("pop_value_released: value stack is empty");
        self.get_location_released(loc)
    }

    /// Prepare data for binary operator with 2 inputs and 1 output.
    fn i2o1_prepare(&mut self, ty: WpType) -> I2O1 {
        let loc_b = self.pop_value_released();
        let loc_a = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &mut self.assembler,
            &[(ty, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        self.value_stack.push(ret);
        I2O1 {
            loc_a,
            loc_b,
            ret,
        }
    }

    fn mark_trappable(
        &mut self
    ) {
        let state_diff_id = self.get_state_diff();
        let offset = self.assembler.get_offset().0;
        self.fsm.trappable_offsets.insert(
            offset,
            OffsetInfo {
                end_offset: offset + 1,
                activate_offset: offset,
                diff_id: state_diff_id,
            },
        );
        self.fsm.wasm_offset_to_target_offset
            .insert(self.machine.state.wasm_inst_offset, SuspendOffset::Trappable(offset));
    }

    /// Marks each address in the code range emitted by `f` with the exception code `code`.
    fn mark_range_with_exception_code<F: FnOnce(&mut Self) -> R, R>(
        &mut self,
        code: ExceptionCode,
        f: F,
    ) -> R {
        let begin = self.assembler.get_offset().0;
        let ret = f(self);
        let end = self.assembler.get_offset().0;
        for i in begin..end {
            self.exception_table.offset_to_code.insert(i, code);
        }
        ret
    }

    /// Canonicalizes the floating point value at `input` into `output`.
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location,
        output: Location,
    ) {
        let tmp1 = self.machine.acquire_temp_xmm().unwrap();
        let tmp2 = self.machine.acquire_temp_xmm().unwrap();
        let tmp3 = self.machine.acquire_temp_xmm().unwrap();
        let tmpg1 = self.machine.acquire_temp_gpr().unwrap();

        self.emit_relaxed_binop(Assembler::emit_mov, sz, input, Location::XMM(tmp1));

        match sz {
            Size::S32 => {
                self.assembler.emit_vcmpunordss(tmp1, XMMOrMemory::XMM(tmp1), tmp2);
                self.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(0x7FC0_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                );
                self.assembler.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp3));
                self.assembler.emit_vblendvps(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1);
            }
            Size::S64 => {
                self.assembler.emit_vcmpunordsd(tmp1, XMMOrMemory::XMM(tmp1), tmp2);
                self.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                );
                self.assembler.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp3));
                self.assembler.emit_vblendvpd(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1);
            }
            _ => unreachable!(),
        }

        self.emit_relaxed_binop(Assembler::emit_mov, sz, Location::XMM(tmp1), output);

        self.machine.release_temp_gpr(tmpg1);
        self.machine.release_temp_xmm(tmp3);
        self.machine.release_temp_xmm(tmp2);
        self.machine.release_temp_xmm(tmp1);
    }

    /// Moves `loc` to a valid location for `div`/`idiv`.
    fn emit_relaxed_xdiv(
        &mut self,
        op: fn(&mut Assembler, Size, Location),
        sz: Size,
        loc: Location,
    ) {
        self.machine.state.wasm_stack_private_depth += 1;
        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                self.assembler.emit_mov(sz, loc, Location::GPR(GPR::RCX)); // must not be used during div (rax, rdx)
                self.mark_trappable();
                self.exception_table
                    .offset_to_code
                    .insert(self.assembler.get_offset().0, ExceptionCode::IllegalArithmetic);
                op(&mut self.assembler, sz, Location::GPR(GPR::RCX));
            }
            _ => {
                self.mark_trappable();
                self.exception_table
                    .offset_to_code
                    .insert(self.assembler.get_offset().0, ExceptionCode::IllegalArithmetic);
                op(&mut self.assembler, sz, loc);
            }
        }
        self.machine.state.wasm_stack_private_depth -= 1;
    }

    /// Moves `src` and `dst` to valid locations for `movzx`/`movsx`.
    fn emit_relaxed_zx_sx(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Size, Location),
        sz_src: Size,
        mut src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CodegenError> {
        let inner = |m: &mut Machine, a: &mut Assembler, src: Location| match dst {
            Location::Imm32(_) | Location::Imm64(_) => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx dst Imm: unreachable code"),
                })
            }
            Location::Memory(_, _) => {
                let tmp_dst = m.acquire_temp_gpr().unwrap();
                op(a, sz_src, src, sz_dst, Location::GPR(tmp_dst));
                a.emit_mov(Size::S64, Location::GPR(tmp_dst), dst);

                m.release_temp_gpr(tmp_dst);
                Ok(())
            }
            Location::GPR(_) => {
                op(a, sz_src, src, sz_dst, dst);
                Ok(())
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx dst: unreachable code"),
                })
            }
        };

        match src {
            Location::Imm32(_) | Location::Imm64(_) => {
                let tmp_src = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(Size::S64, src, Location::GPR(tmp_src));
                src = Location::GPR(tmp_src);

                inner(&mut self.machine, &mut self.assembler, src)?;

                self.machine.release_temp_gpr(tmp_src);
            }
            Location::GPR(_) | Location::Memory(_, _) => inner(&mut self.machine, &mut self.assembler, src)?,
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx src: unreachable code"),
                })
            }
        }
        Ok(())
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
            (_, Location::XMM(_)) => RelaxMode::SrcToGPR,
            _ => RelaxMode::Direct,
        };

        match mode {
            RelaxMode::SrcToGPR => {
                let temp = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(sz, src, Location::GPR(temp));
                op(&mut self.assembler, sz, Location::GPR(temp), dst);
                self.machine.release_temp_gpr(temp);
            }
            RelaxMode::DstToGPR => {
                let temp = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(sz, dst, Location::GPR(temp));
                op(&mut self.assembler, sz, src, Location::GPR(temp));
                self.machine.release_temp_gpr(temp);
            }
            RelaxMode::BothToGPR => {
                let temp_src = self.machine.acquire_temp_gpr().unwrap();
                let temp_dst = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(sz, src, Location::GPR(temp_src));
                self.assembler.emit_mov(sz, dst, Location::GPR(temp_dst));
                op(&mut self.assembler, sz, Location::GPR(temp_src), Location::GPR(temp_dst));
                match dst {
                    Location::Memory(_, _) | Location::GPR(_) => {
                        self.assembler.emit_mov(sz, Location::GPR(temp_dst), dst);
                    }
                    _ => {}
                }
                self.machine.release_temp_gpr(temp_dst);
                self.machine.release_temp_gpr(temp_src);
            }
            RelaxMode::Direct => {
                op(&mut self.assembler, sz, src, dst);
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
            |this, src1, src2, dst| op(&mut this.assembler, src1, src2, dst),
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
        let tmp1 = self.machine.acquire_temp_xmm().unwrap();
        let tmp2 = self.machine.acquire_temp_xmm().unwrap();
        let tmp3 = self.machine.acquire_temp_xmm().unwrap();
        let tmpg = self.machine.acquire_temp_gpr().unwrap();

        let src1 = match src1 {
            Location::XMM(x) => x,
            Location::GPR(_) | Location::Memory(_, _) => {
                self.assembler.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                tmp1
            }
            Location::Imm32(_) => {
                self.assembler.emit_mov(Size::S32, src1, Location::GPR(tmpg));
                self.assembler.emit_mov(Size::S32, Location::GPR(tmpg), Location::XMM(tmp1));
                tmp1
            }
            Location::Imm64(_) => {
                self.assembler.emit_mov(Size::S64, src1, Location::GPR(tmpg));
                self.assembler.emit_mov(Size::S64, Location::GPR(tmpg), Location::XMM(tmp1));
                tmp1
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base src1: unreachable code"),
                })
            }
        };

        let src2 = match src2 {
            Location::XMM(x) => XMMOrMemory::XMM(x),
            Location::Memory(base, disp) => XMMOrMemory::Memory(base, disp),
            Location::GPR(_) => {
                self.assembler.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm32(_) => {
                self.assembler.emit_mov(Size::S32, src2, Location::GPR(tmpg));
                self.assembler.emit_mov(Size::S32, Location::GPR(tmpg), Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm64(_) => {
                self.assembler.emit_mov(Size::S64, src2, Location::GPR(tmpg));
                self.assembler.emit_mov(Size::S64, Location::GPR(tmpg), Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base src2: unreachable code"),
                })
            }
        };

        match dst {
            Location::XMM(x) => {
                op(self, src1, src2, x);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                op(self, src1, src2, tmp3);
                self.assembler.emit_mov(Size::S64, Location::XMM(tmp3), dst);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base dst: unreachable code"),
                })
            }
        }

        self.machine.release_temp_gpr(tmpg);
        self.machine.release_temp_xmm(tmp3);
        self.machine.release_temp_xmm(tmp2);
        self.machine.release_temp_xmm(tmp1);
        Ok(())
    }

    /// I32 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i32(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        // Using Red Zone here.
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);
        if loc_a != ret {
            let tmp = self.machine.acquire_temp_gpr().unwrap();
            self.emit_relaxed_binop(
                Assembler::emit_mov,
                Size::S32,
                loc_a,
                Location::GPR(tmp),
            );
            self.emit_relaxed_binop(f, Size::S32, loc_b, Location::GPR(tmp));
            self.emit_relaxed_binop(
                Assembler::emit_mov,
                Size::S32,
                Location::GPR(tmp),
                ret,
            );
            self.machine.release_temp_gpr(tmp);
        } else {
            self.emit_relaxed_binop(f, Size::S32, loc_b, ret);
        }
    }

    /// I64 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i64(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        // Using Red Zone here.
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);

        if loc_a != ret {
            let tmp = self.machine.acquire_temp_gpr().unwrap();
            self.emit_relaxed_binop(
                Assembler::emit_mov,
                Size::S64,
                loc_a,
                Location::GPR(tmp),
            );
            self.emit_relaxed_binop(f, Size::S64, loc_b, Location::GPR(tmp));
            self.emit_relaxed_binop(
                Assembler::emit_mov,
                Size::S64,
                Location::GPR(tmp),
                ret,
            );
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
            &mut self.assembler,
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                self.emit_relaxed_binop(Assembler::emit_cmp, Size::S32, loc_b, loc_a);
                self.assembler.emit_set(c, x);
                self.assembler.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x));
            }
            Location::Memory(_, _) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.emit_relaxed_binop(Assembler::emit_cmp, Size::S32, loc_b, loc_a);
                self.assembler.emit_set(c, tmp);
                self.assembler.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp));
                self.assembler.emit_mov(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_cmpop_i32_dynamic_b ret: unreachable code"),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I32 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i32(
        &mut self,
        c: Condition,
    ) -> Result<(), CodegenError> {
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
            &mut self.assembler,
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                self.emit_relaxed_binop(Assembler::emit_cmp, Size::S64, loc_b, loc_a);
                self.assembler.emit_set(c, x);
                self.assembler.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x));
            }
            Location::Memory(_, _) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.emit_relaxed_binop(Assembler::emit_cmp, Size::S64, loc_b, loc_a);
                self.assembler.emit_set(c, tmp);
                self.assembler.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp));
                self.assembler.emit_mov(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_cmpop_i64_dynamic_b ret: unreachable code"),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I64 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i64(
        &mut self,
        c: Condition,
    ) -> Result<(), CodegenError> {
        let loc_b = self.pop_value_released();
        self.emit_cmpop_i64_dynamic_b(c, loc_b)?;
        Ok(())
    }

    /// I32 `lzcnt`/`tzcnt`/`popcnt` with operand popped from the virtual stack.
    fn emit_xcnt_i32(
        &mut self,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) -> Result<(), CodegenError> {
        let loc = self.pop_value_released();
        let ret = self.machine.acquire_locations(
            &mut self.assembler,
            &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm32(_) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(Size::S32, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(&mut self.assembler, Size::S32, Location::GPR(tmp), Location::GPR(out_tmp));
                    self.assembler.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.assembler, Size::S32, Location::GPR(tmp), ret);
                }
                self.machine.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(&mut self.assembler, Size::S32, loc, Location::GPR(out_tmp));
                    self.assembler.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.assembler, Size::S32, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_xcnt_i32 loc: unreachable code"),
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
            &mut self.assembler,
            &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                self.assembler.emit_mov(Size::S64, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(&mut self.assembler, Size::S64, Location::GPR(tmp), Location::GPR(out_tmp));
                    self.assembler.emit_mov(Size::S64, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.assembler, Size::S64, Location::GPR(tmp), ret);
                }
                self.machine.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.machine.acquire_temp_gpr().unwrap();
                    f(&mut self.assembler, Size::S64, loc, Location::GPR(out_tmp));
                    self.assembler.emit_mov(Size::S64, Location::GPR(out_tmp), ret);
                    self.machine.release_temp_gpr(out_tmp);
                } else {
                    f(&mut self.assembler, Size::S64, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_xcnt_i64 loc: unreachable code"),
                })
            }
        }
        self.value_stack.push(ret);
        Ok(())
    }

    /// I32 shift with both operands popped from the virtual stack.
    fn emit_shift_i32(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I32);

        self.assembler.emit_mov(Size::S32, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            self.emit_relaxed_binop(Assembler::emit_mov, Size::S32, loc_a, ret);
        }

        f(&mut self.assembler, Size::S32, Location::GPR(GPR::RCX), ret);
    }

    /// I64 shift with both operands popped from the virtual stack.
    fn emit_shift_i64(
        &mut self,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::I64);
        self.assembler.emit_mov(Size::S64, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            self.emit_relaxed_binop(Assembler::emit_mov, Size::S64, loc_a, ret);
        }

        f(&mut self.assembler, Size::S64, Location::GPR(GPR::RCX), ret);
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
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret);
        Ok(())
    }

    /// Floating point (AVX) binary operation with both operands popped from the virtual stack.
    fn emit_fp_unop_avx(
        &mut self,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let I2O1 { loc_a, loc_b, ret } = self.i2o1_prepare(WpType::F64);

        self.emit_relaxed_avx(f, loc_a, loc_b, ret)?;
        Ok(())
    }

    /// Emits a System V call sequence.
    ///
    /// This function must not use RAX before `cb` is called.
    fn emit_call_sysv<I: Iterator<Item = Location>, F: FnOnce(&mut Self)>(
        &mut self,
        cb: F,
        params: I,
        state_context: Option<(&mut FunctionStateMap, &mut [ControlFrame])>,
    ) -> Result<(), CodegenError> {
        // Values pushed in this function are above the shadow region.
        self.machine.state.stack_values.push(MachineValue::ExplicitShadow);

        let params: Vec<_> = params.collect();

        // Save used GPRs.
        let used_gprs = self.machine.get_used_gprs();
        for r in used_gprs.iter() {
            self.assembler.emit_push(Size::S64, Location::GPR(*r));
            let content = self.machine.state.register_values[X64Register::GPR(*r).to_index().0].clone();
            if content == MachineValue::Undefined {
                return Err(CodegenError {
                    message: format!("emit_call_sysv: Undefined used_gprs content"),
                });
            }
            self.machine.state.stack_values.push(content);
        }

        // Save used XMM registers.
        let used_xmms = self.machine.get_used_xmms();
        if used_xmms.len() > 0 {
            self.assembler.emit_sub(
                Size::S64,
                Location::Imm32((used_xmms.len() * 8) as u32),
                Location::GPR(GPR::RSP),
            );

            for (i, r) in used_xmms.iter().enumerate() {
                self.assembler.emit_mov(
                    Size::S64,
                    Location::XMM(*r),
                    Location::Memory(GPR::RSP, (i * 8) as i32),
                );
            }
            for r in used_xmms.iter().rev() {
                let content = self.machine.state.register_values[X64Register::XMM(*r).to_index().0].clone();
                if content == MachineValue::Undefined {
                    return Err(CodegenError {
                        message: format!("emit_call_sysv: Undefined used_xmms content"),
                    });
                }
                self.machine.state.stack_values.push(content);
            }
        }

        let mut stack_offset: usize = 0;

        // Calculate stack offset.
        for (i, _param) in params.iter().enumerate() {
            let loc = Machine::get_param_location(1 + i);
            match loc {
                Location::Memory(_, _) => {
                    stack_offset += 8;
                }
                _ => {}
            }
        }

        // Align stack to 16 bytes.
        if (self.machine.get_stack_offset() + used_gprs.len() * 8 + used_xmms.len() * 8 + stack_offset) % 16
            != 0
        {
            self.assembler.emit_sub(Size::S64, Location::Imm32(8), Location::GPR(GPR::RSP));
            stack_offset += 8;
            self.machine.state.stack_values.push(MachineValue::Undefined);
        }

        let mut call_movs: Vec<(Location, GPR)> = vec![];

        // Prepare register & stack parameters.
        for (i, param) in params.iter().enumerate().rev() {
            let loc = Machine::get_param_location(1 + i);
            match loc {
                Location::GPR(x) => {
                    call_movs.push((*param, x));
                }
                Location::Memory(_, _) => {
                    match *param {
                        Location::GPR(x) => {
                            let content =
                                self.machine.state.register_values[X64Register::GPR(x).to_index().0].clone();
                            // FIXME: There might be some corner cases (release -> emit_call_sysv -> acquire?) that cause this assertion to fail.
                            // Hopefully nothing would be incorrect at runtime.

                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::XMM(x) => {
                            let content =
                                self.machine.state.register_values[X64Register::XMM(x).to_index().0].clone();
                            //assert!(content != MachineValue::Undefined);
                            self.machine.state.stack_values.push(content);
                        }
                        Location::Memory(reg, offset) => {
                            if reg != GPR::RBP {
                                return Err(CodegenError {
                                    message: format!("emit_call_sysv loc param: unreachable code"),
                                });
                            }
                            self.machine.state
                                .stack_values
                                .push(MachineValue::CopyStackBPRelative(offset));
                            // TODO: Read value at this offset
                        }
                        _ => {
                            self.machine.state.stack_values.push(MachineValue::Undefined);
                        }
                    }
                    match *param {
                        Location::Imm64(_) => {
                            // Dummy value slot to be filled with `mov`.
                            self.assembler.emit_push(Size::S64, Location::GPR(GPR::RAX));

                            // Use R10 as the temporary register here, since it is callee-saved and not
                            // used by the callback `cb`.
                            self.assembler.emit_mov(Size::S64, *param, Location::GPR(GPR::R10));
                            self.assembler.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::R10),
                                Location::Memory(GPR::RSP, 0),
                            );
                        }
                        Location::XMM(_) => {
                            // Dummy value slot to be filled with `mov`.
                            self.assembler.emit_push(Size::S64, Location::GPR(GPR::RAX));

                            // XMM registers can be directly stored to memory.
                            self.assembler.emit_mov(Size::S64, *param, Location::Memory(GPR::RSP, 0));
                        }
                        _ => self.assembler.emit_push(Size::S64, *param),
                    }
                }
                _ => {
                    return Err(CodegenError {
                        message: format!("emit_call_sysv loc: unreachable code"),
                    })
                }
            }
        }

        // Sort register moves so that register are not overwritten before read.
        sort_call_movs(&mut call_movs);

        // Emit register moves.
        for (loc, gpr) in call_movs {
            if loc != Location::GPR(gpr) {
                self.assembler.emit_mov(Size::S64, loc, Location::GPR(gpr));
            }
        }

        // Put vmctx as the first parameter.
        self.assembler.emit_mov(
            Size::S64,
            Location::GPR(Machine::get_vmctx_reg()),
            Machine::get_param_location(0),
        ); // vmctx

        if (self.machine.state.stack_values.len() % 2) != 1 {
            return Err(CodegenError {
                message: format!("emit_call_sysv: explicit shadow takes one slot"),
            });
        }

        cb(self);

        // Offset needs to be after the 'call' instruction.
        if let Some((fsm, control_stack)) = state_context {
            let state_diff_id = self.get_state_diff();
            let offset = self.assembler.get_offset().0;
            fsm.call_offsets.insert(
                offset,
                OffsetInfo {
                    end_offset: offset + 1,
                    activate_offset: offset,
                    diff_id: state_diff_id,
                },
            );
            fsm.wasm_offset_to_target_offset
                .insert(self.machine.state.wasm_inst_offset, SuspendOffset::Call(offset));
        }

        // Restore stack.
        if stack_offset > 0 {
            self.assembler.emit_add(
                Size::S64,
                Location::Imm32(stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
            if (stack_offset % 8) != 0 {
                return Err(CodegenError {
                    message: format!("emit_call_sysv: Bad restoring stack alignement"),
                });
            }
            for _ in 0..stack_offset / 8 {
                self.machine.state.stack_values.pop().unwrap();
            }
        }

        // Restore XMMs.
        if used_xmms.len() > 0 {
            for (i, r) in used_xmms.iter().enumerate() {
                self.assembler.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RSP, (i * 8) as i32),
                    Location::XMM(*r),
                );
            }
            self.assembler.emit_add(
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
            self.assembler.emit_pop(Size::S64, Location::GPR(*r));
            self.machine.state.stack_values.pop().unwrap();
        }

        if self.machine.state.stack_values.pop().unwrap() != MachineValue::ExplicitShadow {
            return Err(CodegenError {
                message: format!("emit_call_sysv: Popped value is not ExplicitShadow"),
            });
        }
        Ok(())
    }

    /// Emits a System V call sequence, specialized for labels as the call target.
    fn emit_call_sysv_label<I: Iterator<Item = Location>>(
        &mut self,
        label: DynamicLabel,
        params: I,
        state_context: Option<(&mut FunctionStateMap, &mut [ControlFrame])>,
    ) -> Result<(), CodegenError> {
        self.emit_call_sysv(|this| this.assembler.emit_call_label(label), params, state_context)?;
        Ok(())
    }

    /// Emits a memory operation.
    fn emit_memory_op<F: FnOnce(&mut Self, GPR) -> Result<(), CodegenError>>(
        &mut self,
        addr: Location,
        memarg: &MemoryImmediate,
        check_alignment: bool,
        value_size: usize,
        cb: F,
    ) -> Result<(), CodegenError> {
        let need_check = match self.memory_plans[MemoryIndex::new(0)].style {
            MemoryStyle::Dynamic => true,
            MemoryStyle::Static { .. } => false,
        };

        let tmp_addr = self.machine.acquire_temp_gpr().unwrap();
        let tmp_base = self.machine.acquire_temp_gpr().unwrap();

        // Load base into temporary register.
        self.assembler.emit_mov(
            Size::S64,
            Location::Memory(
                Machine::get_vmctx_reg(),
                0 // !!! FIXME: vm::Ctx::offset_memory_base() as i32,
            ),
            Location::GPR(tmp_base),
        );

        if need_check {
            let tmp_bound = self.machine.acquire_temp_gpr().unwrap();

            self.assembler.emit_mov(
                Size::S64,
                Location::Memory(
                    Machine::get_vmctx_reg(),
                    0 // !!! FIXME: vm::Ctx::offset_memory_bound() as i32,
                ),
                Location::GPR(tmp_bound),
            );
            // Adds base to bound so `tmp_bound` now holds the end of linear memory.
            self.assembler.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_bound));
            self.assembler.emit_mov(Size::S32, addr, Location::GPR(tmp_addr));

            // This branch is used for emitting "faster" code for the special case of (offset + value_size) not exceeding u32 range.
            match (memarg.offset as u32).checked_add(value_size as u32) {
                Some(0) => {}
                Some(x) => {
                    self.assembler.emit_add(Size::S64, Location::Imm32(x), Location::GPR(tmp_addr));
                }
                None => {
                    self.assembler.emit_add(
                        Size::S64,
                        Location::Imm32(memarg.offset as u32),
                        Location::GPR(tmp_addr),
                    );
                    self.assembler.emit_add(
                        Size::S64,
                        Location::Imm32(value_size as u32),
                        Location::GPR(tmp_addr),
                    );
                }
            }

            // Trap if the end address of the requested area is above that of the linear memory.
            self.assembler.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_addr));
            self.assembler.emit_cmp(Size::S64, Location::GPR(tmp_bound), Location::GPR(tmp_addr));

            self.mark_range_with_exception_code(
                ExceptionCode::MemoryOutOfBounds,
                |this| this.assembler.emit_conditional_trap(Condition::Above),
            );

            self.machine.release_temp_gpr(tmp_bound);
        }

        // Calculates the real address, and loads from it.
        self.assembler.emit_mov(Size::S32, addr, Location::GPR(tmp_addr));
        if memarg.offset != 0 {
            self.assembler.emit_add(
                Size::S64,
                Location::Imm32(memarg.offset as u32),
                Location::GPR(tmp_addr),
            );
        }
        self.assembler.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_addr));
        self.machine.release_temp_gpr(tmp_base);

        let align = match memarg.flags & 3 {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            _ => {
                return Err(CodegenError {
                    message: format!("emit_memory_op align: unreachable value"),
                })
            }
        };
        if check_alignment && align != 1 {
            let tmp_aligncheck = self.machine.acquire_temp_gpr().unwrap();
            self.assembler.emit_mov(
                Size::S32,
                Location::GPR(tmp_addr),
                Location::GPR(tmp_aligncheck),
            );
            self.assembler.emit_and(
                Size::S64,
                Location::Imm32(align - 1),
                Location::GPR(tmp_aligncheck),
            );
            self.mark_range_with_exception_code(
                ExceptionCode::MemoryOutOfBounds,
                |this| this.assembler.emit_conditional_trap(Condition::NotEqual),
            );
            self.machine.release_temp_gpr(tmp_aligncheck);
        }

        self.mark_range_with_exception_code(ExceptionCode::MemoryOutOfBounds, |this| {
            cb(this, tmp_addr)
        })?;

        self.machine.release_temp_gpr(tmp_addr);
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
                message: format!("emit_compare_and_swap: memory size > stac size"),
            });
        }

        let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
        let value = if loc == Location::GPR(GPR::R14) {
            GPR::R13
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value));

        self.assembler.emit_mov(stack_sz, loc, Location::GPR(value));

        let retry = self.assembler.get_label();
        self.assembler.emit_label(retry);

        self.emit_memory_op(
            target,
            memarg,
            true,
            value_size,
            |this, addr| {
                // Memory moves with size < 32b do not zero upper bits.
                if memory_sz < Size::S32 {
                    this.assembler.emit_xor(Size::S32, Location::GPR(compare), Location::GPR(compare));
                }
                this.assembler.emit_mov(memory_sz, Location::Memory(addr, 0), Location::GPR(compare));
                this.assembler.emit_mov(stack_sz, Location::GPR(compare), ret);
                cb(this, compare, value);
                this.assembler.emit_lock_cmpxchg(memory_sz, Location::GPR(value), Location::Memory(addr, 0));
                Ok(())
            },
        )?;

        self.assembler.emit_jmp(Condition::NotEqual, retry);

        self.assembler.emit_pop(Size::S64, Location::GPR(value));
        self.machine.release_temp_gpr(compare);
        Ok(())
    }

    // Checks for underflow/overflow/nan.
    fn emit_f32_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_label: <Assembler as Emitter>::Label,
        overflow_label: <Assembler as Emitter>::Label,
        nan_label: <Assembler as Emitter>::Label,
        succeed_label: <Assembler as Emitter>::Label,
    ) {
        let lower_bound = f32::to_bits(lower_bound);
        let upper_bound = f32::to_bits(upper_bound);

        let tmp = self.machine.acquire_temp_gpr().unwrap();
        let tmp_x = self.machine.acquire_temp_xmm().unwrap();

        // Underflow.
        self.assembler.emit_mov(Size::S32, Location::Imm32(lower_bound), Location::GPR(tmp));
        self.assembler.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x));
        self.assembler.emit_vcmpless(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        self.assembler.emit_mov(Size::S32, Location::Imm32(upper_bound), Location::GPR(tmp));
        self.assembler.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x));
        self.assembler.emit_vcmpgess(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        self.assembler.emit_vcmpeqss(reg, XMMOrMemory::XMM(reg), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::Equal, nan_label);

        self.assembler.emit_jmp(Condition::None, succeed_label);

        self.machine.release_temp_xmm(tmp_x);
        self.machine.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F32.
    fn emit_f32_int_conv_check_trap(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
    ) {
        let trap = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f32_int_conv_check(reg, lower_bound, upper_bound, trap, trap, trap, end);
        self.assembler.emit_label(trap);
        self.exception_table
            .offset_to_code
            .insert(self.assembler.get_offset().0, ExceptionCode::IllegalArithmetic);
        self.assembler.emit_ud2();
        self.assembler.emit_label(end);
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

        let underflow = self.assembler.get_label();
        let overflow = self.assembler.get_label();
        let nan = if nan_cb.is_some() {
            self.assembler.get_label()
        } else {
            underflow
        };
        let convert = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f32_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        self.assembler.emit_label(underflow);
        underflow_cb(self);
        self.assembler.emit_jmp(Condition::None, end);

        self.assembler.emit_label(overflow);
        overflow_cb(self);
        self.assembler.emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            self.assembler.emit_label(nan);
            cb(self);
            self.assembler.emit_jmp(Condition::None, end);
        }

        self.assembler.emit_label(convert);
        convert_cb(self);
        self.assembler.emit_label(end);
    }

    // Checks for underflow/overflow/nan.
    fn emit_f64_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_label: <Assembler as Emitter>::Label,
        overflow_label: <Assembler as Emitter>::Label,
        nan_label: <Assembler as Emitter>::Label,
        succeed_label: <Assembler as Emitter>::Label,
    ) {
        let lower_bound = f64::to_bits(lower_bound);
        let upper_bound = f64::to_bits(upper_bound);

        let tmp = self.machine.acquire_temp_gpr().unwrap();
        let tmp_x = self.machine.acquire_temp_xmm().unwrap();

        // Underflow.
        self.assembler.emit_mov(Size::S64, Location::Imm64(lower_bound), Location::GPR(tmp));
        self.assembler.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x));
        self.assembler.emit_vcmplesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        self.assembler.emit_mov(Size::S64, Location::Imm64(upper_bound), Location::GPR(tmp));
        self.assembler.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x));
        self.assembler.emit_vcmpgesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        self.assembler.emit_vcmpeqsd(reg, XMMOrMemory::XMM(reg), tmp_x);
        self.assembler.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        self.assembler.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        self.assembler.emit_jmp(Condition::Equal, nan_label);

        self.assembler.emit_jmp(Condition::None, succeed_label);

        self.machine.release_temp_xmm(tmp_x);
        self.machine.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F64.
    fn emit_f64_int_conv_check_trap(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
    ) {
        let trap = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f64_int_conv_check(reg, lower_bound, upper_bound, trap, trap, trap, end);
        self.assembler.emit_label(trap);
        self.exception_table
            .offset_to_code
            .insert(self.assembler.get_offset().0, ExceptionCode::IllegalArithmetic);
        self.assembler.emit_ud2();
        self.assembler.emit_label(end);
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

        let underflow = self.assembler.get_label();
        let overflow = self.assembler.get_label();
        let nan = if nan_cb.is_some() {
            self.assembler.get_label()
        } else {
            underflow
        };
        let convert = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f64_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        self.assembler.emit_label(underflow);
        underflow_cb(self);
        self.assembler.emit_jmp(Condition::None, end);

        self.assembler.emit_label(overflow);
        overflow_cb(self);
        self.assembler.emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            self.assembler.emit_label(nan);
            cb(self);
            self.assembler.emit_jmp(Condition::None, end);
        }

        self.assembler.emit_label(convert);
        convert_cb(self);
        self.assembler.emit_label(end);
    }

    pub fn get_state_diff(
        &mut self
    ) -> usize {
        if !self.machine.track_state {
            return usize::MAX;
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
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
        Type::AnyRef => WpType::AnyRef,
        Type::FuncRef => WpType::AnyFunc, // TODO: AnyFunc or Func?
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
