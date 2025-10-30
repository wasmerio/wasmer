#[cfg(feature = "unwind")]
use crate::unwind_winx64::create_unwind_info_from_insts;
use crate::{
    codegen_error,
    common_decl::*,
    emitter_x64::*,
    location::{Location as AbstractLocation, Reg},
    machine::*,
    unwind::{UnwindInstructions, UnwindOps, UnwindRegister},
    x64_decl::{ArgumentRegisterAllocator, GPR, X64Register, XMM},
};
use dynasmrt::{DynasmError, VecAssembler, x64::X64Relocation};
#[cfg(feature = "unwind")]
use gimli::{X86_64, write::CallFrameInstruction};
use std::ops::{Deref, DerefMut};
use wasmer_compiler::{
    types::{
        address_map::InstructionAddressMap,
        function::FunctionBody,
        relocation::{Relocation, RelocationKind, RelocationTarget},
        section::{CustomSection, CustomSectionProtection, SectionBody},
    },
    wasmparser::{MemArg, ValType as WpType},
};
use wasmer_types::{
    CompileError, FunctionIndex, FunctionType, SourceLoc, TrapCode, TrapInformation, Type,
    VMOffsets,
    target::{CallingConvention, CpuFeature, Target},
};

type Assembler = VecAssembler<X64Relocation>;

pub struct AssemblerX64 {
    /// the actual inner
    pub inner: Assembler,
    /// the simd instructions set on the target.
    /// Currently only supports SSE 4.2 and AVX
    pub simd_arch: CpuFeature,
    /// Full Target cpu
    pub target: Option<Target>,
}

impl AssemblerX64 {
    fn new(baseaddr: usize, target: Option<Target>) -> Result<Self, CompileError> {
        let simd_arch = target.as_ref().map_or_else(
            || Ok(CpuFeature::SSE42),
            |target| {
                if target.cpu_features().contains(CpuFeature::AVX) {
                    Ok(CpuFeature::AVX)
                } else if target.cpu_features().contains(CpuFeature::SSE42) {
                    Ok(CpuFeature::SSE42)
                } else {
                    Err(CompileError::UnsupportedTarget(
                        "x86_64 without AVX or SSE 4.2, use -m avx to enable".to_string(),
                    ))
                }
            },
        )?;

        Ok(Self {
            inner: Assembler::new(baseaddr),
            simd_arch,
            target,
        })
    }

    fn finalize(self) -> Result<Vec<u8>, DynasmError> {
        self.inner.finalize()
    }
}

impl Deref for AssemblerX64 {
    type Target = Assembler;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for AssemblerX64 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

type Location = AbstractLocation<GPR, XMM>;

pub struct MachineX86_64 {
    assembler: AssemblerX64,
    used_gprs: u32,
    used_simd: u32,
    trap_table: TrapTable,
    /// Map from byte offset into wasm function to range of native instructions.
    ///
    // Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,
    /// The source location for the current operator.
    src_loc: u32,
    /// Vector of unwind operations with offset
    unwind_ops: Vec<(usize, UnwindOps<GPR, XMM>)>,
}

impl MachineX86_64 {
    pub fn new(target: Option<Target>) -> Result<Self, CompileError> {
        let assembler = AssemblerX64::new(0, target)?;
        Ok(MachineX86_64 {
            assembler,
            used_gprs: 0,
            used_simd: 0,
            trap_table: TrapTable::default(),
            instructions_address_map: vec![],
            src_loc: 0,
            unwind_ops: vec![],
        })
    }
    pub fn emit_relaxed_binop(
        &mut self,
        op: fn(&mut AssemblerX64, Size, Location, Location) -> Result<(), CompileError>,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        enum RelaxMode {
            Direct,
            SrcToGPR,
            DstToGPR,
            BothToGPR,
        }
        let mode = match (src, dst) {
            (Location::GPR(_), Location::GPR(_))
                if std::ptr::eq(op as *const u8, AssemblerX64::emit_imul as *const u8) =>
            {
                RelaxMode::Direct
            }
            _ if std::ptr::eq(op as *const u8, AssemblerX64::emit_imul as *const u8) => {
                RelaxMode::BothToGPR
            }

            (Location::Memory(_, _), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::Imm64(_)) | (Location::Imm64(_), Location::Imm32(_)) => {
                RelaxMode::BothToGPR
            }
            (_, Location::Imm32(_)) | (_, Location::Imm64(_)) => RelaxMode::DstToGPR,
            (Location::Imm64(_), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::GPR(_))
                if (op as *const u8 != AssemblerX64::emit_mov as *const u8) =>
            {
                RelaxMode::SrcToGPR
            }
            (_, Location::SIMD(_)) => RelaxMode::SrcToGPR,
            _ => RelaxMode::Direct,
        };

        match mode {
            RelaxMode::SrcToGPR => {
                let temp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(sz, src, Location::GPR(temp))?;
                op(&mut self.assembler, sz, Location::GPR(temp), dst)?;
                self.release_gpr(temp);
            }
            RelaxMode::DstToGPR => {
                let temp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(sz, dst, Location::GPR(temp))?;
                op(&mut self.assembler, sz, src, Location::GPR(temp))?;
                self.release_gpr(temp);
            }
            RelaxMode::BothToGPR => {
                let temp_src = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                let temp_dst = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(sz, src, Location::GPR(temp_src))?;
                self.move_location(sz, dst, Location::GPR(temp_dst))?;
                op(
                    &mut self.assembler,
                    sz,
                    Location::GPR(temp_src),
                    Location::GPR(temp_dst),
                )?;
                match dst {
                    Location::Memory(_, _) | Location::GPR(_) => {
                        self.move_location(sz, Location::GPR(temp_dst), dst)?;
                    }
                    _ => {}
                }
                self.release_gpr(temp_dst);
                self.release_gpr(temp_src);
            }
            RelaxMode::Direct => {
                op(&mut self.assembler, sz, src, dst)?;
            }
        }
        Ok(())
    }
    pub fn emit_relaxed_zx_sx(
        &mut self,
        op: fn(&mut AssemblerX64, Size, Location, Size, Location) -> Result<(), CompileError>,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        match src {
            Location::Imm32(_) | Location::Imm64(_) => {
                let tmp_src = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.assembler
                    .emit_mov(Size::S64, src, Location::GPR(tmp_src))?;
                let src = Location::GPR(tmp_src);

                match dst {
                    Location::Imm32(_) | Location::Imm64(_) => unreachable!(),
                    Location::Memory(_, _) => {
                        let tmp_dst = self.acquire_temp_gpr().ok_or_else(|| {
                            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                        })?;
                        op(
                            &mut self.assembler,
                            sz_src,
                            src,
                            sz_dst,
                            Location::GPR(tmp_dst),
                        )?;
                        self.move_location(Size::S64, Location::GPR(tmp_dst), dst)?;

                        self.release_gpr(tmp_dst);
                    }
                    Location::GPR(_) => {
                        op(&mut self.assembler, sz_src, src, sz_dst, dst)?;
                    }
                    _ => {
                        codegen_error!("singlepass emit_relaxed_zx_sx unreachable");
                    }
                };

                self.release_gpr(tmp_src);
            }
            Location::GPR(_) | Location::Memory(_, _) => {
                match dst {
                    Location::Imm32(_) | Location::Imm64(_) => unreachable!(),
                    Location::Memory(_, _) => {
                        let tmp_dst = self.acquire_temp_gpr().ok_or_else(|| {
                            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                        })?;
                        op(
                            &mut self.assembler,
                            sz_src,
                            src,
                            sz_dst,
                            Location::GPR(tmp_dst),
                        )?;
                        self.move_location(Size::S64, Location::GPR(tmp_dst), dst)?;

                        self.release_gpr(tmp_dst);
                    }
                    Location::GPR(_) => {
                        op(&mut self.assembler, sz_src, src, sz_dst, dst)?;
                    }
                    _ => {
                        codegen_error!("singlepass emit_relaxed_zx_sx unreachable");
                    }
                };
            }
            _ => {
                codegen_error!("singlepass emit_relaxed_zx_sx unreachable");
            }
        }
        Ok(())
    }
    /// I32 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i32(
        &mut self,
        f: fn(&mut AssemblerX64, Size, Location, Location) -> Result<(), CompileError>,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if loc_a != ret {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc_a, Location::GPR(tmp))?;
            self.emit_relaxed_binop(f, Size::S32, loc_b, Location::GPR(tmp))?;
            self.emit_relaxed_mov(Size::S32, Location::GPR(tmp), ret)?;
            self.release_gpr(tmp);
        } else {
            self.emit_relaxed_binop(f, Size::S32, loc_b, ret)?;
        }
        Ok(())
    }
    /// I64 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i64(
        &mut self,
        f: fn(&mut AssemblerX64, Size, Location, Location) -> Result<(), CompileError>,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if loc_a != ret {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc_a, Location::GPR(tmp))?;
            self.emit_relaxed_binop(f, Size::S64, loc_b, Location::GPR(tmp))?;
            self.emit_relaxed_mov(Size::S64, Location::GPR(tmp), ret)?;
            self.release_gpr(tmp);
        } else {
            self.emit_relaxed_binop(f, Size::S64, loc_b, ret)?;
        }
        Ok(())
    }
    /// I64 comparison with.
    fn emit_cmpop_i64_dynamic_b(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        match ret {
            Location::GPR(x) => {
                self.emit_relaxed_cmp(Size::S64, loc_b, loc_a)?;
                self.assembler.emit_set(c, x)?;
                self.assembler
                    .emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x))?;
            }
            Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.emit_relaxed_cmp(Size::S64, loc_b, loc_a)?;
                self.assembler.emit_set(c, tmp)?;
                self.assembler
                    .emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp))?;
                self.move_location(Size::S32, Location::GPR(tmp), ret)?;
                self.release_gpr(tmp);
            }
            _ => {
                codegen_error!("singlepass emit_cmpop_i64_dynamic_b unreachable");
            }
        }
        Ok(())
    }
    /// I64 shift with both operands popped from the virtual stack.
    fn emit_shift_i64(
        &mut self,
        f: fn(&mut AssemblerX64, Size, Location, Location) -> Result<(), CompileError>,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S64, loc_b, Location::GPR(GPR::RCX))?;

        if loc_a != ret {
            self.emit_relaxed_mov(Size::S64, loc_a, ret)?;
        }

        f(&mut self.assembler, Size::S64, Location::GPR(GPR::RCX), ret)
    }
    /// Moves `loc` to a valid location for `div`/`idiv`.
    fn emit_relaxed_xdiv(
        &mut self,
        op: fn(&mut AssemblerX64, Size, Location) -> Result<(), CompileError>,
        sz: Size,
        loc: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        self.assembler.emit_cmp(sz, Location::Imm32(0), loc)?;
        self.assembler
            .emit_jmp(Condition::Equal, integer_division_by_zero)?;

        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                self.move_location(sz, loc, Location::GPR(GPR::RCX))?; // must not be used during div (rax, rdx)
                let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
                op(&mut self.assembler, sz, Location::GPR(GPR::RCX))?;
                self.mark_instruction_address_end(offset);
                Ok(offset)
            }
            _ => {
                let offset = self.mark_instruction_with_trap_code(TrapCode::IntegerOverflow);
                op(&mut self.assembler, sz, loc)?;
                self.mark_instruction_address_end(offset);
                Ok(offset)
            }
        }
    }
    /// I32 comparison with.
    fn emit_cmpop_i32_dynamic_b(
        &mut self,
        c: Condition,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        match ret {
            Location::GPR(x) => {
                self.emit_relaxed_cmp(Size::S32, loc_b, loc_a)?;
                self.assembler.emit_set(c, x)?;
                self.assembler
                    .emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x))?;
            }
            Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.emit_relaxed_cmp(Size::S32, loc_b, loc_a)?;
                self.assembler.emit_set(c, tmp)?;
                self.assembler
                    .emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp))?;
                self.move_location(Size::S32, Location::GPR(tmp), ret)?;
                self.release_gpr(tmp);
            }
            _ => {
                codegen_error!("singlepass emit_cmpop_i32_dynamic_b unreachable");
            }
        }
        Ok(())
    }
    /// I32 shift with both operands popped from the virtual stack.
    fn emit_shift_i32(
        &mut self,
        f: fn(&mut AssemblerX64, Size, Location, Location) -> Result<(), CompileError>,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.assembler
            .emit_mov(Size::S32, loc_b, Location::GPR(GPR::RCX))?;

        if loc_a != ret {
            self.emit_relaxed_mov(Size::S32, loc_a, ret)?;
        }

        f(&mut self.assembler, Size::S32, Location::GPR(GPR::RCX), ret)
    }

    #[allow(clippy::too_many_arguments)]
    fn memory_op<F: FnOnce(&mut Self, GPR) -> Result<(), CompileError>>(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        check_alignment: bool,
        value_size: usize,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
        cb: F,
    ) -> Result<(), CompileError> {
        // This function as been re-writen to use only 2 temporary register instead of 3
        // without compromisong on the perfomances.
        // The number of memory move should be equivalent to previous 3-temp regs version
        // Register pressure is high on x86_64, and this is needed to be able to use
        // instruction that neead RAX, like cmpxchg for example
        let tmp_addr = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp2 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        // Reusing `tmp_addr` for temporary indirection here, since it's not used before the last reference to `{base,bound}_loc`.
        let base_loc = if imported_memories {
            // Imported memories require one level of indirection.
            self.emit_relaxed_binop(
                AssemblerX64::emit_mov,
                Size::S64,
                Location::Memory(self.get_vmctx_reg(), offset),
                Location::GPR(tmp2),
            )?;
            Location::Memory(tmp2, 0)
        } else {
            Location::Memory(self.get_vmctx_reg(), offset)
        };

        // Load base into temporary register.
        self.assembler
            .emit_mov(Size::S64, base_loc, Location::GPR(tmp2))?;

        // Load effective address.
        // `base_loc` and `bound_loc` becomes INVALID after this line, because `tmp_addr`
        // might be reused.
        self.assembler
            .emit_mov(Size::S32, addr, Location::GPR(tmp_addr))?;

        // Add offset to memory address.
        if memarg.offset != 0 {
            self.assembler.emit_add(
                Size::S32,
                Location::Imm32(memarg.offset as u32),
                Location::GPR(tmp_addr),
            )?;

            // Trap if offset calculation overflowed.
            self.assembler.emit_jmp(Condition::Carry, heap_access_oob)?;
        }

        if need_check {
            let bound_loc = if imported_memories {
                // Imported memories require one level of indirection.
                self.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S64,
                    Location::Memory(self.get_vmctx_reg(), offset),
                    Location::GPR(tmp2),
                )?;
                Location::Memory(tmp2, 8)
            } else {
                Location::Memory(self.get_vmctx_reg(), offset + 8)
            };
            self.assembler
                .emit_mov(Size::S64, bound_loc, Location::GPR(tmp2))?;

            // We will compare the upper bound limit without having add the "temp_base" value, as it's a constant
            self.assembler.emit_lea(
                Size::S64,
                Location::Memory(tmp2, -(value_size as i32)),
                Location::GPR(tmp2),
            )?;
            // Trap if the end address of the requested area is above that of the linear memory.
            self.assembler
                .emit_cmp(Size::S64, Location::GPR(tmp2), Location::GPR(tmp_addr))?;

            // `tmp_bound` is inclusive. So trap only if `tmp_addr > tmp_bound`.
            self.assembler.emit_jmp(Condition::Above, heap_access_oob)?;
        }
        // get back baseloc, as it might have been destroid with the upper memory test
        let base_loc = if imported_memories {
            // Imported memories require one level of indirection.
            self.emit_relaxed_binop(
                AssemblerX64::emit_mov,
                Size::S64,
                Location::Memory(self.get_vmctx_reg(), offset),
                Location::GPR(tmp2),
            )?;
            Location::Memory(tmp2, 0)
        } else {
            Location::Memory(self.get_vmctx_reg(), offset)
        };
        // Wasm linear memory -> real memory
        self.assembler
            .emit_add(Size::S64, base_loc, Location::GPR(tmp_addr))?;

        self.release_gpr(tmp2);

        let align = value_size as u32;
        if check_alignment && align != 1 {
            let tmp_aligncheck = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.assembler.emit_mov(
                Size::S32,
                Location::GPR(tmp_addr),
                Location::GPR(tmp_aligncheck),
            )?;
            self.assembler.emit_and(
                Size::S64,
                Location::Imm32(align - 1),
                Location::GPR(tmp_aligncheck),
            )?;
            self.assembler
                .emit_jmp(Condition::NotEqual, unaligned_atomic)?;
            self.release_gpr(tmp_aligncheck);
        }
        let begin = self.assembler.get_offset().0;
        cb(self, tmp_addr)?;
        let end = self.assembler.get_offset().0;
        self.mark_address_range_with_trap_code(TrapCode::HeapAccessOutOfBounds, begin, end);

        self.release_gpr(tmp_addr);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_compare_and_swap<F: FnOnce(&mut Self, GPR, GPR) -> Result<(), CompileError>>(
        &mut self,
        loc: Location,
        target: Location,
        ret: Location,
        memarg: &MemArg,
        value_size: usize,
        memory_sz: Size,
        stack_sz: Size,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
        cb: F,
    ) -> Result<(), CompileError> {
        if memory_sz > stack_sz {
            codegen_error!("singlepass emit_compare_and_swap unreachable");
        }

        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if loc == Location::GPR(GPR::R14) {
            GPR::R13
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;

        self.move_location(stack_sz, loc, Location::GPR(value))?;

        let retry = self.assembler.get_label();
        self.emit_label(retry)?;

        self.memory_op(
            target,
            memarg,
            true,
            value_size,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.load_address(memory_sz, Location::GPR(compare), Location::Memory(addr, 0))?;
                this.move_location(stack_sz, Location::GPR(compare), ret)?;
                cb(this, compare, value)?;
                this.assembler.emit_lock_cmpxchg(
                    memory_sz,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;

        self.assembler.emit_jmp(Condition::NotEqual, retry)?;

        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }

    // Checks for underflow/overflow/nan.
    #[allow(clippy::too_many_arguments)]
    fn emit_f32_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_label: Label,
        overflow_label: Label,
        nan_label: Label,
        succeed_label: Label,
    ) -> Result<(), CompileError> {
        let lower_bound = f32::to_bits(lower_bound);
        let upper_bound = f32::to_bits(upper_bound);

        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_x = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        // Underflow.
        self.move_location(Size::S32, Location::Imm32(lower_bound), Location::GPR(tmp))?;
        self.move_location(Size::S32, Location::GPR(tmp), Location::SIMD(tmp_x))?;
        self.assembler
            .emit_vcmpless(reg, XMMOrMemory::XMM(tmp_x), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler
            .emit_jmp(Condition::NotEqual, underflow_label)?;

        // Overflow.
        self.move_location(Size::S32, Location::Imm32(upper_bound), Location::GPR(tmp))?;
        self.move_location(Size::S32, Location::GPR(tmp), Location::SIMD(tmp_x))?;
        self.assembler
            .emit_vcmpgess(reg, XMMOrMemory::XMM(tmp_x), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler
            .emit_jmp(Condition::NotEqual, overflow_label)?;

        // NaN.
        self.assembler
            .emit_vcmpeqss(reg, XMMOrMemory::XMM(reg), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler.emit_jmp(Condition::Equal, nan_label)?;

        self.assembler.emit_jmp(Condition::None, succeed_label)?;

        self.release_simd(tmp_x);
        self.release_gpr(tmp);
        Ok(())
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F32.
    fn emit_f32_int_conv_check_trap(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<(), CompileError> {
        let trap_overflow = self.assembler.get_label();
        let trap_badconv = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f32_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            trap_overflow,
            trap_overflow,
            trap_badconv,
            end,
        )?;

        self.emit_label(trap_overflow)?;

        self.emit_illegal_op_internal(TrapCode::IntegerOverflow)?;

        self.emit_label(trap_badconv)?;

        self.emit_illegal_op_internal(TrapCode::BadConversionToInteger)?;

        self.emit_label(end)?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn emit_f32_int_conv_check_sat<
        F1: FnOnce(&mut Self) -> Result<(), CompileError>,
        F2: FnOnce(&mut Self) -> Result<(), CompileError>,
        F3: FnOnce(&mut Self) -> Result<(), CompileError>,
        F4: FnOnce(&mut Self) -> Result<(), CompileError>,
    >(
        &mut self,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) -> Result<(), CompileError> {
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
        )?;

        self.emit_label(underflow)?;
        underflow_cb(self)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        self.emit_label(overflow)?;
        overflow_cb(self)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        if let Some(cb) = nan_cb {
            self.emit_label(nan)?;
            cb(self)?;
            self.assembler.emit_jmp(Condition::None, end)?;
        }

        self.emit_label(convert)?;
        convert_cb(self)?;
        self.emit_label(end)
    }
    // Checks for underflow/overflow/nan.
    #[allow(clippy::too_many_arguments)]
    fn emit_f64_int_conv_check(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_label: Label,
        overflow_label: Label,
        nan_label: Label,
        succeed_label: Label,
    ) -> Result<(), CompileError> {
        let lower_bound = f64::to_bits(lower_bound);
        let upper_bound = f64::to_bits(upper_bound);

        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_x = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        // Underflow.
        self.move_location(Size::S64, Location::Imm64(lower_bound), Location::GPR(tmp))?;
        self.move_location(Size::S64, Location::GPR(tmp), Location::SIMD(tmp_x))?;
        self.assembler
            .emit_vcmplesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler
            .emit_jmp(Condition::NotEqual, underflow_label)?;

        // Overflow.
        self.move_location(Size::S64, Location::Imm64(upper_bound), Location::GPR(tmp))?;
        self.move_location(Size::S64, Location::GPR(tmp), Location::SIMD(tmp_x))?;
        self.assembler
            .emit_vcmpgesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler
            .emit_jmp(Condition::NotEqual, overflow_label)?;

        // NaN.
        self.assembler
            .emit_vcmpeqsd(reg, XMMOrMemory::XMM(reg), tmp_x)?;
        self.move_location(Size::S32, Location::SIMD(tmp_x), Location::GPR(tmp))?;
        self.assembler
            .emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp))?;
        self.assembler.emit_jmp(Condition::Equal, nan_label)?;

        self.assembler.emit_jmp(Condition::None, succeed_label)?;

        self.release_simd(tmp_x);
        self.release_gpr(tmp);
        Ok(())
    }
    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F64.. return offset/len for trap_overflow and trap_badconv
    fn emit_f64_int_conv_check_trap(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
    ) -> Result<(), CompileError> {
        let trap_overflow = self.assembler.get_label();
        let trap_badconv = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_f64_int_conv_check(
            reg,
            lower_bound,
            upper_bound,
            trap_overflow,
            trap_overflow,
            trap_badconv,
            end,
        )?;

        self.emit_label(trap_overflow)?;
        self.emit_illegal_op_internal(TrapCode::IntegerOverflow)?;

        self.emit_label(trap_badconv)?;
        self.emit_illegal_op_internal(TrapCode::BadConversionToInteger)?;

        self.emit_label(end)
    }
    #[allow(clippy::too_many_arguments)]
    fn emit_f64_int_conv_check_sat<
        F1: FnOnce(&mut Self) -> Result<(), CompileError>,
        F2: FnOnce(&mut Self) -> Result<(), CompileError>,
        F3: FnOnce(&mut Self) -> Result<(), CompileError>,
        F4: FnOnce(&mut Self) -> Result<(), CompileError>,
    >(
        &mut self,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) -> Result<(), CompileError> {
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
        )?;

        self.emit_label(underflow)?;
        underflow_cb(self)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        self.emit_label(overflow)?;
        overflow_cb(self)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        if let Some(cb) = nan_cb {
            self.emit_label(nan)?;
            cb(self)?;
            self.assembler.emit_jmp(Condition::None, end)?;
        }

        self.emit_label(convert)?;
        convert_cb(self)?;
        self.emit_label(end)
    }
    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx(
        &mut self,
        op: fn(&mut AssemblerX64, XMM, XMMOrMemory, XMM) -> Result<(), CompileError>,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx_base(
            |this, src1, src2, dst| op(&mut this.assembler, src1, src2, dst),
            src1,
            src2,
            dst,
        )
    }

    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx_base<
        F: FnOnce(&mut Self, XMM, XMMOrMemory, XMM) -> Result<(), CompileError>,
    >(
        &mut self,
        op: F,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp3 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmpg = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        let src1 = match src1 {
            Location::SIMD(x) => x,
            Location::GPR(_) | Location::Memory(_, _) => {
                self.assembler
                    .emit_mov(Size::S64, src1, Location::SIMD(tmp1))?;
                tmp1
            }
            Location::Imm32(_) => {
                self.assembler
                    .emit_mov(Size::S32, src1, Location::GPR(tmpg))?;
                self.move_location(Size::S32, Location::GPR(tmpg), Location::SIMD(tmp1))?;
                tmp1
            }
            Location::Imm64(_) => {
                self.assembler
                    .emit_mov(Size::S64, src1, Location::GPR(tmpg))?;
                self.move_location(Size::S64, Location::GPR(tmpg), Location::SIMD(tmp1))?;
                tmp1
            }
            _ => {
                codegen_error!("singlepass emit_relaxed_avx_base unreachable")
            }
        };

        let src2 = match src2 {
            Location::SIMD(x) => XMMOrMemory::XMM(x),
            Location::Memory(base, disp) => XMMOrMemory::Memory(base, disp),
            Location::GPR(_) => {
                self.assembler
                    .emit_mov(Size::S64, src2, Location::SIMD(tmp2))?;
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm32(_) => {
                self.assembler
                    .emit_mov(Size::S32, src2, Location::GPR(tmpg))?;
                self.move_location(Size::S32, Location::GPR(tmpg), Location::SIMD(tmp2))?;
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm64(_) => {
                self.assembler
                    .emit_mov(Size::S64, src2, Location::GPR(tmpg))?;
                self.move_location(Size::S64, Location::GPR(tmpg), Location::SIMD(tmp2))?;
                XMMOrMemory::XMM(tmp2)
            }
            _ => {
                codegen_error!("singlepass emit_relaxed_avx_base unreachable")
            }
        };

        match dst {
            Location::SIMD(x) => {
                op(self, src1, src2, x)?;
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                op(self, src1, src2, tmp3)?;
                self.assembler
                    .emit_mov(Size::S64, Location::SIMD(tmp3), dst)?;
            }
            _ => {
                codegen_error!("singlepass emit_relaxed_avx_base unreachable")
            }
        }

        self.release_gpr(tmpg);
        self.release_simd(tmp3);
        self.release_simd(tmp2);
        self.release_simd(tmp1);
        Ok(())
    }

    fn convert_i64_f64_u_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
        self.emit_f64_int_conv_check_sat(
            tmp_in,
            GEF64_LT_U64_MIN,
            LEF64_GT_U64_MAX,
            |this| {
                this.assembler
                    .emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out))
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(u64::MAX),
                    Location::GPR(tmp_out),
                )
            },
            None::<fn(this: &mut Self) -> Result<(), CompileError>>,
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i64_trunc_uf64(tmp_in, tmp_out)
                } else {
                    let tmp = this.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    let tmp_x1 = this.acquire_temp_simd().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
                    })?;
                    let tmp_x2 = this.acquire_temp_simd().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
                    })?;

                    this.assembler.emit_mov(
                        Size::S64,
                        Location::Imm64(4890909195324358656u64),
                        Location::GPR(tmp),
                    )?; //double 9.2233720368547758E+18
                    this.assembler.emit_mov(
                        Size::S64,
                        Location::GPR(tmp),
                        Location::SIMD(tmp_x1),
                    )?;
                    this.assembler.emit_mov(
                        Size::S64,
                        Location::SIMD(tmp_in),
                        Location::SIMD(tmp_x2),
                    )?;
                    this.assembler
                        .emit_vsubsd(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in)?;
                    this.assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
                    this.assembler.emit_mov(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    )?;
                    this.assembler.emit_xor(
                        Size::S64,
                        Location::GPR(tmp_out),
                        Location::GPR(tmp),
                    )?;
                    this.assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out)?;
                    this.assembler
                        .emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2)?;
                    this.assembler.emit_cmovae_gpr_64(tmp, tmp_out)?;

                    this.release_simd(tmp_x2);
                    this.release_simd(tmp_x1);
                    this.release_gpr(tmp);
                    Ok(())
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S64, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i64_f64_u_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i64_trunc_uf64(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm2

            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_U64_MIN, LEF64_GT_U64_MAX)?;

            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?; // r15
            let tmp_x1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm1
            let tmp_x2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm3

            self.move_location(
                Size::S64,
                Location::Imm64(4890909195324358656u64),
                Location::GPR(tmp),
            )?; //double 9.2233720368547758E+18
            self.move_location(Size::S64, Location::GPR(tmp), Location::SIMD(tmp_x1))?;
            self.move_location(Size::S64, Location::SIMD(tmp_in), Location::SIMD(tmp_x2))?;
            self.assembler
                .emit_vsubsd(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in)?;
            self.assembler
                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(
                Size::S64,
                Location::Imm64(0x8000000000000000u64),
                Location::GPR(tmp),
            )?;
            self.assembler
                .emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp))?;
            self.assembler
                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out)?;
            self.assembler
                .emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2)?;
            self.assembler.emit_cmovae_gpr_64(tmp, tmp_out)?;
            self.move_location(Size::S64, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_x2);
            self.release_simd(tmp_x1);
            self.release_gpr(tmp);
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i64_f64_s_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
        self.emit_f64_int_conv_check_sat(
            tmp_in,
            GEF64_LT_I64_MIN,
            LEF64_GT_I64_MAX,
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(i64::MIN as u64),
                    Location::GPR(tmp_out),
                )
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(i64::MAX as u64),
                    Location::GPR(tmp_out),
                )
            },
            Some(|this: &mut Self| {
                this.assembler
                    .emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out))
            }),
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i64_trunc_sf64(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S64, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i64_f64_s_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i64_trunc_sf64(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;

            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_I64_MIN, LEF64_GT_I64_MAX)?;

            self.assembler
                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(Size::S64, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i32_f64_s_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        let real_in = match loc {
            Location::Imm32(_) | Location::Imm64(_) => {
                self.move_location(Size::S64, loc, Location::GPR(tmp_out))?;
                self.move_location(Size::S64, Location::GPR(tmp_out), Location::SIMD(tmp_in))?;
                tmp_in
            }
            Location::SIMD(x) => x,
            _ => {
                self.move_location(Size::S64, loc, Location::SIMD(tmp_in))?;
                tmp_in
            }
        };

        self.emit_f64_int_conv_check_sat(
            real_in,
            GEF64_LT_I32_MIN,
            LEF64_GT_I32_MAX,
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(i32::MIN as u32),
                    Location::GPR(tmp_out),
                )
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(i32::MAX as u32),
                    Location::GPR(tmp_out),
                )
            },
            Some(|this: &mut Self| {
                this.assembler
                    .emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out))
            }),
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i32_trunc_sf64(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S32, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i32_f64_s_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i32_trunc_sf64(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;

            let real_in = match loc {
                Location::Imm32(_) | Location::Imm64(_) => {
                    self.move_location(Size::S64, loc, Location::GPR(tmp_out))?;
                    self.move_location(Size::S64, Location::GPR(tmp_out), Location::SIMD(tmp_in))?;
                    tmp_in
                }
                Location::SIMD(x) => x,
                _ => {
                    self.move_location(Size::S64, loc, Location::SIMD(tmp_in))?;
                    tmp_in
                }
            };

            self.emit_f64_int_conv_check_trap(real_in, GEF64_LT_I32_MIN, LEF64_GT_I32_MAX)?;

            self.assembler
                .emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out)?;
            self.move_location(Size::S32, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i32_f64_u_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
        self.emit_f64_int_conv_check_sat(
            tmp_in,
            GEF64_LT_U32_MIN,
            LEF64_GT_U32_MAX,
            |this| {
                this.assembler
                    .emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out))
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(u32::MAX),
                    Location::GPR(tmp_out),
                )
            },
            None::<fn(this: &mut Self) -> Result<(), CompileError>>,
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i32_trunc_uf64(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S32, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i32_f64_u_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i32_trunc_uf64(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;

            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp_in))?;
            self.emit_f64_int_conv_check_trap(tmp_in, GEF64_LT_U32_MIN, LEF64_GT_U32_MAX)?;

            self.assembler
                .emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(Size::S32, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i64_f32_u_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
        self.emit_f32_int_conv_check_sat(
            tmp_in,
            GEF32_LT_U64_MIN,
            LEF32_GT_U64_MAX,
            |this| {
                this.assembler
                    .emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out))
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(u64::MAX),
                    Location::GPR(tmp_out),
                )
            },
            None::<fn(this: &mut Self) -> Result<(), CompileError>>,
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i64_trunc_uf32(tmp_in, tmp_out)
                } else {
                    let tmp = this.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    let tmp_x1 = this.acquire_temp_simd().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
                    })?;
                    let tmp_x2 = this.acquire_temp_simd().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
                    })?;

                    this.assembler.emit_mov(
                        Size::S32,
                        Location::Imm32(1593835520u32),
                        Location::GPR(tmp),
                    )?; //float 9.22337203E+18
                    this.assembler.emit_mov(
                        Size::S32,
                        Location::GPR(tmp),
                        Location::SIMD(tmp_x1),
                    )?;
                    this.assembler.emit_mov(
                        Size::S32,
                        Location::SIMD(tmp_in),
                        Location::SIMD(tmp_x2),
                    )?;
                    this.assembler
                        .emit_vsubss(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in)?;
                    this.assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
                    this.assembler.emit_mov(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    )?;
                    this.assembler.emit_xor(
                        Size::S64,
                        Location::GPR(tmp_out),
                        Location::GPR(tmp),
                    )?;
                    this.assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out)?;
                    this.assembler
                        .emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2)?;
                    this.assembler.emit_cmovae_gpr_64(tmp, tmp_out)?;

                    this.release_simd(tmp_x2);
                    this.release_simd(tmp_x1);
                    this.release_gpr(tmp);
                    Ok(())
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S64, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i64_f32_u_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i64_trunc_uf32(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm2

            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_U64_MIN, LEF32_GT_U64_MAX)?;

            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?; // r15
            let tmp_x1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm1
            let tmp_x2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?; // xmm3

            self.move_location(
                Size::S32,
                Location::Imm32(1593835520u32),
                Location::GPR(tmp),
            )?; //float 9.22337203E+18
            self.move_location(Size::S32, Location::GPR(tmp), Location::SIMD(tmp_x1))?;
            self.move_location(Size::S32, Location::SIMD(tmp_in), Location::SIMD(tmp_x2))?;
            self.assembler
                .emit_vsubss(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in)?;
            self.assembler
                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(
                Size::S64,
                Location::Imm64(0x8000000000000000u64),
                Location::GPR(tmp),
            )?;
            self.assembler
                .emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp))?;
            self.assembler
                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out)?;
            self.assembler
                .emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2)?;
            self.assembler.emit_cmovae_gpr_64(tmp, tmp_out)?;
            self.move_location(Size::S64, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_x2);
            self.release_simd(tmp_x1);
            self.release_gpr(tmp);
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i64_f32_s_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
        self.emit_f32_int_conv_check_sat(
            tmp_in,
            GEF32_LT_I64_MIN,
            LEF32_GT_I64_MAX,
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(i64::MIN as u64),
                    Location::GPR(tmp_out),
                )
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S64,
                    Location::Imm64(i64::MAX as u64),
                    Location::GPR(tmp_out),
                )
            },
            Some(|this: &mut Self| {
                this.assembler
                    .emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out))
            }),
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i64_trunc_sf32(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S64, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i64_f32_s_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i64_trunc_sf32(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S64, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;

            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_I64_MIN, LEF32_GT_I64_MAX)?;
            self.assembler
                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(Size::S64, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i32_f32_s_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
        self.emit_f32_int_conv_check_sat(
            tmp_in,
            GEF32_LT_I32_MIN,
            LEF32_GT_I32_MAX,
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(i32::MIN as u32),
                    Location::GPR(tmp_out),
                )
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(i32::MAX as u32),
                    Location::GPR(tmp_out),
                )
            },
            Some(|this: &mut Self| {
                this.assembler
                    .emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out))
            }),
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i32_trunc_sf32(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S32, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i32_f32_s_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i32_trunc_sf32(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;

            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_I32_MIN, LEF32_GT_I32_MAX)?;

            self.assembler
                .emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(Size::S32, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }
    fn convert_i32_f32_u_s(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
        self.emit_f32_int_conv_check_sat(
            tmp_in,
            GEF32_LT_U32_MIN,
            LEF32_GT_U32_MAX,
            |this| {
                this.assembler
                    .emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out))
            },
            |this| {
                this.assembler.emit_mov(
                    Size::S32,
                    Location::Imm32(u32::MAX),
                    Location::GPR(tmp_out),
                )
            },
            None::<fn(this: &mut Self) -> Result<(), CompileError>>,
            |this| {
                if this.assembler.arch_has_itruncf() {
                    this.assembler.arch_emit_i32_trunc_uf32(tmp_in, tmp_out)
                } else {
                    this.assembler
                        .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)
                }
            },
        )?;

        self.assembler
            .emit_mov(Size::S32, Location::GPR(tmp_out), ret)?;
        self.release_simd(tmp_in);
        self.release_gpr(tmp_out);
        Ok(())
    }
    fn convert_i32_f32_u_u(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_itruncf() {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.assembler.arch_emit_i32_trunc_uf32(tmp_in, tmp_out)?;
            self.emit_relaxed_mov(Size::S32, Location::GPR(tmp_out), ret)?;
            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        } else {
            let tmp_out = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmp_in = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp_in))?;
            self.emit_f32_int_conv_check_trap(tmp_in, GEF32_LT_U32_MIN, LEF32_GT_U32_MAX)?;

            self.assembler
                .emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out)?;
            self.move_location(Size::S32, Location::GPR(tmp_out), ret)?;

            self.release_simd(tmp_in);
            self.release_gpr(tmp_out);
        }
        Ok(())
    }

    fn emit_relaxed_atomic_xchg(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop(AssemblerX64::emit_xchg, sz, src, dst)
    }

    fn used_gprs_contains(&self, r: &GPR) -> bool {
        self.used_gprs & (1 << r.into_index()) != 0
    }
    fn used_simd_contains(&self, r: &XMM) -> bool {
        self.used_simd & (1 << r.into_index()) != 0
    }
    fn used_gprs_insert(&mut self, r: GPR) {
        self.used_gprs |= 1 << r.into_index();
    }
    fn used_simd_insert(&mut self, r: XMM) {
        self.used_simd |= 1 << r.into_index();
    }
    fn used_gprs_remove(&mut self, r: &GPR) -> bool {
        let ret = self.used_gprs_contains(r);
        self.used_gprs &= !(1 << r.into_index());
        ret
    }
    fn used_simd_remove(&mut self, r: &XMM) -> bool {
        let ret = self.used_simd_contains(r);
        self.used_simd &= !(1 << r.into_index());
        ret
    }
    fn emit_unwind_op(&mut self, op: UnwindOps<GPR, XMM>) -> Result<(), CompileError> {
        self.unwind_ops.push((self.get_offset().0, op));
        Ok(())
    }
    fn emit_illegal_op_internal(&mut self, trap: TrapCode) -> Result<(), CompileError> {
        let v = trap as u8;
        self.assembler.emit_ud1_payload(v)
    }

    // logic
    fn location_xor(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        _flags: bool,
    ) -> Result<(), CompileError> {
        self.assembler.emit_xor(size, source, dest)
    }
    fn location_or(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        _flags: bool,
    ) -> Result<(), CompileError> {
        self.assembler.emit_or(size, source, dest)
    }
    fn load_address(
        &mut self,
        size: Size,
        reg: Location,
        mem: Location,
    ) -> Result<(), CompileError> {
        match reg {
            Location::GPR(_) => {
                match mem {
                    Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                        // Memory moves with size < 32b do not zero upper bits.
                        if size < Size::S32 {
                            self.assembler.emit_xor(Size::S32, reg, reg)?;
                        }
                        self.assembler.emit_mov(size, mem, reg)?;
                    }
                    _ => codegen_error!("singlepass load_address unreachable"),
                }
            }
            _ => codegen_error!("singlepass load_address unreachable"),
        }
        Ok(())
    }

    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        self.move_location_extend(size_val, signed, source, size_op, dest)?;
        self.assembler.emit_neg(size_val, dest)
    }

    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        if (sz_src == Size::S32 || sz_src == Size::S64) && sz_dst == Size::S64 {
            self.emit_relaxed_binop(AssemblerX64::emit_mov, sz_src, src, dst)
        } else {
            self.emit_relaxed_zx_sx(AssemblerX64::emit_movzx, sz_src, src, sz_dst, dst)
        }
    }
}

impl Machine for MachineX86_64 {
    type GPR = GPR;
    type SIMD = XMM;
    fn assembler_get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }

    fn get_vmctx_reg(&self) -> GPR {
        GPR::R15
    }

    fn get_used_gprs(&self) -> Vec<GPR> {
        GPR::iterator()
            .filter(|x| self.used_gprs & (1 << x.into_index()) != 0)
            .cloned()
            .collect()
    }

    fn get_used_simd(&self) -> Vec<XMM> {
        XMM::iterator()
            .filter(|x| self.used_simd & (1 << x.into_index()) != 0)
            .cloned()
            .collect()
    }

    fn pick_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[RSI, RDI, R8, R9, R10, R11];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused general purpose register for internal temporary use.
    fn pick_temp_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[RAX, RCX, RDX];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    fn acquire_temp_gpr(&mut self) -> Option<GPR> {
        let gpr = self.pick_temp_gpr();
        if let Some(x) = gpr {
            self.used_gprs_insert(x);
        }
        gpr
    }

    fn release_gpr(&mut self, gpr: GPR) {
        assert!(self.used_gprs_remove(&gpr));
    }

    fn reserve_unused_temp_gpr(&mut self, gpr: GPR) -> GPR {
        assert!(!self.used_gprs_contains(&gpr));
        self.used_gprs_insert(gpr);
        gpr
    }

    fn reserve_gpr(&mut self, gpr: GPR) {
        self.used_gprs_insert(gpr);
    }

    fn push_used_gpr(&mut self, used_gprs: &[GPR]) -> Result<usize, CompileError> {
        for r in used_gprs.iter() {
            self.assembler.emit_push(Size::S64, Location::GPR(*r))?;
        }
        Ok(used_gprs.len() * 8)
    }
    fn pop_used_gpr(&mut self, used_gprs: &[GPR]) -> Result<(), CompileError> {
        for r in used_gprs.iter().rev() {
            self.assembler.emit_pop(Size::S64, Location::GPR(*r))?;
        }
        Ok(())
    }

    // Picks an unused XMM register.
    fn pick_simd(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &[XMM] = &[XMM3, XMM4, XMM5, XMM6, XMM7];
        for r in REGS {
            if !self.used_simd_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused XMM register for internal temporary use.
    fn pick_temp_simd(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &[XMM] = &[XMM0, XMM1, XMM2];
        for r in REGS {
            if !self.used_simd_contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Acquires a temporary XMM register.
    fn acquire_temp_simd(&mut self) -> Option<XMM> {
        let simd = self.pick_temp_simd();
        if let Some(x) = simd {
            self.used_simd_insert(x);
        }
        simd
    }

    fn reserve_simd(&mut self, simd: XMM) {
        self.used_simd_insert(simd);
    }

    // Releases a temporary XMM register.
    fn release_simd(&mut self, simd: XMM) {
        assert!(self.used_simd_remove(&simd));
    }

    fn push_used_simd(&mut self, used_xmms: &[XMM]) -> Result<usize, CompileError> {
        self.adjust_stack((used_xmms.len() * 8) as u32)?;

        for (i, r) in used_xmms.iter().enumerate() {
            self.move_location(
                Size::S64,
                Location::SIMD(*r),
                Location::Memory(GPR::RSP, (i * 8) as i32),
            )?;
        }

        Ok(used_xmms.len() * 8)
    }
    fn pop_used_simd(&mut self, used_xmms: &[XMM]) -> Result<(), CompileError> {
        for (i, r) in used_xmms.iter().enumerate() {
            self.move_location(
                Size::S64,
                Location::Memory(GPR::RSP, (i * 8) as i32),
                Location::SIMD(*r),
            )?;
        }
        self.assembler.emit_add(
            Size::S64,
            Location::Imm32((used_xmms.len() * 8) as u32),
            Location::GPR(GPR::RSP),
        )
    }

    /// Set the source location of the Wasm to the given offset.
    fn set_srcloc(&mut self, offset: u32) {
        self.src_loc = offset;
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
        let offset = self.assembler.get_offset().0;
        self.trap_table.offset_to_code.insert(offset, code);
        self.mark_instruction_address_end(offset);
    }
    /// Marks the instruction as trappable with trap code `code`. return "begin" offset
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize {
        let offset = self.assembler.get_offset().0;
        self.trap_table.offset_to_code.insert(offset, code);
        offset
    }
    /// Pushes the instruction to the address map, calculating the offset from a
    /// provided beginning address.
    fn mark_instruction_address_end(&mut self, begin: usize) {
        self.instructions_address_map.push(InstructionAddressMap {
            srcloc: SourceLoc::new(self.src_loc),
            code_offset: begin,
            code_len: self.assembler.get_offset().0 - begin,
        });
    }

    /// Insert a StackOverflow (at offset 0)
    fn insert_stackoverflow(&mut self) {
        let offset = 0;
        self.trap_table
            .offset_to_code
            .insert(offset, TrapCode::StackOverflow);
        self.mark_instruction_address_end(offset);
    }

    /// Get all current TrapInformation
    fn collect_trap_information(&self) -> Vec<TrapInformation> {
        self.trap_table
            .offset_to_code
            .clone()
            .into_iter()
            .map(|(offset, code)| TrapInformation {
                code_offset: offset as u32,
                trap_code: code,
            })
            .collect()
    }

    fn instructions_address_map(&self) -> Vec<InstructionAddressMap> {
        self.instructions_address_map.clone()
    }

    // Memory location for a local on the stack
    fn local_on_stack(&mut self, stack_offset: i32) -> Location {
        Location::Memory(GPR::RBP, -stack_offset)
    }

    // Return a rounded stack adjustement value (must be multiple of 16bytes on ARM64 for example)
    fn round_stack_adjust(&self, value: usize) -> usize {
        value
    }

    // Adjust stack for locals
    fn adjust_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        self.assembler.emit_sub(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        )
    }
    // restore stack
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        self.assembler.emit_add(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        )
    }
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        self.assembler.emit_add(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        )
    }
    // push a value on the stack for a native call
    fn move_location_for_native(
        &mut self,
        _size: Size,
        loc: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        match loc {
            Location::Imm64(_) | Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                let tmp = self.pick_temp_gpr();
                if let Some(x) = tmp {
                    self.assembler.emit_mov(Size::S64, loc, Location::GPR(x))?;
                    self.assembler.emit_mov(Size::S64, Location::GPR(x), dest)
                } else {
                    self.assembler
                        .emit_mov(Size::S64, Location::GPR(GPR::RAX), dest)?;
                    self.assembler
                        .emit_mov(Size::S64, loc, Location::GPR(GPR::RAX))?;
                    self.assembler
                        .emit_xchg(Size::S64, Location::GPR(GPR::RAX), dest)
                }
            }
            _ => self.assembler.emit_mov(Size::S64, loc, dest),
        }
    }

    // Zero a location that is 32bits
    fn zero_location(&mut self, size: Size, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_mov(size, Location::Imm32(0), location)
    }

    // GPR Reg used for local pointer on the stack
    fn local_pointer(&self) -> GPR {
        GPR::RBP
    }

    // Determine whether a local should be allocated on the stack.
    fn is_local_on_stack(&self, idx: usize) -> bool {
        idx > 3
    }

    // Determine a local's location.
    fn get_local_location(&self, idx: usize, callee_saved_regs_size: usize) -> Location {
        // Use callee-saved registers for the first locals.
        match idx {
            0 => Location::GPR(GPR::R12),
            1 => Location::GPR(GPR::R13),
            2 => Location::GPR(GPR::R14),
            3 => Location::GPR(GPR::RBX),
            _ => Location::Memory(GPR::RBP, -(((idx - 3) * 8 + callee_saved_regs_size) as i32)),
        }
    }
    // Move a local to the stack
    fn move_local(&mut self, stack_offset: i32, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_mov(
            Size::S64,
            location,
            Location::Memory(GPR::RBP, -stack_offset),
        )?;
        match location {
            Location::GPR(x) => self.emit_unwind_op(UnwindOps::SaveRegister {
                reg: UnwindRegister::GPR(x),
                bp_neg_offset: stack_offset,
            }),
            Location::SIMD(x) => self.emit_unwind_op(UnwindOps::SaveRegister {
                reg: UnwindRegister::FPR(x),
                bp_neg_offset: stack_offset,
            }),
            _ => Ok(()),
        }
    }

    // List of register to save, depending on the CallingConvention
    fn list_to_save(&self, calling_convention: CallingConvention) -> Vec<Location> {
        match calling_convention {
            CallingConvention::WindowsFastcall => {
                vec![Location::GPR(GPR::RDI), Location::GPR(GPR::RSI)]
            }
            _ => vec![],
        }
    }

    /// Get registers for first N function call parameters.
    fn get_param_registers(&self, calling_convention: CallingConvention) -> &'static [Self::GPR] {
        match calling_convention {
            CallingConvention::WindowsFastcall => &[GPR::RCX, GPR::RDX, GPR::R8, GPR::R9],
            _ => &[GPR::RDI, GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9],
        }
    }

    // Get param location
    fn get_param_location(
        &self,
        idx: usize,
        _sz: Size,
        stack_location: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location {
        self.get_param_registers(calling_convention)
            .get(idx)
            .map_or_else(
                || {
                    let loc = Location::Memory(GPR::RSP, *stack_location as i32);
                    *stack_location += 8;
                    loc
                },
                |reg| Location::GPR(*reg),
            )
    }
    // Get call param location
    fn get_call_param_location(
        &self,
        idx: usize,
        _sz: Size,
        _stack_location: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location {
        self.get_simple_param_location(idx, calling_convention)
    }
    // Get simple param location
    fn get_simple_param_location(
        &self,
        idx: usize,
        calling_convention: CallingConvention,
    ) -> Location {
        let register_params = self.get_param_registers(calling_convention);
        match calling_convention {
            CallingConvention::WindowsFastcall => register_params.get(idx).map_or_else(
                || {
                    Location::Memory(
                        GPR::RBP,
                        (32 + 16 + (idx - register_params.len()) * 8) as i32,
                    )
                },
                |reg| Location::GPR(*reg),
            ),
            _ => register_params.get(idx).map_or_else(
                || Location::Memory(GPR::RBP, (16 + (idx - register_params.len()) * 8) as i32),
                |reg| Location::GPR(*reg),
            ),
        }
    }
    // move a location to another
    fn move_location(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        match source {
            Location::GPR(_) => self.assembler.emit_mov(size, source, dest),
            Location::Memory(_, _) => match dest {
                Location::GPR(_) | Location::SIMD(_) => self.assembler.emit_mov(size, source, dest),
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    let tmp = self.pick_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass can't pick a temp gpr".to_owned())
                    })?;
                    self.assembler.emit_mov(size, source, Location::GPR(tmp))?;
                    self.assembler.emit_mov(size, Location::GPR(tmp), dest)
                }
                _ => codegen_error!("singlepass move_location unreachable"),
            },
            Location::Memory2(_, _, _, _) => match dest {
                Location::GPR(_) | Location::SIMD(_) => self.assembler.emit_mov(size, source, dest),
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    let tmp = self.pick_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass can't pick a temp gpr".to_owned())
                    })?;
                    self.assembler.emit_mov(size, source, Location::GPR(tmp))?;
                    self.assembler.emit_mov(size, Location::GPR(tmp), dest)
                }
                _ => codegen_error!("singlepass move_location unreachable"),
            },
            Location::Imm8(_) | Location::Imm32(_) | Location::Imm64(_) => match dest {
                Location::GPR(_) | Location::SIMD(_) => self.assembler.emit_mov(size, source, dest),
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    let tmp = self.pick_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass can't pick a temp gpr".to_owned())
                    })?;
                    self.assembler.emit_mov(size, source, Location::GPR(tmp))?;
                    self.assembler.emit_mov(size, Location::GPR(tmp), dest)
                }
                _ => codegen_error!("singlepass move_location unreachable"),
            },
            Location::SIMD(_) => self.assembler.emit_mov(size, source, dest),
            _ => codegen_error!("singlepass move_location unreachable"),
        }
    }
    // move a location to another
    fn move_location_extend(
        &mut self,
        size_val: Size,
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        let dst = match dest {
            Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                Location::GPR(self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?)
            }
            Location::GPR(_) | Location::SIMD(_) => dest,
            _ => codegen_error!("singlepass move_location_extend unreachable"),
        };
        match source {
            Location::GPR(_)
            | Location::Memory(_, _)
            | Location::Memory2(_, _, _, _)
            | Location::Imm32(_)
            | Location::Imm64(_) => match size_val {
                Size::S32 | Size::S64 => self.assembler.emit_mov(size_val, source, dst),
                Size::S16 | Size::S8 => {
                    if signed {
                        self.assembler.emit_movsx(size_val, source, size_op, dst)
                    } else {
                        self.assembler.emit_movzx(size_val, source, size_op, dst)
                    }
                }
            },
            _ => panic!(
                "unimplemented move_location_extend({size_val:?}, {signed}, {source:?}, {size_op:?}, {dest:?}"
            ),
        }?;
        if dst != dest {
            self.assembler.emit_mov(size_op, dst, dest)?;
            match dst {
                Location::GPR(x) => self.release_gpr(x),
                _ => codegen_error!("singlepass move_location_extend unreachable"),
            };
        }
        Ok(())
    }

    // Init the stack loc counter
    fn init_stack_loc(
        &mut self,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location,
    ) -> Result<(), CompileError> {
        // Since these assemblies take up to 24 bytes, if more than 2 slots are initialized, then they are smaller.
        self.assembler.emit_mov(
            Size::S64,
            Location::Imm64(init_stack_loc_cnt),
            Location::GPR(GPR::RCX),
        )?;
        self.assembler
            .emit_xor(Size::S64, Location::GPR(GPR::RAX), Location::GPR(GPR::RAX))?;
        self.assembler
            .emit_lea(Size::S64, last_stack_loc, Location::GPR(GPR::RDI))?;
        self.assembler.emit_rep_stosq()
    }
    // Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CompileError> {
        self.assembler.emit_lea(
            Size::S64,
            Location::Memory(GPR::RBP, -saved_area_offset),
            Location::GPR(GPR::RSP),
        )
    }
    // Pop a location
    fn pop_location(&mut self, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_pop(Size::S64, location)
    }

    // assembler finalize
    fn assembler_finalize(self) -> Result<Vec<u8>, CompileError> {
        self.assembler.finalize().map_err(|e| {
            CompileError::Codegen(format!("Assembler failed finalization with: {e:?}"))
        })
    }

    fn get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        self.assembler.finalize_function()?;
        Ok(())
    }

    fn emit_function_prolog(&mut self) -> Result<(), CompileError> {
        self.emit_push(Size::S64, Location::GPR(GPR::RBP))?;
        self.emit_unwind_op(UnwindOps::PushFP { up_to_sp: 16 })?;
        self.move_location(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RBP))?;
        self.emit_unwind_op(UnwindOps::DefineNewFrame)
    }

    fn emit_function_epilog(&mut self) -> Result<(), CompileError> {
        self.move_location(Size::S64, Location::GPR(GPR::RBP), Location::GPR(GPR::RSP))?;
        self.emit_pop(Size::S64, Location::GPR(GPR::RBP))
    }

    fn emit_function_return_value(
        &mut self,
        ty: WpType,
        canonicalize: bool,
        loc: Location,
    ) -> Result<(), CompileError> {
        if canonicalize {
            self.canonicalize_nan(
                match ty {
                    WpType::F32 => Size::S32,
                    WpType::F64 => Size::S64,
                    _ => codegen_error!("singlepass emit_function_return_value unreachable"),
                },
                loc,
                Location::GPR(GPR::RAX),
            )
        } else {
            self.emit_relaxed_mov(Size::S64, loc, Location::GPR(GPR::RAX))
        }
    }

    fn emit_function_return_float(&mut self) -> Result<(), CompileError> {
        self.move_location(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::SIMD(XMM::XMM0),
        )
    }

    fn arch_supports_canonicalize_nan(&self) -> bool {
        self.assembler.arch_supports_canonicalize_nan()
    }
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location,
        output: Location,
    ) -> Result<(), CompileError> {
        let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp3 = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;

        self.emit_relaxed_mov(sz, input, Location::SIMD(tmp1))?;
        let tmpg1 = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        match sz {
            Size::S32 => {
                self.assembler
                    .emit_vcmpunordss(tmp1, XMMOrMemory::XMM(tmp1), tmp2)?;
                self.move_location(
                    Size::S32,
                    Location::Imm32(0x7FC0_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                )?;
                self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp3))?;
                self.assembler
                    .emit_vblendvps(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1)?;
            }
            Size::S64 => {
                self.assembler
                    .emit_vcmpunordsd(tmp1, XMMOrMemory::XMM(tmp1), tmp2)?;
                self.move_location(
                    Size::S64,
                    Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                )?;
                self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp3))?;
                self.assembler
                    .emit_vblendvpd(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1)?;
            }
            _ => codegen_error!("singlepass canonicalize_nan unreachable"),
        }

        self.emit_relaxed_mov(sz, Location::SIMD(tmp1), output)?;

        self.release_gpr(tmpg1);
        self.release_simd(tmp3);
        self.release_simd(tmp2);
        self.release_simd(tmp1);
        Ok(())
    }

    fn emit_illegal_op(&mut self, trap: TrapCode) -> Result<(), CompileError> {
        // code below is kept as a reference on how to emit illegal op with trap info
        // without an Undefined opcode with payload
        /*
        let offset = self.assembler.get_offset().0;
        self.trap_table
        .offset_to_code
        .insert(offset, trap);
        self.assembler.emit_ud2();
        self.mark_instruction_address_end(offset);*/
        let v = trap as u8;
        // payload needs to be between 0-15
        // this will emit an 40 0F B9 Cx opcode, with x the payload
        let offset = self.assembler.get_offset().0;
        self.assembler.emit_ud1_payload(v)?;
        self.mark_instruction_address_end(offset);
        Ok(())
    }
    fn get_label(&mut self) -> Label {
        self.assembler.new_dynamic_label()
    }
    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        self.assembler.emit_label(label)
    }
    fn get_grp_for_call(&self) -> GPR {
        GPR::RAX
    }
    fn emit_call_register(&mut self, reg: GPR) -> Result<(), CompileError> {
        self.assembler.emit_call_register(reg)
    }
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        self.assembler.emit_call_label(label)
    }
    fn get_gpr_for_ret(&self) -> GPR {
        GPR::RAX
    }
    fn get_simd_for_ret(&self) -> XMM {
        XMM::XMM0
    }

    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        self.assembler.arch_requires_indirect_call_trampoline()
    }

    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        location: Location,
    ) -> Result<(), CompileError> {
        self.assembler
            .arch_emit_indirect_call_with_trampoline(location)
    }

    fn emit_debug_breakpoint(&mut self) -> Result<(), CompileError> {
        self.assembler.emit_bkpt()
    }

    fn emit_call_location(&mut self, location: Location) -> Result<(), CompileError> {
        self.assembler.emit_call_location(location)
    }
    fn location_add(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        _flags: bool,
    ) -> Result<(), CompileError> {
        self.assembler.emit_add(size, source, dest)
    }
    fn location_cmp(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        self.assembler.emit_cmp(size, source, dest)
    }

    // unconditional jmp
    fn jmp_unconditional(&mut self, label: Label) -> Result<(), CompileError> {
        self.assembler.emit_jmp(Condition::None, label)
    }

    fn jmp_on_condition(
        &mut self,
        cond: UnsignedCondition,
        size: Size,
        loc_a: AbstractLocation<Self::GPR, Self::SIMD>,
        loc_b: AbstractLocation<Self::GPR, Self::SIMD>,
        label: Label,
    ) -> Result<(), CompileError> {
        self.assembler.emit_cmp(size, loc_b, loc_a)?;
        let cond = match cond {
            UnsignedCondition::Equal => Condition::Equal,
            UnsignedCondition::NotEqual => Condition::NotEqual,
            UnsignedCondition::Above => Condition::Above,
            UnsignedCondition::AboveEqual => Condition::AboveEqual,
            UnsignedCondition::Below => Condition::Below,
            UnsignedCondition::BelowEqual => Condition::BelowEqual,
        };
        self.assembler.emit_jmp(cond, label)
    }

    // jmp table
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location) -> Result<(), CompileError> {
        let tmp1 = self
            .pick_temp_gpr()
            .ok_or_else(|| CompileError::Codegen("singlepass can't pick a temp gpr".to_owned()))?;
        self.reserve_gpr(tmp1);
        let tmp2 = self
            .pick_temp_gpr()
            .ok_or_else(|| CompileError::Codegen("singlepass can't pick a temp gpr".to_owned()))?;
        self.reserve_gpr(tmp2);

        self.assembler.emit_lea_label(label, Location::GPR(tmp1))?;
        self.move_location(Size::S32, cond, Location::GPR(tmp2))?;

        let instr_size = self.assembler.get_jmp_instr_size();
        self.assembler
            .emit_imul_imm32_gpr64(instr_size as _, tmp2)?;
        self.assembler
            .emit_add(Size::S64, Location::GPR(tmp1), Location::GPR(tmp2))?;
        self.assembler.emit_jmp_location(Location::GPR(tmp2))?;
        self.release_gpr(tmp2);
        self.release_gpr(tmp1);
        Ok(())
    }

    fn align_for_loop(&mut self) -> Result<(), CompileError> {
        // Pad with NOPs to the next 16-byte boundary.
        // Here we don't use the dynasm `.align 16` attribute because it pads the alignment with single-byte nops
        // which may lead to efficiency problems.
        match self.assembler.get_offset().0 % 16 {
            0 => {}
            x => {
                self.assembler.emit_nop_n(16 - x)?;
            }
        }
        assert_eq!(self.assembler.get_offset().0 % 16, 0);
        Ok(())
    }

    fn emit_ret(&mut self) -> Result<(), CompileError> {
        self.assembler.emit_ret()
    }

    fn emit_push(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        self.assembler.emit_push(size, loc)
    }
    fn emit_pop(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        self.assembler.emit_pop(size, loc)
    }

    fn emit_memory_fence(&mut self) -> Result<(), CompileError> {
        // nothing on x86_64
        Ok(())
    }

    fn emit_imul_imm32(&mut self, size: Size, imm32: u32, gpr: GPR) -> Result<(), CompileError> {
        match size {
            Size::S64 => self.assembler.emit_imul_imm32_gpr64(imm32, gpr),
            _ => {
                codegen_error!("singlepass emit_imul_imm32 unreachable");
            }
        }
    }

    // relaxed binop based...
    fn emit_relaxed_mov(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop(AssemblerX64::emit_mov, sz, src, dst)
    }
    fn emit_relaxed_cmp(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_binop(AssemblerX64::emit_cmp, sz, src, dst)
    }

    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_zx_sx(AssemblerX64::emit_movsx, sz_src, src, sz_dst, dst)
    }

    fn emit_binop_add32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_add, loc_a, loc_b, ret)
    }
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_sub, loc_a, loc_b, ret)
    }
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_imul, loc_a, loc_b, ret)
    }
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler
            .emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX))?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_div,
            Size::S32,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S32, Location::GPR(GPR::RAX), ret)?;
        Ok(offset)
    }
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        _integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler.emit_cdq()?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_idiv,
            Size::S32,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S32, Location::GPR(GPR::RAX), ret)?;
        Ok(offset)
    }
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler
            .emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX))?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_div,
            Size::S32,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S32, Location::GPR(GPR::RDX), ret)?;
        Ok(offset)
    }
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        let normal_path = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_relaxed_cmp(Size::S32, Location::Imm32(0x80000000), loc_a)?;
        self.assembler.emit_jmp(Condition::NotEqual, normal_path)?;
        self.emit_relaxed_cmp(Size::S32, Location::Imm32(0xffffffff), loc_b)?;
        self.assembler.emit_jmp(Condition::NotEqual, normal_path)?;
        self.move_location(Size::S32, Location::Imm32(0), ret)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        self.emit_label(normal_path)?;
        self.assembler
            .emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler.emit_cdq()?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_idiv,
            Size::S32,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S32, Location::GPR(GPR::RDX), ret)?;

        self.emit_label(end)?;
        Ok(offset)
    }
    fn emit_binop_and32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_and, loc_a, loc_b, ret)
    }
    fn emit_binop_or32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_or, loc_a, loc_b, ret)
    }
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i32(AssemblerX64::emit_xor, loc_a, loc_b, ret)
    }
    fn i32_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::GreaterEqual, loc_a, loc_b, ret)
    }
    fn i32_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Greater, loc_a, loc_b, ret)
    }
    fn i32_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::LessEqual, loc_a, loc_b, ret)
    }
    fn i32_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Less, loc_a, loc_b, ret)
    }
    fn i32_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::AboveEqual, loc_a, loc_b, ret)
    }
    fn i32_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Above, loc_a, loc_b, ret)
    }
    fn i32_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::BelowEqual, loc_a, loc_b, ret)
    }
    fn i32_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Below, loc_a, loc_b, ret)
    }
    fn i32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::NotEqual, loc_a, loc_b, ret)
    }
    fn i32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i32_dynamic_b(Condition::Equal, loc_a, loc_b, ret)
    }
    fn i32_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let src = match loc {
            Location::Imm32(_) | Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S32, loc, Location::GPR(tmp))?;
                tmp
            }
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i32_clz unreachable");
            }
        };
        let dst = match ret {
            Location::Memory(_, _) => self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?,
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i32_clz unreachable");
            }
        };

        if self.assembler.arch_has_xzcnt() {
            self.assembler
                .arch_emit_lzcnt(Size::S32, Location::GPR(src), Location::GPR(dst))?;
        } else {
            let zero_path = self.assembler.get_label();
            let end = self.assembler.get_label();

            self.assembler.emit_test_gpr_64(src)?;
            self.assembler.emit_jmp(Condition::Equal, zero_path)?;
            self.assembler
                .emit_bsr(Size::S32, Location::GPR(src), Location::GPR(dst))?;
            self.assembler
                .emit_xor(Size::S32, Location::Imm32(31), Location::GPR(dst))?;
            self.assembler.emit_jmp(Condition::None, end)?;
            self.emit_label(zero_path)?;
            self.move_location(Size::S32, Location::Imm32(32), Location::GPR(dst))?;
            self.emit_label(end)?;
        }
        match loc {
            Location::Imm32(_) | Location::Memory(_, _) => {
                self.release_gpr(src);
            }
            _ => {}
        };
        if let Location::Memory(_, _) = ret {
            self.move_location(Size::S32, Location::GPR(dst), ret)?;
            self.release_gpr(dst);
        };
        Ok(())
    }
    fn i32_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let src = match loc {
            Location::Imm32(_) | Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S32, loc, Location::GPR(tmp))?;
                tmp
            }
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i32_ctz unreachable");
            }
        };
        let dst = match ret {
            Location::Memory(_, _) => self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?,
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i32_ctz unreachable");
            }
        };

        if self.assembler.arch_has_xzcnt() {
            self.assembler
                .arch_emit_tzcnt(Size::S32, Location::GPR(src), Location::GPR(dst))?;
        } else {
            let zero_path = self.assembler.get_label();
            let end = self.assembler.get_label();

            self.assembler.emit_test_gpr_64(src)?;
            self.assembler.emit_jmp(Condition::Equal, zero_path)?;
            self.assembler
                .emit_bsf(Size::S32, Location::GPR(src), Location::GPR(dst))?;
            self.assembler.emit_jmp(Condition::None, end)?;
            self.emit_label(zero_path)?;
            self.move_location(Size::S32, Location::Imm32(32), Location::GPR(dst))?;
            self.emit_label(end)?;
        }

        match loc {
            Location::Imm32(_) | Location::Memory(_, _) => {
                self.release_gpr(src);
            }
            _ => {}
        };
        if let Location::Memory(_, _) = ret {
            self.move_location(Size::S32, Location::GPR(dst), ret)?;
            self.release_gpr(dst);
        };
        Ok(())
    }
    fn i32_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        match loc {
            Location::Imm32(_) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S32, loc, Location::GPR(tmp))?;
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler.emit_popcnt(
                        Size::S32,
                        Location::GPR(tmp),
                        Location::GPR(out_tmp),
                    )?;
                    self.move_location(Size::S32, Location::GPR(out_tmp), ret)?;
                    self.release_gpr(out_tmp);
                } else {
                    self.assembler
                        .emit_popcnt(Size::S32, Location::GPR(tmp), ret)?;
                }
                self.release_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler
                        .emit_popcnt(Size::S32, loc, Location::GPR(out_tmp))?;
                    self.move_location(Size::S32, Location::GPR(out_tmp), ret)?;
                    self.release_gpr(out_tmp);
                } else {
                    self.assembler.emit_popcnt(Size::S32, loc, ret)?;
                }
            }
            _ => {
                codegen_error!("singlepass i32_popcnt unreachable");
            }
        }
        Ok(())
    }
    fn i32_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i32(AssemblerX64::emit_shl, loc_a, loc_b, ret)
    }
    fn i32_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i32(AssemblerX64::emit_shr, loc_a, loc_b, ret)
    }
    fn i32_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i32(AssemblerX64::emit_sar, loc_a, loc_b, ret)
    }
    fn i32_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i32(AssemblerX64::emit_rol, loc_a, loc_b, ret)
    }
    fn i32_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i32(AssemblerX64::emit_ror, loc_a, loc_b, ret)
    }
    fn i32_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    Location::Memory(addr, 0),
                    ret,
                )
            },
        )
    }
    fn i32_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movzx,
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_load_8s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movsx,
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movzx,
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_load_16s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movsx,
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_atomic_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_mov(Size::S32, Location::Memory(addr, 0), ret),
        )
    }
    fn i32_atomic_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zero_extension(
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_atomic_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zero_extension(
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S32,
                    ret,
                )
            },
        )
    }
    fn i32_save(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i32_save_8(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S8,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i32_save_16(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S16,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    // x86_64 have a strong memory model, so coherency between all threads (core) is garantied
    // and aligned move is guarantied to be atomic, too or from memory
    // so store/load an atomic is a simple mov on x86_64
    fn i32_atomic_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i32_atomic_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S8,
                    value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i32_atomic_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S16,
                    value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    // i32 atomic Add with i32
    fn i32_atomic_add(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S32, loc, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Add with u8
    fn i32_atomic_add_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location_extend(Size::S8, false, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Add with u16
    fn i32_atomic_add_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location_extend(Size::S16, false, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Sub with i32
    fn i32_atomic_sub(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S32, false, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Sub with u8
    fn i32_atomic_sub_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S8, false, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Sub with u16
    fn i32_atomic_sub_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S16, false, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic And with i32
    fn i32_atomic_and(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic And with u8
    fn i32_atomic_and_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic And with u16
    fn i32_atomic_and_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Or with i32
    fn i32_atomic_or(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_or(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Or with u8
    fn i32_atomic_or_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_or(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Or with u16
    fn i32_atomic_or_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_or(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Xor with i32
    fn i32_atomic_xor(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Xor with u8
    fn i32_atomic_xor_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Xor with u16
    fn i32_atomic_xor_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S32,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i32 atomic Exchange with i32
    fn i32_atomic_xchg(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S32, loc, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S32, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Exchange with u8
    fn i32_atomic_xchg_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_movzx(Size::S8, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S8, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Exchange with u16
    fn i32_atomic_xchg_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_movzx(Size::S16, loc, Size::S32, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S16, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S32, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i32 atomic Exchange with i32
    fn i32_atomic_cmpxchg(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S32, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S32, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_mov(Size::S32, Location::GPR(compare), ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }
    // i32 atomic Exchange with u8
    fn i32_atomic_cmpxchg_8u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S32, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S32, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_movzx(Size::S8, Location::GPR(compare), Size::S32, ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }
    // i32 atomic Exchange with u16
    fn i32_atomic_cmpxchg_16u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S32, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S32, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_movzx(Size::S16, Location::GPR(compare), Size::S32, ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }

    fn emit_call_with_reloc(
        &mut self,
        _calling_convention: CallingConvention,
        reloc_target: RelocationTarget,
    ) -> Result<Vec<Relocation>, CompileError> {
        let mut relocations = vec![];
        let next = self.get_label();
        let reloc_at = self.assembler.get_offset().0 + 1; // skip E8
        self.assembler.emit_call_label(next)?;
        self.emit_label(next)?;
        relocations.push(Relocation {
            kind: RelocationKind::X86CallPCRel4,
            reloc_target,
            offset: reloc_at as u32,
            addend: -4,
        });
        Ok(relocations)
    }

    fn emit_binop_add64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_add, loc_a, loc_b, ret)
    }
    fn emit_binop_sub64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_sub, loc_a, loc_b, ret)
    }
    fn emit_binop_mul64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_imul, loc_a, loc_b, ret)
    }
    fn emit_binop_udiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler
            .emit_xor(Size::S64, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX))?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_div,
            Size::S64,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::RAX), ret)?;
        Ok(offset)
    }
    fn emit_binop_sdiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        _integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler.emit_cqo()?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_idiv,
            Size::S64,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::RAX), ret)?;
        Ok(offset)
    }
    fn emit_binop_urem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        self.assembler
            .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler
            .emit_xor(Size::S64, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX))?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_div,
            Size::S64,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::RDX), ret)?;
        Ok(offset)
    }
    fn emit_binop_srem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
    ) -> Result<usize, CompileError> {
        // We assume that RAX and RDX are temporary registers here.
        let normal_path = self.assembler.get_label();
        let end = self.assembler.get_label();

        self.emit_relaxed_cmp(Size::S64, Location::Imm64(0x8000000000000000u64), loc_a)?;
        self.assembler.emit_jmp(Condition::NotEqual, normal_path)?;
        self.emit_relaxed_cmp(Size::S64, Location::Imm64(0xffffffffffffffffu64), loc_b)?;
        self.assembler.emit_jmp(Condition::NotEqual, normal_path)?;
        self.move_location(Size::S64, Location::Imm64(0), ret)?;
        self.assembler.emit_jmp(Condition::None, end)?;

        self.emit_label(normal_path)?;
        self.assembler
            .emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX))?;
        self.assembler.emit_cqo()?;
        let offset = self.emit_relaxed_xdiv(
            AssemblerX64::emit_idiv,
            Size::S64,
            loc_b,
            integer_division_by_zero,
        )?;
        self.assembler
            .emit_mov(Size::S64, Location::GPR(GPR::RDX), ret)?;

        self.emit_label(end)?;
        Ok(offset)
    }
    fn emit_binop_and64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_and, loc_a, loc_b, ret)
    }
    fn emit_binop_or64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_or, loc_a, loc_b, ret)
    }
    fn emit_binop_xor64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_binop_i64(AssemblerX64::emit_xor, loc_a, loc_b, ret)
    }
    fn i64_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::GreaterEqual, loc_a, loc_b, ret)
    }
    fn i64_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Greater, loc_a, loc_b, ret)
    }
    fn i64_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::LessEqual, loc_a, loc_b, ret)
    }
    fn i64_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Less, loc_a, loc_b, ret)
    }
    fn i64_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::AboveEqual, loc_a, loc_b, ret)
    }
    fn i64_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Above, loc_a, loc_b, ret)
    }
    fn i64_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::BelowEqual, loc_a, loc_b, ret)
    }
    fn i64_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Below, loc_a, loc_b, ret)
    }
    fn i64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::NotEqual, loc_a, loc_b, ret)
    }
    fn i64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_cmpop_i64_dynamic_b(Condition::Equal, loc_a, loc_b, ret)
    }
    fn i64_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let src = match loc {
            Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S64, loc, Location::GPR(tmp))?;
                tmp
            }
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i64_clz unreachable");
            }
        };
        let dst = match ret {
            Location::Memory(_, _) => self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?,
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i64_clz unreachable");
            }
        };

        if self.assembler.arch_has_xzcnt() {
            self.assembler
                .arch_emit_lzcnt(Size::S64, Location::GPR(src), Location::GPR(dst))?;
        } else {
            let zero_path = self.assembler.get_label();
            let end = self.assembler.get_label();

            self.assembler.emit_test_gpr_64(src)?;
            self.assembler.emit_jmp(Condition::Equal, zero_path)?;
            self.assembler
                .emit_bsr(Size::S64, Location::GPR(src), Location::GPR(dst))?;
            self.assembler
                .emit_xor(Size::S64, Location::Imm32(63), Location::GPR(dst))?;
            self.assembler.emit_jmp(Condition::None, end)?;
            self.emit_label(zero_path)?;
            self.move_location(Size::S64, Location::Imm32(64), Location::GPR(dst))?;
            self.emit_label(end)?;
        }
        match loc {
            Location::Imm64(_) | Location::Memory(_, _) => {
                self.release_gpr(src);
            }
            _ => {}
        };
        if let Location::Memory(_, _) = ret {
            self.move_location(Size::S64, Location::GPR(dst), ret)?;
            self.release_gpr(dst);
        };
        Ok(())
    }
    fn i64_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let src = match loc {
            Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S64, loc, Location::GPR(tmp))?;
                tmp
            }
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i64_ctz unreachable");
            }
        };
        let dst = match ret {
            Location::Memory(_, _) => self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?,
            Location::GPR(reg) => reg,
            _ => {
                codegen_error!("singlepass i64_ctz unreachable");
            }
        };

        if self.assembler.arch_has_xzcnt() {
            self.assembler
                .arch_emit_tzcnt(Size::S64, Location::GPR(src), Location::GPR(dst))?;
        } else {
            let zero_path = self.assembler.get_label();
            let end = self.assembler.get_label();

            self.assembler.emit_test_gpr_64(src)?;
            self.assembler.emit_jmp(Condition::Equal, zero_path)?;
            self.assembler
                .emit_bsf(Size::S64, Location::GPR(src), Location::GPR(dst))?;
            self.assembler.emit_jmp(Condition::None, end)?;
            self.emit_label(zero_path)?;
            self.move_location(Size::S64, Location::Imm64(64), Location::GPR(dst))?;
            self.emit_label(end)?;
        }

        match loc {
            Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                self.release_gpr(src);
            }
            _ => {}
        };
        if let Location::Memory(_, _) = ret {
            self.move_location(Size::S64, Location::GPR(dst), ret)?;
            self.release_gpr(dst);
        };
        Ok(())
    }
    fn i64_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                    CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                })?;
                self.move_location(Size::S64, loc, Location::GPR(tmp))?;
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler.emit_popcnt(
                        Size::S64,
                        Location::GPR(tmp),
                        Location::GPR(out_tmp),
                    )?;
                    self.move_location(Size::S64, Location::GPR(out_tmp), ret)?;
                    self.release_gpr(out_tmp);
                } else {
                    self.assembler
                        .emit_popcnt(Size::S64, Location::GPR(tmp), ret)?;
                }
                self.release_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = self.acquire_temp_gpr().ok_or_else(|| {
                        CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
                    })?;
                    self.assembler
                        .emit_popcnt(Size::S64, loc, Location::GPR(out_tmp))?;
                    self.move_location(Size::S64, Location::GPR(out_tmp), ret)?;
                    self.release_gpr(out_tmp);
                } else {
                    self.assembler.emit_popcnt(Size::S64, loc, ret)?;
                }
            }
            _ => {
                codegen_error!("singlepass i64_popcnt unreachable");
            }
        }
        Ok(())
    }
    fn i64_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i64(AssemblerX64::emit_shl, loc_a, loc_b, ret)
    }
    fn i64_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i64(AssemblerX64::emit_shr, loc_a, loc_b, ret)
    }
    fn i64_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i64(AssemblerX64::emit_sar, loc_a, loc_b, ret)
    }
    fn i64_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i64(AssemblerX64::emit_rol, loc_a, loc_b, ret)
    }
    fn i64_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_shift_i64(AssemblerX64::emit_ror, loc_a, loc_b, ret)
    }
    fn i64_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S64,
                    Location::Memory(addr, 0),
                    ret,
                )
            },
        )
    }
    fn i64_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movzx,
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_load_8s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movsx,
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movzx,
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_load_16s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movsx,
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_load_32u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                match ret {
                    Location::GPR(_) => {}
                    Location::Memory(base, offset) => {
                        this.assembler.emit_mov(
                            Size::S32,
                            Location::Imm32(0),
                            Location::Memory(base, offset + 4),
                        )?; // clear upper bits
                    }
                    _ => {
                        codegen_error!("singlepass i64_load_32u unreacahble");
                    }
                }
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    Location::Memory(addr, 0),
                    ret,
                )
            },
        )
    }
    fn i64_load_32s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zx_sx(
                    AssemblerX64::emit_movsx,
                    Size::S32,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_atomic_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_mov(Size::S64, Location::Memory(addr, 0), ret),
        )
    }
    fn i64_atomic_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zero_extension(
                    Size::S8,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_atomic_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_zero_extension(
                    Size::S16,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_atomic_load_32u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                match ret {
                    Location::GPR(_) => {}
                    Location::Memory(base, offset) => {
                        this.move_location(
                            Size::S32,
                            Location::Imm32(0),
                            Location::Memory(base, offset + 4),
                        )?; // clear upper bits
                    }
                    _ => {
                        codegen_error!("singlepass i64_atomic_load_32u unreachable");
                    }
                }
                this.emit_relaxed_zero_extension(
                    Size::S32,
                    Location::Memory(addr, 0),
                    Size::S64,
                    ret,
                )
            },
        )
    }
    fn i64_save(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S64,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i64_save_8(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S8,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i64_save_16(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S16,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i64_save_32(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    target_value,
                    Location::Memory(addr, 0),
                )
            },
        )
    }
    fn i64_atomic_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_xchg(Size::S64, value, Location::Memory(addr, 0)),
        )
    }
    fn i64_atomic_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_xchg(Size::S8, value, Location::Memory(addr, 0)),
        )
    }
    fn i64_atomic_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_xchg(Size::S16, value, Location::Memory(addr, 0)),
        )
    }
    fn i64_atomic_save_32(
        &mut self,
        value: Location,
        memarg: &MemArg,
        target_addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            target_addr,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| this.emit_relaxed_atomic_xchg(Size::S32, value, Location::Memory(addr, 0)),
        )
    }
    // i64 atomic Add with i64
    fn i64_atomic_add(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S64, loc, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S64,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Add with u8
    fn i64_atomic_add_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location_extend(Size::S8, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Add with u16
    fn i64_atomic_add_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location_extend(Size::S16, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Add with u32
    fn i64_atomic_add_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location_extend(Size::S32, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Sub with i64
    fn i64_atomic_sub(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S64, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S64,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Sub with u8
    fn i64_atomic_sub_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S8, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Sub with u16
    fn i64_atomic_sub_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S16, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Sub with u32
    fn i64_atomic_sub_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.location_neg(Size::S32, false, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_xadd(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic And with i64
    fn i64_atomic_and(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            8,
            Size::S64,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S64, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i64 atomic And with u8
    fn i64_atomic_and_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S64, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i64 atomic And with u16
    fn i64_atomic_and_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S64, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i64 atomic And with u32
    fn i64_atomic_and_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.assembler
                    .emit_and(Size::S64, Location::GPR(src), Location::GPR(dst))
            },
        )
    }
    // i64 atomic Or with i64
    fn i64_atomic_or(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            8,
            Size::S64,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_or(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic Or with u8
    fn i64_atomic_or_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_or(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic Or with u16
    fn i64_atomic_or_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_or(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic Or with u32
    fn i64_atomic_or_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_or(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic xor with i64
    fn i64_atomic_xor(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            8,
            Size::S64,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_xor(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic xor with u8
    fn i64_atomic_xor_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            1,
            Size::S8,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_xor(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic xor with u16
    fn i64_atomic_xor_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            2,
            Size::S16,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_xor(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic xor with u32
    fn i64_atomic_xor_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.emit_compare_and_swap(
            loc,
            target,
            ret,
            memarg,
            4,
            Size::S32,
            Size::S64,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, src, dst| {
                this.location_xor(Size::S64, Location::GPR(src), Location::GPR(dst), false)
            },
        )
    }
    // i64 atomic Exchange with i64
    fn i64_atomic_xchg(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S64, loc, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S64, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Exchange with u8
    fn i64_atomic_xchg_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_movzx(Size::S8, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S8, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Exchange with u16
    fn i64_atomic_xchg_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_movzx(Size::S16, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S16, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Exchange with u32
    fn i64_atomic_xchg_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let value = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.assembler
            .emit_movzx(Size::S32, loc, Size::S64, Location::GPR(value))?;
        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler
                    .emit_xchg(Size::S32, Location::GPR(value), Location::Memory(addr, 0))
            },
        )?;
        self.move_location(Size::S64, Location::GPR(value), ret)?;
        self.release_gpr(value);
        Ok(())
    }
    // i64 atomic Exchange with i64
    fn i64_atomic_cmpxchg(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S64, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S64, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S64,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_mov(Size::S64, Location::GPR(compare), ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }
    // i64 atomic Exchange with u8
    fn i64_atomic_cmpxchg_8u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S64, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S64, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            1,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S8,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_movzx(Size::S8, Location::GPR(compare), Size::S64, ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }
    // i64 atomic Exchange with u16
    fn i64_atomic_cmpxchg_16u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S64, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S64, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            2,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S16,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_movzx(Size::S16, Location::GPR(compare), Size::S64, ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }
    // i64 atomic Exchange with u32
    fn i64_atomic_cmpxchg_32u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let compare = self.reserve_unused_temp_gpr(GPR::RAX);
        let value = if cmp == Location::GPR(GPR::R14) {
            if new == Location::GPR(GPR::R13) {
                GPR::R12
            } else {
                GPR::R13
            }
        } else {
            GPR::R14
        };
        self.assembler.emit_push(Size::S64, Location::GPR(value))?;
        self.assembler
            .emit_mov(Size::S64, cmp, Location::GPR(compare))?;
        self.assembler
            .emit_mov(Size::S64, new, Location::GPR(value))?;

        self.memory_op(
            target,
            memarg,
            true,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.assembler.emit_lock_cmpxchg(
                    Size::S32,
                    Location::GPR(value),
                    Location::Memory(addr, 0),
                )?;
                this.assembler
                    .emit_mov(Size::S32, Location::GPR(compare), ret)
            },
        )?;
        self.assembler.emit_pop(Size::S64, Location::GPR(value))?;
        self.release_gpr(compare);
        Ok(())
    }

    fn f32_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S32,
                    Location::Memory(addr, 0),
                    ret,
                )
            },
        )
    }
    fn f32_save(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let canonicalize = canonicalize && self.arch_supports_canonicalize_nan();
        self.memory_op(
            target_addr,
            memarg,
            false,
            4,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                if !canonicalize {
                    this.emit_relaxed_binop(
                        AssemblerX64::emit_mov,
                        Size::S32,
                        target_value,
                        Location::Memory(addr, 0),
                    )
                } else {
                    this.canonicalize_nan(Size::S32, target_value, Location::Memory(addr, 0))
                }
            },
        )
    }
    fn f64_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        self.memory_op(
            addr,
            memarg,
            false,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                this.emit_relaxed_binop(
                    AssemblerX64::emit_mov,
                    Size::S64,
                    Location::Memory(addr, 0),
                    ret,
                )
            },
        )
    }
    fn f64_save(
        &mut self,
        target_value: Location,
        memarg: &MemArg,
        target_addr: Location,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        let canonicalize = canonicalize && self.arch_supports_canonicalize_nan();
        self.memory_op(
            target_addr,
            memarg,
            false,
            8,
            need_check,
            imported_memories,
            offset,
            heap_access_oob,
            unaligned_atomic,
            |this, addr| {
                if !canonicalize {
                    this.emit_relaxed_binop(
                        AssemblerX64::emit_mov,
                        Size::S64,
                        target_value,
                        Location::Memory(addr, 0),
                    )
                } else {
                    this.canonicalize_nan(Size::S64, target_value, Location::Memory(addr, 0))
                }
            },
        )
    }

    fn convert_f64_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp_in = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        if self.assembler.arch_has_fconverti() {
            self.emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler.arch_emit_f64_convert_si64(tmp_in, tmp_out)?;
            } else {
                self.assembler.arch_emit_f64_convert_ui64(tmp_in, tmp_out)?;
            }
            self.emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret)?;
        } else if signed {
            self.assembler
                .emit_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            self.assembler
                .emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.move_location(Size::S64, Location::SIMD(tmp_out), ret)?;
        } else {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let do_convert = self.assembler.get_label();
            let end_convert = self.assembler.get_label();

            self.assembler
                .emit_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            self.assembler.emit_test_gpr_64(tmp_in)?;
            self.assembler.emit_jmp(Condition::Signed, do_convert)?;
            self.assembler
                .emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.assembler.emit_jmp(Condition::None, end_convert)?;
            self.emit_label(do_convert)?;
            self.move_location(Size::S64, Location::GPR(tmp_in), Location::GPR(tmp))?;
            self.assembler
                .emit_and(Size::S64, Location::Imm32(1), Location::GPR(tmp))?;
            self.assembler
                .emit_shr(Size::S64, Location::Imm8(1), Location::GPR(tmp_in))?;
            self.assembler
                .emit_or(Size::S64, Location::GPR(tmp), Location::GPR(tmp_in))?;
            self.assembler
                .emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.assembler
                .emit_vaddsd(tmp_out, XMMOrMemory::XMM(tmp_out), tmp_out)?;
            self.emit_label(end_convert)?;
            self.move_location(Size::S64, Location::SIMD(tmp_out), ret)?;

            self.release_gpr(tmp);
        }
        self.release_gpr(tmp_in);
        self.release_simd(tmp_out);
        Ok(())
    }
    fn convert_f64_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp_in = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        if self.assembler.arch_has_fconverti() {
            self.emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler.arch_emit_f64_convert_si32(tmp_in, tmp_out)?;
            } else {
                self.assembler.arch_emit_f64_convert_ui32(tmp_in, tmp_out)?;
            }
            self.emit_relaxed_mov(Size::S64, Location::SIMD(tmp_out), ret)?;
        } else {
            self.assembler
                .emit_mov(Size::S32, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler
                    .emit_vcvtsi2sd_32(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            } else {
                self.assembler
                    .emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            }
            self.move_location(Size::S64, Location::SIMD(tmp_out), ret)?;
        }
        self.release_gpr(tmp_in);
        self.release_simd(tmp_out);
        Ok(())
    }
    fn convert_f32_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp_in = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        if self.assembler.arch_has_fconverti() {
            self.emit_relaxed_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler.arch_emit_f32_convert_si64(tmp_in, tmp_out)?;
            } else {
                self.assembler.arch_emit_f32_convert_ui64(tmp_in, tmp_out)?;
            }
            self.emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret)?;
        } else if signed {
            self.assembler
                .emit_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            self.assembler
                .emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.move_location(Size::S32, Location::SIMD(tmp_out), ret)?;
        } else {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let do_convert = self.assembler.get_label();
            let end_convert = self.assembler.get_label();

            self.assembler
                .emit_mov(Size::S64, loc, Location::GPR(tmp_in))?;
            self.assembler.emit_test_gpr_64(tmp_in)?;
            self.assembler.emit_jmp(Condition::Signed, do_convert)?;
            self.assembler
                .emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.assembler.emit_jmp(Condition::None, end_convert)?;
            self.emit_label(do_convert)?;
            self.move_location(Size::S64, Location::GPR(tmp_in), Location::GPR(tmp))?;
            self.assembler
                .emit_and(Size::S64, Location::Imm32(1), Location::GPR(tmp))?;
            self.assembler
                .emit_shr(Size::S64, Location::Imm8(1), Location::GPR(tmp_in))?;
            self.assembler
                .emit_or(Size::S64, Location::GPR(tmp), Location::GPR(tmp_in))?;
            self.assembler
                .emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            self.assembler
                .emit_vaddss(tmp_out, XMMOrMemory::XMM(tmp_out), tmp_out)?;
            self.emit_label(end_convert)?;
            self.move_location(Size::S32, Location::SIMD(tmp_out), ret)?;

            self.release_gpr(tmp);
        }
        self.release_gpr(tmp_in);
        self.release_simd(tmp_out);
        Ok(())
    }
    fn convert_f32_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        let tmp_out = self.acquire_temp_simd().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
        })?;
        let tmp_in = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        if self.assembler.arch_has_fconverti() {
            self.emit_relaxed_mov(Size::S32, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler.arch_emit_f32_convert_si32(tmp_in, tmp_out)?;
            } else {
                self.assembler.arch_emit_f32_convert_ui32(tmp_in, tmp_out)?;
            }
            self.emit_relaxed_mov(Size::S32, Location::SIMD(tmp_out), ret)?;
        } else {
            self.assembler
                .emit_mov(Size::S32, loc, Location::GPR(tmp_in))?;
            if signed {
                self.assembler
                    .emit_vcvtsi2ss_32(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            } else {
                self.assembler
                    .emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out)?;
            }
            self.move_location(Size::S32, Location::SIMD(tmp_out), ret)?;
        }
        self.release_gpr(tmp_in);
        self.release_simd(tmp_out);
        Ok(())
    }
    fn convert_i64_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        match (signed, sat) {
            (false, true) => self.convert_i64_f64_u_s(loc, ret),
            (false, false) => self.convert_i64_f64_u_u(loc, ret),
            (true, true) => self.convert_i64_f64_s_s(loc, ret),
            (true, false) => self.convert_i64_f64_s_u(loc, ret),
        }
    }
    fn convert_i32_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        match (signed, sat) {
            (false, true) => self.convert_i32_f64_u_s(loc, ret),
            (false, false) => self.convert_i32_f64_u_u(loc, ret),
            (true, true) => self.convert_i32_f64_s_s(loc, ret),
            (true, false) => self.convert_i32_f64_s_u(loc, ret),
        }
    }
    fn convert_i64_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        match (signed, sat) {
            (false, true) => self.convert_i64_f32_u_s(loc, ret),
            (false, false) => self.convert_i64_f32_u_u(loc, ret),
            (true, true) => self.convert_i64_f32_s_s(loc, ret),
            (true, false) => self.convert_i64_f32_s_u(loc, ret),
        }
    }
    fn convert_i32_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        match (signed, sat) {
            (false, true) => self.convert_i32_f32_u_s(loc, ret),
            (false, false) => self.convert_i32_f32_u_u(loc, ret),
            (true, true) => self.convert_i32_f32_s_s(loc, ret),
            (true, false) => self.convert_i32_f32_s_u(loc, ret),
        }
    }
    fn convert_f64_f32(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcvtss2sd, loc, loc, ret)
    }
    fn convert_f32_f64(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcvtsd2ss, loc, loc, ret)
    }
    fn f64_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_fneg() {
            let tmp = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S64, loc, Location::SIMD(tmp))?;
            self.assembler.arch_emit_f64_neg(tmp, tmp)?;
            self.emit_relaxed_mov(Size::S64, Location::SIMD(tmp), ret)?;
            self.release_simd(tmp);
        } else {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.move_location(Size::S64, loc, Location::GPR(tmp))?;
            self.assembler.emit_btc_gpr_imm8_64(63, tmp)?;
            self.move_location(Size::S64, Location::GPR(tmp), ret)?;
            self.release_gpr(tmp);
        }
        Ok(())
    }
    fn f64_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        let c = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.move_location(Size::S64, loc, Location::GPR(tmp))?;
        self.move_location(
            Size::S64,
            Location::Imm64(0x7fffffffffffffffu64),
            Location::GPR(c),
        )?;
        self.assembler
            .emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp))?;
        self.move_location(Size::S64, Location::GPR(tmp), ret)?;

        self.release_gpr(c);
        self.release_gpr(tmp);
        Ok(())
    }
    fn emit_i64_copysign(&mut self, tmp1: GPR, tmp2: GPR) -> Result<(), CompileError> {
        let c = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;

        self.move_location(
            Size::S64,
            Location::Imm64(0x7fffffffffffffffu64),
            Location::GPR(c),
        )?;
        self.assembler
            .emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp1))?;

        self.move_location(
            Size::S64,
            Location::Imm64(0x8000000000000000u64),
            Location::GPR(c),
        )?;
        self.assembler
            .emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp2))?;

        self.assembler
            .emit_or(Size::S64, Location::GPR(tmp2), Location::GPR(tmp1))?;

        self.release_gpr(c);
        Ok(())
    }
    fn f64_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vsqrtsd, loc, loc, ret)
    }
    fn f64_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundsd_trunc, loc, loc, ret)
    }
    fn f64_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundsd_ceil, loc, loc, ret)
    }
    fn f64_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundsd_floor, loc, loc, ret)
    }
    fn f64_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundsd_nearest, loc, loc, ret)
    }
    fn f64_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpgesd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpgtsd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmplesd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpltsd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpneqsd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpeqsd, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f64_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if !self.arch_supports_canonicalize_nan() {
            self.emit_relaxed_avx(AssemblerX64::emit_vminsd, loc_a, loc_b, ret)
        } else {
            let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmpg1 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmpg2 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let src1 = match loc_a {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_a, Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                _ => {
                    codegen_error!("singlepass f64_min unreachable");
                }
            };
            let src2 = match loc_b {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_b, Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                _ => {
                    codegen_error!("singlepass f64_min unreachable");
                }
            };

            let tmp_xmm1 = XMM::XMM8;
            let tmp_xmm2 = XMM::XMM9;
            let tmp_xmm3 = XMM::XMM10;

            self.move_location(Size::S64, Location::SIMD(src1), Location::GPR(tmpg1))?;
            self.move_location(Size::S64, Location::SIMD(src2), Location::GPR(tmpg2))?;
            self.assembler
                .emit_cmp(Size::S64, Location::GPR(tmpg2), Location::GPR(tmpg1))?;
            self.assembler
                .emit_vminsd(src1, XMMOrMemory::XMM(src2), tmp_xmm1)?;
            let label1 = self.assembler.get_label();
            let label2 = self.assembler.get_label();
            self.assembler.emit_jmp(Condition::NotEqual, label1)?;
            self.assembler
                .emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2))?;
            self.assembler.emit_jmp(Condition::None, label2)?;
            self.emit_label(label1)?;
            // load float -0.0
            self.move_location(
                Size::S64,
                Location::Imm64(0x8000_0000_0000_0000), // Negative zero
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp_xmm2))?;
            self.emit_label(label2)?;
            self.assembler
                .emit_vcmpeqsd(src1, XMMOrMemory::XMM(src2), tmp_xmm3)?;
            self.assembler.emit_vblendvpd(
                tmp_xmm3,
                XMMOrMemory::XMM(tmp_xmm2),
                tmp_xmm1,
                tmp_xmm1,
            )?;
            self.assembler
                .emit_vcmpunordsd(src1, XMMOrMemory::XMM(src2), src1)?;
            // load float canonical nan
            self.move_location(
                Size::S64,
                Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(src2))?;
            self.assembler
                .emit_vblendvpd(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1)?;
            match ret {
                Location::SIMD(x) => {
                    self.assembler
                        .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x))?;
                }
                Location::Memory(_, _) | Location::GPR(_) => {
                    self.move_location(Size::S64, Location::SIMD(src1), ret)?;
                }
                _ => {
                    codegen_error!("singlepass f64_min unreachable");
                }
            }

            self.release_gpr(tmpg2);
            self.release_gpr(tmpg1);
            self.release_simd(tmp2);
            self.release_simd(tmp1);
            Ok(())
        }
    }
    fn f64_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if !self.arch_supports_canonicalize_nan() {
            self.emit_relaxed_avx(AssemblerX64::emit_vmaxsd, loc_a, loc_b, ret)
        } else {
            let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmpg1 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmpg2 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let src1 = match loc_a {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_a, Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                _ => {
                    codegen_error!("singlepass f64_max unreachable");
                }
            };
            let src2 = match loc_b {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_b, Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                _ => {
                    codegen_error!("singlepass f64_max unreachable");
                }
            };

            let tmp_xmm1 = XMM::XMM8;
            let tmp_xmm2 = XMM::XMM9;
            let tmp_xmm3 = XMM::XMM10;

            self.move_location(Size::S64, Location::SIMD(src1), Location::GPR(tmpg1))?;
            self.move_location(Size::S64, Location::SIMD(src2), Location::GPR(tmpg2))?;
            self.assembler
                .emit_cmp(Size::S64, Location::GPR(tmpg2), Location::GPR(tmpg1))?;
            self.assembler
                .emit_vmaxsd(src1, XMMOrMemory::XMM(src2), tmp_xmm1)?;
            let label1 = self.assembler.get_label();
            let label2 = self.assembler.get_label();
            self.assembler.emit_jmp(Condition::NotEqual, label1)?;
            self.assembler
                .emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2))?;
            self.assembler.emit_jmp(Condition::None, label2)?;
            self.emit_label(label1)?;
            self.assembler
                .emit_vxorpd(tmp_xmm2, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm2)?;
            self.emit_label(label2)?;
            self.assembler
                .emit_vcmpeqsd(src1, XMMOrMemory::XMM(src2), tmp_xmm3)?;
            self.assembler.emit_vblendvpd(
                tmp_xmm3,
                XMMOrMemory::XMM(tmp_xmm2),
                tmp_xmm1,
                tmp_xmm1,
            )?;
            self.assembler
                .emit_vcmpunordsd(src1, XMMOrMemory::XMM(src2), src1)?;
            // load float canonical nan
            self.move_location(
                Size::S64,
                Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(src2))?;
            self.assembler
                .emit_vblendvpd(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1)?;
            match ret {
                Location::SIMD(x) => {
                    self.assembler
                        .emit_vmovapd(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x))?;
                }
                Location::Memory(_, _) | Location::GPR(_) => {
                    self.move_location(Size::S64, Location::SIMD(src1), ret)?;
                }
                _ => {
                    codegen_error!("singlepass f64_max unreachable");
                }
            }

            self.release_gpr(tmpg2);
            self.release_gpr(tmpg1);
            self.release_simd(tmp2);
            self.release_simd(tmp1);
            Ok(())
        }
    }
    fn f64_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vaddsd, loc_a, loc_b, ret)
    }
    fn f64_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vsubsd, loc_a, loc_b, ret)
    }
    fn f64_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vmulsd, loc_a, loc_b, ret)
    }
    fn f64_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vdivsd, loc_a, loc_b, ret)
    }
    fn f32_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        if self.assembler.arch_has_fneg() {
            let tmp = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            self.emit_relaxed_mov(Size::S32, loc, Location::SIMD(tmp))?;
            self.assembler.arch_emit_f32_neg(tmp, tmp)?;
            self.emit_relaxed_mov(Size::S32, Location::SIMD(tmp), ret)?;
            self.release_simd(tmp);
        } else {
            let tmp = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            self.move_location(Size::S32, loc, Location::GPR(tmp))?;
            self.assembler.emit_btc_gpr_imm8_32(31, tmp)?;
            self.move_location(Size::S32, Location::GPR(tmp), ret)?;
            self.release_gpr(tmp);
        }
        Ok(())
    }
    fn f32_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        let tmp = self.acquire_temp_gpr().ok_or_else(|| {
            CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
        })?;
        self.move_location(Size::S32, loc, Location::GPR(tmp))?;
        self.assembler.emit_and(
            Size::S32,
            Location::Imm32(0x7fffffffu32),
            Location::GPR(tmp),
        )?;
        self.move_location(Size::S32, Location::GPR(tmp), ret)?;
        self.release_gpr(tmp);
        Ok(())
    }
    fn emit_i32_copysign(&mut self, tmp1: GPR, tmp2: GPR) -> Result<(), CompileError> {
        self.assembler.emit_and(
            Size::S32,
            Location::Imm32(0x7fffffffu32),
            Location::GPR(tmp1),
        )?;
        self.assembler.emit_and(
            Size::S32,
            Location::Imm32(0x80000000u32),
            Location::GPR(tmp2),
        )?;
        self.assembler
            .emit_or(Size::S32, Location::GPR(tmp2), Location::GPR(tmp1))
    }
    fn f32_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vsqrtss, loc, loc, ret)
    }
    fn f32_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundss_trunc, loc, loc, ret)
    }
    fn f32_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundss_ceil, loc, loc, ret)
    }
    fn f32_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundss_floor, loc, loc, ret)
    }
    fn f32_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vroundss_nearest, loc, loc, ret)
    }
    fn f32_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpgess, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpgtss, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpless, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpltss, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpneqss, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vcmpeqss, loc_a, loc_b, ret)?;
        self.assembler.emit_and(Size::S32, Location::Imm32(1), ret)
    }
    fn f32_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if !self.arch_supports_canonicalize_nan() {
            self.emit_relaxed_avx(AssemblerX64::emit_vminss, loc_a, loc_b, ret)
        } else {
            let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmpg1 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmpg2 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let src1 = match loc_a {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_a, Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                _ => {
                    codegen_error!("singlepass f32_min unreachable");
                }
            };
            let src2 = match loc_b {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_b, Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                _ => {
                    codegen_error!("singlepass f32_min unreachable");
                }
            };

            let tmp_xmm1 = XMM::XMM8;
            let tmp_xmm2 = XMM::XMM9;
            let tmp_xmm3 = XMM::XMM10;

            self.move_location(Size::S32, Location::SIMD(src1), Location::GPR(tmpg1))?;
            self.move_location(Size::S32, Location::SIMD(src2), Location::GPR(tmpg2))?;
            self.assembler
                .emit_cmp(Size::S32, Location::GPR(tmpg2), Location::GPR(tmpg1))?;
            self.assembler
                .emit_vminss(src1, XMMOrMemory::XMM(src2), tmp_xmm1)?;
            let label1 = self.assembler.get_label();
            let label2 = self.assembler.get_label();
            self.assembler.emit_jmp(Condition::NotEqual, label1)?;
            self.assembler
                .emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2))?;
            self.assembler.emit_jmp(Condition::None, label2)?;
            self.emit_label(label1)?;
            // load float -0.0
            self.move_location(
                Size::S64,
                Location::Imm32(0x8000_0000), // Negative zero
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp_xmm2))?;
            self.emit_label(label2)?;
            self.assembler
                .emit_vcmpeqss(src1, XMMOrMemory::XMM(src2), tmp_xmm3)?;
            self.assembler.emit_vblendvps(
                tmp_xmm3,
                XMMOrMemory::XMM(tmp_xmm2),
                tmp_xmm1,
                tmp_xmm1,
            )?;
            self.assembler
                .emit_vcmpunordss(src1, XMMOrMemory::XMM(src2), src1)?;
            // load float canonical nan
            self.move_location(
                Size::S64,
                Location::Imm32(0x7FC0_0000), // Canonical NaN
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(src2))?;
            self.assembler
                .emit_vblendvps(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1)?;
            match ret {
                Location::SIMD(x) => {
                    self.assembler
                        .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x))?;
                }
                Location::Memory(_, _) | Location::GPR(_) => {
                    self.move_location(Size::S64, Location::SIMD(src1), ret)?;
                }
                _ => {
                    codegen_error!("singlepass f32_min unreachable");
                }
            }

            self.release_gpr(tmpg2);
            self.release_gpr(tmpg1);
            self.release_simd(tmp2);
            self.release_simd(tmp1);
            Ok(())
        }
    }
    fn f32_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        if !self.arch_supports_canonicalize_nan() {
            self.emit_relaxed_avx(AssemblerX64::emit_vmaxss, loc_a, loc_b, ret)
        } else {
            let tmp1 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmp2 = self.acquire_temp_simd().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp simd".to_owned())
            })?;
            let tmpg1 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;
            let tmpg2 = self.acquire_temp_gpr().ok_or_else(|| {
                CompileError::Codegen("singlepass cannot acquire temp gpr".to_owned())
            })?;

            let src1 = match loc_a {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_a, Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_a, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp1))?;
                    tmp1
                }
                _ => {
                    codegen_error!("singlepass f32_max unreachable");
                }
            };
            let src2 = match loc_b {
                Location::SIMD(x) => x,
                Location::GPR(_) | Location::Memory(_, _) => {
                    self.move_location(Size::S64, loc_b, Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm32(_) => {
                    self.move_location(Size::S32, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S32, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                Location::Imm64(_) => {
                    self.move_location(Size::S64, loc_b, Location::GPR(tmpg1))?;
                    self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp2))?;
                    tmp2
                }
                _ => {
                    codegen_error!("singlepass f32_max unreachable");
                }
            };

            let tmp_xmm1 = XMM::XMM8;
            let tmp_xmm2 = XMM::XMM9;
            let tmp_xmm3 = XMM::XMM10;

            self.move_location(Size::S32, Location::SIMD(src1), Location::GPR(tmpg1))?;
            self.move_location(Size::S32, Location::SIMD(src2), Location::GPR(tmpg2))?;
            self.assembler
                .emit_cmp(Size::S32, Location::GPR(tmpg2), Location::GPR(tmpg1))?;
            self.assembler
                .emit_vmaxss(src1, XMMOrMemory::XMM(src2), tmp_xmm1)?;
            let label1 = self.assembler.get_label();
            let label2 = self.assembler.get_label();
            self.assembler.emit_jmp(Condition::NotEqual, label1)?;
            self.assembler
                .emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2))?;
            self.assembler.emit_jmp(Condition::None, label2)?;
            self.emit_label(label1)?;
            self.assembler
                .emit_vxorps(tmp_xmm2, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm2)?;
            self.emit_label(label2)?;
            self.assembler
                .emit_vcmpeqss(src1, XMMOrMemory::XMM(src2), tmp_xmm3)?;
            self.assembler.emit_vblendvps(
                tmp_xmm3,
                XMMOrMemory::XMM(tmp_xmm2),
                tmp_xmm1,
                tmp_xmm1,
            )?;
            self.assembler
                .emit_vcmpunordss(src1, XMMOrMemory::XMM(src2), src1)?;
            // load float canonical nan
            self.move_location(
                Size::S64,
                Location::Imm32(0x7FC0_0000), // Canonical NaN
                Location::GPR(tmpg1),
            )?;
            self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(src2))?;
            self.assembler
                .emit_vblendvps(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1)?;
            match ret {
                Location::SIMD(x) => {
                    self.assembler
                        .emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x))?;
                }
                Location::Memory(_, _) | Location::GPR(_) => {
                    self.move_location(Size::S64, Location::SIMD(src1), ret)?;
                }
                _ => {
                    codegen_error!("singlepass f32_max unreachable");
                }
            }

            self.release_gpr(tmpg2);
            self.release_gpr(tmpg1);
            self.release_simd(tmp2);
            self.release_simd(tmp1);
            Ok(())
        }
    }
    fn f32_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vaddss, loc_a, loc_b, ret)
    }
    fn f32_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vsubss, loc_a, loc_b, ret)
    }
    fn f32_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vmulss, loc_a, loc_b, ret)
    }
    fn f32_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        self.emit_relaxed_avx(AssemblerX64::emit_vdivss, loc_a, loc_b, ret)
    }

    fn gen_std_trampoline(
        &self,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        // the cpu feature here is irrelevant
        let mut a = AssemblerX64::new(0, None)?;

        // Calculate stack offset.
        let mut stack_offset: u32 = 0;
        for (i, _param) in sig.params().iter().enumerate() {
            if let Location::Memory(_, _) =
                self.get_simple_param_location(1 + i, calling_convention)
            {
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
        a.emit_push(Size::S64, Location::GPR(GPR::R15))?;
        a.emit_push(Size::S64, Location::GPR(GPR::R14))?;

        // Prepare stack space.
        a.emit_sub(
            Size::S64,
            Location::Imm32(stack_offset + stack_padding),
            Location::GPR(GPR::RSP),
        )?;

        // Arguments
        a.emit_mov(
            Size::S64,
            self.get_simple_param_location(1, calling_convention),
            Location::GPR(GPR::R15),
        )?; // func_ptr
        a.emit_mov(
            Size::S64,
            self.get_simple_param_location(2, calling_convention),
            Location::GPR(GPR::R14),
        )?; // args_rets

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        {
            let mut n_stack_args: usize = 0;
            for (i, _param) in sig.params().iter().enumerate() {
                let src_loc = Location::Memory(GPR::R14, (i * 16) as _); // args_rets[i]
                let dst_loc = self.get_simple_param_location(1 + i, calling_convention);

                match dst_loc {
                    Location::GPR(_) => {
                        a.emit_mov(Size::S64, src_loc, dst_loc)?;
                    }
                    Location::Memory(_, _) => {
                        // This location is for reading arguments but we are writing arguments here.
                        // So recalculate it.
                        a.emit_mov(Size::S64, src_loc, Location::GPR(GPR::RAX))?;
                        a.emit_mov(
                            Size::S64,
                            Location::GPR(GPR::RAX),
                            Location::Memory(
                                GPR::RSP,
                                (stack_padding as usize + n_stack_args * 8) as _,
                            ),
                        )?;
                        n_stack_args += 1;
                    }
                    _ => codegen_error!("singlepass gen_std_trampoline unreachable"),
                }
            }
        }

        // Call.
        a.emit_call_location(Location::GPR(GPR::R15))?;

        // Restore stack.
        a.emit_add(
            Size::S64,
            Location::Imm32(stack_offset + stack_padding),
            Location::GPR(GPR::RSP),
        )?;

        // Write return value.
        if !sig.results().is_empty() {
            a.emit_mov(
                Size::S64,
                Location::GPR(GPR::RAX),
                Location::Memory(GPR::R14, 0),
            )?;
        }

        // Restore callee-saved registers.
        a.emit_pop(Size::S64, Location::GPR(GPR::R14))?;
        a.emit_pop(Size::S64, Location::GPR(GPR::R15))?;

        a.emit_ret()?;

        let mut body = a.finalize().unwrap();
        body.shrink_to_fit();
        Ok(FunctionBody {
            body,
            unwind_info: None,
        })
    }
    // Generates dynamic import function call trampoline for a function type.
    fn gen_std_dynamic_import_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        // the cpu feature here is irrelevant
        let mut a = AssemblerX64::new(0, None)?;

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
        )?;

        // Copy arguments.
        if !sig.params().is_empty() {
            let mut argalloc = ArgumentRegisterAllocator::default();
            argalloc.next(Type::I64, calling_convention).unwrap(); // skip VMContext

            let mut stack_param_count: usize = 0;

            for (i, ty) in sig.params().iter().enumerate() {
                let source_loc = match argalloc.next(*ty, calling_convention)? {
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
                        )?;
                        stack_param_count += 1;
                        Location::GPR(GPR::RAX)
                    }
                };
                a.emit_mov(
                    Size::S64,
                    source_loc,
                    Location::Memory(GPR::RSP, (stack_padding + i * 16) as _),
                )?;

                // Zero upper 64 bits.
                a.emit_mov(
                    Size::S64,
                    Location::Imm32(0),
                    Location::Memory(GPR::RSP, (stack_padding + i * 16 + 8) as _),
                )?;
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
                )?;
                // Load values array.
                a.emit_lea(
                    Size::S64,
                    Location::Memory(GPR::RSP, stack_padding as i32),
                    Location::GPR(GPR::RDX),
                )?;
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
                )?;
                // Load values array.
                a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RSI))?;
            }
        };

        // Call target.
        a.emit_call_location(Location::GPR(GPR::RAX))?;

        // Fetch return value.
        if !sig.results().is_empty() {
            assert_eq!(sig.results().len(), 1);
            a.emit_mov(
                Size::S64,
                Location::Memory(GPR::RSP, stack_padding as i32),
                Location::GPR(GPR::RAX),
            )?;
        }

        // Release values array.
        a.emit_add(
            Size::S64,
            Location::Imm32((stack_offset + stack_padding) as _),
            Location::GPR(GPR::RSP),
        )?;

        // Return.
        a.emit_ret()?;

        let mut body = a.finalize().unwrap();
        body.shrink_to_fit();
        Ok(FunctionBody {
            body,
            unwind_info: None,
        })
    }
    // Singlepass calls import functions through a trampoline.
    fn gen_import_call_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<CustomSection, CompileError> {
        // the cpu feature here is irrelevant
        let mut a = AssemblerX64::new(0, None)?;

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
                    static PARAM_REGS: &[GPR] = &[GPR::RDX, GPR::R8, GPR::R9];
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..sig.params().len() {
                        let loc = match i {
                            0..=2 => Location::GPR(PARAM_REGS[i]),
                            _ => Location::Memory(GPR::RSP, 32 + 8 + ((i - 3) * 8) as i32), // will not be used anyway
                        };
                        param_locations.push(loc);
                    }

                    // Copy Float arguments to XMM from GPR.
                    let mut argalloc = ArgumentRegisterAllocator::default();
                    for (i, ty) in sig.params().iter().enumerate() {
                        let prev_loc = param_locations[i];
                        match argalloc.next(*ty, calling_convention)? {
                            Some(X64Register::GPR(_gpr)) => continue,
                            Some(X64Register::XMM(xmm)) => {
                                a.emit_mov(Size::S64, prev_loc, Location::SIMD(xmm))?
                            }
                            None => continue,
                        };
                    }
                }
                _ => {
                    let mut param_locations = vec![];

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
                        )?;
                    }

                    // Store all arguments to the stack to prevent overwrite.
                    static PARAM_REGS: &[GPR] = &[GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9];
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..sig.params().len() {
                        let loc = match i {
                            0..=4 => {
                                let loc = Location::Memory(GPR::RSP, (i * 8) as i32);
                                a.emit_mov(Size::S64, Location::GPR(PARAM_REGS[i]), loc)?;
                                loc
                            }
                            _ => {
                                Location::Memory(GPR::RSP, stack_offset + 8 + ((i - 5) * 8) as i32)
                            }
                        };
                        param_locations.push(loc);
                    }

                    // Copy arguments.
                    let mut argalloc = ArgumentRegisterAllocator::default();
                    argalloc.next(Type::I64, calling_convention)?.unwrap(); // skip VMContext
                    let mut caller_stack_offset: i32 = 0;
                    for (i, ty) in sig.params().iter().enumerate() {
                        let prev_loc = param_locations[i];
                        let targ = match argalloc.next(*ty, calling_convention)? {
                            Some(X64Register::GPR(gpr)) => Location::GPR(gpr),
                            Some(X64Register::XMM(xmm)) => Location::SIMD(xmm),
                            None => {
                                // No register can be allocated. Put this argument on the stack.
                                //
                                // Since here we never use fewer registers than by the original call, on the caller's frame
                                // we always have enough space to store the rearranged arguments, and the copy "backward" between different
                                // slots in the caller argument region will always work.
                                a.emit_mov(Size::S64, prev_loc, Location::GPR(GPR::RAX))?;
                                a.emit_mov(
                                    Size::S64,
                                    Location::GPR(GPR::RAX),
                                    Location::Memory(
                                        GPR::RSP,
                                        stack_offset + 8 + caller_stack_offset,
                                    ),
                                )?;
                                caller_stack_offset += 8;
                                continue;
                            }
                        };
                        a.emit_mov(Size::S64, prev_loc, targ)?;
                    }

                    // Restore stack pointer.
                    if stack_offset > 0 {
                        a.emit_add(
                            Size::S64,
                            Location::Imm32(stack_offset as u32),
                            Location::GPR(GPR::RSP),
                        )?;
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
                )?;
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RCX, offset as i32 + 8), // target vmctx
                    Location::GPR(GPR::RCX),
                )?;
            }
            _ => {
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RDI, offset as i32), // function pointer
                    Location::GPR(GPR::RAX),
                )?;
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RDI, offset as i32 + 8), // target vmctx
                    Location::GPR(GPR::RDI),
                )?;
            }
        }
        a.emit_host_redirection(GPR::RAX)?;

        let mut contents = a.finalize().unwrap();
        contents.shrink_to_fit();
        let section_body = SectionBody::new_with_vec(contents);

        Ok(CustomSection {
            protection: CustomSectionProtection::ReadExecute,
            alignment: None,
            bytes: section_body,
            relocations: vec![],
        })
    }
    #[cfg(feature = "unwind")]
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> {
        let mut instructions = vec![];
        for &(instruction_offset, ref inst) in &self.unwind_ops {
            let instruction_offset = instruction_offset as u32;
            match *inst {
                UnwindOps::PushFP { up_to_sp } => {
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::CfaOffset(up_to_sp as i32),
                    ));
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::Offset(X86_64::RBP, -(up_to_sp as i32)),
                    ));
                }
                UnwindOps::DefineNewFrame => {
                    instructions.push((
                        instruction_offset,
                        CallFrameInstruction::CfaRegister(X86_64::RBP),
                    ));
                }
                UnwindOps::SaveRegister { reg, bp_neg_offset } => instructions.push((
                    instruction_offset,
                    CallFrameInstruction::Offset(reg.dwarf_index(), -bp_neg_offset),
                )),
                UnwindOps::Push2Regs { .. } => unimplemented!(),
            }
        }
        Some(UnwindInstructions {
            instructions,
            len: code_len as u32,
        })
    }
    #[cfg(not(feature = "unwind"))]
    fn gen_dwarf_unwind_info(&mut self, _code_len: usize) -> Option<UnwindInstructions> {
        None
    }

    #[cfg(feature = "unwind")]
    fn gen_windows_unwind_info(&mut self, _code_len: usize) -> Option<Vec<u8>> {
        let unwind_info = create_unwind_info_from_insts(&self.unwind_ops);
        if let Some(unwind) = unwind_info {
            let sz = unwind.emit_size();
            let mut tbl = vec![0; sz];
            unwind.emit(&mut tbl);
            Some(tbl)
        } else {
            None
        }
    }

    #[cfg(not(feature = "unwind"))]
    fn gen_windows_unwind_info(&mut self, _code_len: usize) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use enumset::enum_set;
    use std::str::FromStr;
    use wasmer_types::target::{CpuFeature, Target, Triple};

    fn test_move_location(machine: &mut MachineX86_64) -> Result<(), CompileError> {
        machine.move_location_for_native(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RCX),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, 10),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, -10),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::Memory(GPR::RDX, 10),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::Imm64(50),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::Imm32(50),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(Size::S64, Location::Imm8(50), Location::GPR(GPR::RAX))?;

        machine.move_location_for_native(
            Size::S32,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RCX),
        )?;
        machine.move_location_for_native(
            Size::S32,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, 10),
        )?;
        machine.move_location_for_native(
            Size::S32,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, -10),
        )?;
        machine.move_location_for_native(
            Size::S32,
            Location::Memory(GPR::RDX, 10),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(
            Size::S32,
            Location::Imm32(50),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(Size::S32, Location::Imm8(50), Location::GPR(GPR::RAX))?;

        machine.move_location_for_native(
            Size::S16,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RCX),
        )?;
        machine.move_location_for_native(
            Size::S16,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, 10),
        )?;
        machine.move_location_for_native(
            Size::S16,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, -10),
        )?;
        machine.move_location_for_native(
            Size::S16,
            Location::Memory(GPR::RDX, 10),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(Size::S16, Location::Imm8(50), Location::GPR(GPR::RAX))?;

        machine.move_location_for_native(
            Size::S8,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RCX),
        )?;
        machine.move_location_for_native(
            Size::S8,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, 10),
        )?;
        machine.move_location_for_native(
            Size::S8,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, -10),
        )?;
        machine.move_location_for_native(
            Size::S8,
            Location::Memory(GPR::RDX, 10),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(Size::S8, Location::Imm8(50), Location::GPR(GPR::RAX))?;

        machine.move_location_for_native(
            Size::S64,
            Location::SIMD(XMM::XMM0),
            Location::GPR(GPR::RAX),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::SIMD(XMM::XMM0),
            Location::Memory(GPR::RDX, -10),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::SIMD(XMM::XMM0),
        )?;
        machine.move_location_for_native(
            Size::S64,
            Location::Memory(GPR::RDX, -10),
            Location::SIMD(XMM::XMM0),
        )?;

        Ok(())
    }

    fn test_move_location_extended(
        machine: &mut MachineX86_64,
        signed: bool,
        sized: Size,
    ) -> Result<(), CompileError> {
        machine.move_location_extend(
            sized,
            signed,
            Location::GPR(GPR::RAX),
            Size::S64,
            Location::GPR(GPR::RCX),
        )?;
        machine.move_location_extend(
            sized,
            signed,
            Location::GPR(GPR::RAX),
            Size::S64,
            Location::Memory(GPR::RCX, 10),
        )?;
        machine.move_location_extend(
            sized,
            signed,
            Location::Memory(GPR::RAX, 10),
            Size::S64,
            Location::GPR(GPR::RCX),
        )?;
        if sized != Size::S32 {
            machine.move_location_extend(
                sized,
                signed,
                Location::GPR(GPR::RAX),
                Size::S32,
                Location::GPR(GPR::RCX),
            )?;
            machine.move_location_extend(
                sized,
                signed,
                Location::GPR(GPR::RAX),
                Size::S32,
                Location::Memory(GPR::RCX, 10),
            )?;
            machine.move_location_extend(
                sized,
                signed,
                Location::Memory(GPR::RAX, 10),
                Size::S32,
                Location::GPR(GPR::RCX),
            )?;
        }

        Ok(())
    }

    fn test_binop_op(
        machine: &mut MachineX86_64,
        op: fn(&mut MachineX86_64, Location, Location, Location) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        op(
            machine,
            Location::GPR(GPR::RDX),
            Location::GPR(GPR::RDX),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::GPR(GPR::RDX),
            Location::Imm32(10),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::Imm32(10),
            Location::GPR(GPR::RDX),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::GPR(GPR::RAX),
            Location::GPR(GPR::RDX),
            Location::Memory(GPR::RAX, 10),
        )?;
        op(
            machine,
            Location::GPR(GPR::RAX),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 10),
        )?;
        op(
            machine,
            Location::Memory(GPR::RAX, 0),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 10),
        )?;

        Ok(())
    }

    fn test_float_binop_op(
        machine: &mut MachineX86_64,
        op: fn(&mut MachineX86_64, Location, Location, Location) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        op(
            machine,
            Location::SIMD(XMM::XMM3),
            Location::SIMD(XMM::XMM2),
            Location::SIMD(XMM::XMM0),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM2),
            Location::SIMD(XMM::XMM0),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM0),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::SIMD(XMM::XMM2),
            Location::SIMD(XMM::XMM0),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::Memory(GPR::RDX, 10),
            Location::SIMD(XMM::XMM0),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 32),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 32),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM1),
            Location::Memory(GPR::RAX, 32),
        )?;

        Ok(())
    }

    fn test_float_cmp_op(
        machine: &mut MachineX86_64,
        op: fn(&mut MachineX86_64, Location, Location, Location) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        op(
            machine,
            Location::SIMD(XMM::XMM3),
            Location::SIMD(XMM::XMM2),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM0),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::SIMD(XMM::XMM2),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::Memory(GPR::RDX, 10),
            Location::GPR(GPR::RAX),
        )?;
        op(
            machine,
            Location::Memory(GPR::RBP, 0),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 32),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::Memory(GPR::RDX, 16),
            Location::Memory(GPR::RAX, 32),
        )?;
        op(
            machine,
            Location::SIMD(XMM::XMM0),
            Location::SIMD(XMM::XMM1),
            Location::Memory(GPR::RAX, 32),
        )?;

        Ok(())
    }

    #[test]
    fn tests_avx() -> Result<(), CompileError> {
        let set = enum_set!(CpuFeature::AVX);
        let target = Target::new(Triple::from_str("x86_64-linux-gnu").unwrap(), set);
        let mut machine = MachineX86_64::new(Some(target))?;

        test_move_location(&mut machine)?;
        test_move_location_extended(&mut machine, false, Size::S8)?;
        test_move_location_extended(&mut machine, false, Size::S16)?;
        test_move_location_extended(&mut machine, false, Size::S32)?;
        test_move_location_extended(&mut machine, true, Size::S8)?;
        test_move_location_extended(&mut machine, true, Size::S16)?;
        test_move_location_extended(&mut machine, true, Size::S32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_add32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_add64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_sub32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_sub64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_and32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_and64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_xor32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_xor64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_or32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_or64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_mul32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_mul64)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_add)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_sub)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_mul)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_div)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_eq)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_lt)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_le)?;

        Ok(())
    }

    #[test]
    fn tests_sse42() -> Result<(), CompileError> {
        let set = enum_set!(CpuFeature::SSE42);
        let target = Target::new(Triple::from_str("x86_64-linux-gnu").unwrap(), set);
        let mut machine = MachineX86_64::new(Some(target))?;

        test_move_location(&mut machine)?;
        test_move_location_extended(&mut machine, false, Size::S8)?;
        test_move_location_extended(&mut machine, false, Size::S16)?;
        test_move_location_extended(&mut machine, false, Size::S32)?;
        test_move_location_extended(&mut machine, true, Size::S8)?;
        test_move_location_extended(&mut machine, true, Size::S16)?;
        test_move_location_extended(&mut machine, true, Size::S32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_add32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_add64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_sub32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_sub64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_and32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_and64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_xor32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_xor64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_or32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_or64)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_mul32)?;
        test_binop_op(&mut machine, MachineX86_64::emit_binop_mul64)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_add)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_sub)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_mul)?;
        test_float_binop_op(&mut machine, MachineX86_64::f32_div)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_eq)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_lt)?;
        test_float_cmp_op(&mut machine, MachineX86_64::f32_cmp_le)?;

        Ok(())
    }
}
