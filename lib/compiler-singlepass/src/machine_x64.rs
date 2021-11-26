use crate::common_decl::*;
use crate::emitter_x64::*;
use crate::machine::Machine as AbstractMachine;
use crate::machine::{MachineSpecific, MemoryImmediate};
use crate::x64_decl::new_machine_state;
use crate::x64_decl::{X64Register, GPR, XMM};
use dynasmrt::x64::Assembler;
use std::collections::HashSet;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer_compiler::CallingConvention;

pub struct MachineX86_64 {
    pub assembler: Assembler, //temporary public
    used_gprs: HashSet<GPR>,
    used_simd: HashSet<XMM>,
}

impl MachineX86_64 {
    pub fn emit_relaxed_binop(
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
                let temp = self.acquire_temp_gpr().unwrap();
                self.move_location(sz, src, Location::GPR(temp));
                op(&mut self.assembler, sz, Location::GPR(temp), dst);
                self.release_gpr(temp);
            }
            RelaxMode::DstToGPR => {
                let temp = self.acquire_temp_gpr().unwrap();
                self.move_location(sz, dst, Location::GPR(temp));
                op(&mut self.assembler, sz, src, Location::GPR(temp));
                self.release_gpr(temp);
            }
            RelaxMode::BothToGPR => {
                let temp_src = self.acquire_temp_gpr().unwrap();
                let temp_dst = self.acquire_temp_gpr().unwrap();
                self.move_location(sz, src, Location::GPR(temp_src));
                self.move_location(sz, dst, Location::GPR(temp_dst));
                op(
                    &mut self.assembler,
                    sz,
                    Location::GPR(temp_src),
                    Location::GPR(temp_dst),
                );
                match dst {
                    Location::Memory(_, _) | Location::GPR(_) => {
                        self.move_location(sz, Location::GPR(temp_dst), dst);
                    }
                    _ => {}
                }
                self.release_gpr(temp_dst);
                self.release_gpr(temp_src);
            }
            RelaxMode::Direct => {
                op(&mut self.assembler, sz, src, dst);
            }
        }
    }
    pub fn emit_relaxed_zx_sx(
        &mut self,
        op: fn(&mut Assembler, Size, Location, Size, Location),
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) {
        match src {
            Location::Imm32(_) | Location::Imm64(_) => {
                let tmp_src = self.acquire_temp_gpr().unwrap();
                self.assembler
                    .emit_mov(Size::S64, src, Location::GPR(tmp_src));
                let src = Location::GPR(tmp_src);

                match dst {
                    Location::Imm32(_) | Location::Imm64(_) => unreachable!(),
                    Location::Memory(_, _) => {
                        let tmp_dst = self.acquire_temp_gpr().unwrap();
                        op(
                            &mut self.assembler,
                            sz_src,
                            src,
                            sz_dst,
                            Location::GPR(tmp_dst),
                        );
                        self.move_location(Size::S64, Location::GPR(tmp_dst), dst);

                        self.release_gpr(tmp_dst);
                    }
                    Location::GPR(_) => {
                        op(&mut self.assembler, sz_src, src, sz_dst, dst);
                    }
                    _ => {
                        unreachable!();
                    }
                };

                self.release_gpr(tmp_src);
            }
            Location::GPR(_) | Location::Memory(_, _) => {
                match dst {
                    Location::Imm32(_) | Location::Imm64(_) => unreachable!(),
                    Location::Memory(_, _) => {
                        let tmp_dst = self.acquire_temp_gpr().unwrap();
                        op(
                            &mut self.assembler,
                            sz_src,
                            src,
                            sz_dst,
                            Location::GPR(tmp_dst),
                        );
                        self.move_location(Size::S64, Location::GPR(tmp_dst), dst);

                        self.release_gpr(tmp_dst);
                    }
                    Location::GPR(_) => {
                        op(&mut self.assembler, sz_src, src, sz_dst, dst);
                    }
                    _ => {
                        unreachable!();
                    }
                };
            }
            _ => {
                unreachable!();
            }
        }
    }
}

impl MachineSpecific<GPR, XMM> for MachineX86_64 {
    fn new() -> Self {
        MachineX86_64 {
            assembler: Assembler::new().unwrap(),
            used_gprs: HashSet::new(),
            used_simd: HashSet::new(),
        }
    }
    fn get_vmctx_reg() -> GPR {
        GPR::R15
    }

    fn get_used_gprs(&self) -> Vec<GPR> {
        self.used_gprs.iter().cloned().collect()
    }

    fn get_used_simd(&self) -> Vec<XMM> {
        self.used_simd.iter().cloned().collect()
    }

    fn pick_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[RSI, RDI, R8, R9, R10, R11];
        for r in REGS {
            if !self.used_gprs.contains(r) {
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
            if !self.used_gprs.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    fn acquire_temp_gpr(&mut self) -> Option<GPR> {
        let gpr = self.pick_temp_gpr();
        if let Some(x) = gpr {
            self.used_gprs.insert(x);
        }
        gpr
    }

    fn release_gpr(&mut self, gpr: GPR) {
        assert!(self.used_gprs.remove(&gpr));
    }

    fn reserve_unused_temp_gpr(&mut self, gpr: GPR) -> GPR {
        assert!(!self.used_gprs.contains(&gpr));
        self.used_gprs.insert(gpr);
        gpr
    }

    fn reserve_gpr(&mut self, gpr: GPR) {
        self.used_gprs.insert(gpr);
    }

    fn get_cmpxchg_temp_gprs(&self) -> Vec<GPR> {
        vec![GPR::RAX]
    }

    fn reserve_cmpxchg_temp_gpr(&mut self) {
        for gpr in self.get_cmpxchg_temp_gprs().iter() {
            assert!(!self.used_gprs.contains(gpr));
            self.used_gprs.insert(*gpr);
        }
    }

    fn get_xchg_temp_gprs(&self) -> Vec<GPR> {
        vec![]
    }

    fn release_xchg_temp_gpr(&mut self) {
        for gpr in self.get_xchg_temp_gprs().iter() {
            assert_eq!(!self.used_gprs.remove(gpr), true);
        }
    }

    fn reserve_xchg_temp_gpr(&mut self) {
        for gpr in self.get_xchg_temp_gprs().iter() {
            assert!(!self.used_gprs.contains(gpr));
            self.used_gprs.insert(*gpr);
        }
    }

    fn release_cmpxchg_temp_gpr(&mut self) {
        for gpr in self.get_cmpxchg_temp_gprs().iter() {
            assert_eq!(!self.used_gprs.remove(gpr), true);
        }
    }

    // Picks an unused XMM register.
    fn pick_simd(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &[XMM] = &[XMM3, XMM4, XMM5, XMM6, XMM7];
        for r in REGS {
            if !self.used_simd.contains(r) {
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
            if !self.used_simd.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Acquires a temporary XMM register.
    fn acquire_temp_simd(&mut self) -> Option<XMM> {
        let simd = self.pick_temp_simd();
        if let Some(x) = simd {
            self.used_simd.insert(x);
        }
        simd
    }

    fn reserve_simd(&mut self, simd: XMM) {
        self.used_simd.insert(simd);
    }

    // Releases a temporary XMM register.
    fn release_simd(&mut self, simd: XMM) {
        assert_eq!(self.used_simd.remove(&simd), true);
    }

    // Memory location for a local on the stack
    fn local_on_stack(&mut self, stack_offset: i32) -> Location {
        Location::Memory(GPR::RBP, -stack_offset)
    }

    // Adjust stack for locals
    fn adjust_stack(&mut self, delta_stack_offset: u32) {
        self.assembler.emit_sub(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        );
    }
    // Pop stack of locals
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) {
        self.assembler.emit_add(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        );
    }

    // Zero a location that is 32bits
    fn zero_location(&mut self, size: Size, location: Location) {
        self.assembler.emit_mov(size, Location::Imm32(0), location);
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
    fn move_local(&mut self, stack_offset: i32, location: Location) {
        self.assembler.emit_mov(
            Size::S64,
            location,
            Location::Memory(GPR::RBP, -stack_offset),
        );
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

    // Get param location
    fn get_param_location(idx: usize, calling_convention: CallingConvention) -> Location {
        match calling_convention {
            CallingConvention::WindowsFastcall => match idx {
                0 => Location::GPR(GPR::RCX),
                1 => Location::GPR(GPR::RDX),
                2 => Location::GPR(GPR::R8),
                3 => Location::GPR(GPR::R9),
                _ => Location::Memory(GPR::RBP, (16 + 32 + (idx - 4) * 8) as i32),
            },
            _ => match idx {
                0 => Location::GPR(GPR::RDI),
                1 => Location::GPR(GPR::RSI),
                2 => Location::GPR(GPR::RDX),
                3 => Location::GPR(GPR::RCX),
                4 => Location::GPR(GPR::R8),
                5 => Location::GPR(GPR::R9),
                _ => Location::Memory(GPR::RBP, (16 + (idx - 6) * 8) as i32),
            },
        }
    }
    // move a location to another
    fn move_location(&mut self, size: Size, source: Location, dest: Location) {
        match source {
            Location::GPR(_) => {
                self.assembler.emit_mov(size, source, dest);
            }
            Location::Memory(_, _) => match dest {
                Location::GPR(_) | Location::SIMD(_) => {
                    self.assembler.emit_mov(size, source, dest);
                }
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    self.assembler
                        .emit_mov(size, source, Location::GPR(GPR::RAX));
                    self.assembler.emit_mov(size, Location::GPR(GPR::RAX), dest);
                }
                _ => unreachable!(),
            },
            Location::Memory2(_, _, _, _) => match dest {
                Location::GPR(_) | Location::SIMD(_) => {
                    self.assembler.emit_mov(size, source, dest);
                }
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    self.assembler
                        .emit_mov(size, source, Location::GPR(GPR::RAX));
                    self.assembler.emit_mov(size, Location::GPR(GPR::RAX), dest);
                }
                _ => unreachable!(),
            },
            Location::Imm8(_) | Location::Imm32(_) | Location::Imm64(_) => match dest {
                Location::GPR(_) | Location::SIMD(_) => {
                    self.assembler.emit_mov(size, source, dest);
                }
                Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                    self.assembler
                        .emit_mov(size, source, Location::GPR(GPR::RAX));
                    self.assembler.emit_mov(size, Location::GPR(GPR::RAX), dest);
                }
                _ => unreachable!(),
            },
            Location::SIMD(_) => {
                self.assembler.emit_mov(size, source, dest);
            }
            _ => unreachable!(),
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
    ) {
        match source {
            Location::GPR(_) | Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                match size_val {
                    Size::S32 | Size::S64 => self.assembler.emit_mov(size_val, source, dest),
                    Size::S16 | Size::S8 => {
                        if signed {
                            self.assembler.emit_movsx(size_val, source, size_op, dest)
                        } else {
                            self.assembler.emit_movzx(size_val, source, size_op, dest)
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }
    fn load_address(&mut self, size: Size, reg: Location, mem: Location) {
        match reg {
            Location::GPR(_) => {
                match mem {
                    Location::Memory(_, _) | Location::Memory2(_, _, _, _) => {
                        // Memory moves with size < 32b do not zero upper bits.
                        if size < Size::S32 {
                            self.assembler.emit_xor(Size::S32, reg, reg);
                        }
                        self.assembler.emit_mov(size, mem, reg);
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }
    // Init the stack loc counter
    fn init_stack_loc(&mut self, init_stack_loc_cnt: u64, last_stack_loc: Location) {
        // Since these assemblies take up to 24 bytes, if more than 2 slots are initialized, then they are smaller.
        self.assembler.emit_mov(
            Size::S64,
            Location::Imm64(init_stack_loc_cnt),
            Location::GPR(GPR::RCX),
        );
        self.assembler
            .emit_xor(Size::S64, Location::GPR(GPR::RAX), Location::GPR(GPR::RAX));
        self.assembler
            .emit_lea(Size::S64, last_stack_loc, Location::GPR(GPR::RDI));
        self.assembler.emit_rep_stosq();
    }
    // Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32) {
        self.assembler.emit_lea(
            Size::S64,
            Location::Memory(GPR::RBP, -saved_area_offset),
            Location::GPR(GPR::RSP),
        );
    }
    // Pop a location
    fn pop_location(&mut self, location: Location) {
        self.assembler.emit_pop(Size::S64, location);
    }
    // Create a new `MachineState` with default values.
    fn new_machine_state() -> MachineState {
        new_machine_state()
    }

    // assembler finalize
    fn assembler_finalize(self) -> Vec<u8> {
        self.assembler.finalize().unwrap().to_vec()
    }

    fn get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }

    fn finalize_function(&mut self) {
        self.assembler.finalize_function();
    }

    fn emit_function_prolog(&mut self) {
        self.emit_push(Size::S64, Location::GPR(GPR::RBP));
        self.move_location(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RBP));
    }

    fn emit_function_epilog(&mut self) {
        self.move_location(Size::S64, Location::GPR(GPR::RBP), Location::GPR(GPR::RSP));
        self.emit_pop(Size::S64, Location::GPR(GPR::RBP));
    }

    fn emit_function_return_value(&mut self, ty: WpType, canonicalize: bool, loc: Location) {
        if canonicalize {
            self.canonicalize_nan(
                match ty {
                    WpType::F32 => Size::S32,
                    WpType::F64 => Size::S64,
                    _ => unreachable!(),
                },
                loc,
                Location::GPR(GPR::RAX),
            );
        } else {
            self.emit_relaxed_mov(Size::S64, loc, Location::GPR(GPR::RAX));
        }
    }

    fn emit_function_return_float(&mut self) {
        self.move_location(
            Size::S64,
            Location::GPR(GPR::RAX),
            Location::SIMD(XMM::XMM0),
        );
    }

    fn arch_supports_canonicalize_nan(&self) -> bool {
        self.assembler.arch_supports_canonicalize_nan()
    }
    fn canonicalize_nan(&mut self, sz: Size, input: Location, output: Location) {
        let tmp1 = self.acquire_temp_simd().unwrap();
        let tmp2 = self.acquire_temp_simd().unwrap();
        let tmp3 = self.acquire_temp_simd().unwrap();

        self.emit_relaxed_mov(sz, input, Location::SIMD(tmp1));
        let tmpg1 = self.acquire_temp_gpr().unwrap();

        match sz {
            Size::S32 => {
                self.assembler
                    .emit_vcmpunordss(tmp1, XMMOrMemory::XMM(tmp1), tmp2);
                self.move_location(
                    Size::S32,
                    Location::Imm32(0x7FC0_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                );
                self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp3));
                self.assembler
                    .emit_vblendvps(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1);
            }
            Size::S64 => {
                self.assembler
                    .emit_vcmpunordsd(tmp1, XMMOrMemory::XMM(tmp1), tmp2);
                self.move_location(
                    Size::S64,
                    Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                    Location::GPR(tmpg1),
                );
                self.move_location(Size::S64, Location::GPR(tmpg1), Location::SIMD(tmp3));
                self.assembler
                    .emit_vblendvpd(tmp2, XMMOrMemory::XMM(tmp3), tmp1, tmp1);
            }
            _ => unreachable!(),
        }

        self.emit_relaxed_mov(sz, Location::SIMD(tmp1), output);

        self.release_gpr(tmpg1);
        self.release_simd(tmp3);
        self.release_simd(tmp2);
        self.release_simd(tmp1);
    }

    fn emit_illegal_op(&mut self) {
        self.assembler.emit_ud2();
    }
    fn get_label(&mut self) -> Label {
        self.assembler.new_dynamic_label()
    }
    fn emit_label(&mut self, label: Label) {
        self.assembler.emit_label(label);
    }
    fn get_grp_for_call(&self) -> GPR {
        GPR::RAX
    }
    fn emit_call_register(&mut self, reg: GPR) {
        self.assembler.emit_call_register(reg);
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

    fn arch_emit_indirect_call_with_trampoline(&mut self, location: Location) {
        self.assembler
            .arch_emit_indirect_call_with_trampoline(location);
    }

    fn emit_call_location(&mut self, location: Location) {
        self.assembler.emit_call_location(location);
    }

    fn location_address(&mut self, size: Size, source: Location, dest: Location) {
        self.assembler.emit_lea(size, source, dest);
    }
    // logic
    fn location_and(&mut self, size: Size, source: Location, dest: Location, _flags: bool) {
        self.assembler.emit_and(size, source, dest);
    }
    fn location_xor(&mut self, size: Size, source: Location, dest: Location, _flags: bool) {
        self.assembler.emit_xor(size, source, dest);
    }
    fn location_or(&mut self, size: Size, source: Location, dest: Location, _flags: bool) {
        self.assembler.emit_or(size, source, dest);
    }
    fn location_test(&mut self, size: Size, source: Location, dest: Location) {
        self.assembler.emit_test(size, source, dest);
    }
    // math
    fn location_add(&mut self, size: Size, source: Location, dest: Location, _flags: bool) {
        self.assembler.emit_add(size, source, dest);
    }
    fn location_sub(&mut self, size: Size, source: Location, dest: Location, _flags: bool) {
        self.assembler.emit_sub(size, source, dest);
    }
    fn location_cmp(&mut self, size: Size, source: Location, dest: Location) {
        self.assembler.emit_cmp(size, source, dest);
    }
    // (un)conditionnal jmp
    // (un)conditionnal jmp
    fn jmp_unconditionnal(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::None, label);
    }
    fn jmp_on_equal(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Equal, label);
    }
    fn jmp_on_different(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::NotEqual, label);
    }
    fn jmp_on_above(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Above, label);
    }
    fn jmp_on_aboveequal(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::AboveEqual, label);
    }
    fn jmp_on_belowequal(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::BelowEqual, label);
    }
    fn jmp_on_overflow(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Carry, label);
    }

    // jmp table
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location) {
        let tmp1 = self.pick_temp_gpr().unwrap();
        self.reserve_gpr(tmp1);
        let tmp2 = self.pick_temp_gpr().unwrap();
        self.reserve_gpr(tmp2);

        self.assembler.emit_lea_label(label, Location::GPR(tmp1));
        self.move_location(Size::S32, cond, Location::GPR(tmp2));

        let instr_size = self.assembler.get_jmp_instr_size();
        self.assembler.emit_imul_imm32_gpr64(instr_size as _, tmp2);
        self.assembler
            .emit_add(Size::S64, Location::GPR(tmp1), Location::GPR(tmp2));
        self.assembler.emit_jmp_location(Location::GPR(tmp2));
        self.release_gpr(tmp2);
        self.release_gpr(tmp1);
    }

    fn align_for_loop(&mut self) {
        // Pad with NOPs to the next 16-byte boundary.
        // Here we don't use the dynasm `.align 16` attribute because it pads the alignment with single-byte nops
        // which may lead to efficiency problems.
        match self.assembler.get_offset().0 % 16 {
            0 => {}
            x => {
                self.assembler.emit_nop_n(16 - x);
            }
        }
        assert_eq!(self.assembler.get_offset().0 % 16, 0);
    }

    fn emit_ret(&mut self) {
        self.assembler.emit_ret();
    }

    fn emit_push(&mut self, size: Size, loc: Location) {
        self.assembler.emit_push(size, loc);
    }
    fn emit_pop(&mut self, size: Size, loc: Location) {
        self.assembler.emit_pop(size, loc);
    }

    fn memory_op_begin(
        &mut self,
        addr: Location,
        memarg: &MemoryImmediate,
        check_alignment: bool,
        value_size: usize,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        tmp_addr: GPR,
    ) -> usize {
        self.reserve_gpr(tmp_addr);

        // Reusing `tmp_addr` for temporary indirection here, since it's not used before the last reference to `{base,bound}_loc`.
        let (base_loc, bound_loc) = if imported_memories {
            // Imported memories require one level of indirection.
            self.move_location(
                Size::S64,
                Location::Memory(Machine::get_vmctx_reg(), offset),
                Location::GPR(tmp_addr),
            );
            (Location::Memory(tmp_addr, 0), Location::Memory(tmp_addr, 8))
        } else {
            (
                Location::Memory(Machine::get_vmctx_reg(), offset),
                Location::Memory(Machine::get_vmctx_reg(), offset + 8),
            )
        };

        let tmp_base = self.pick_temp_gpr().unwrap();
        self.reserve_gpr(tmp_base);
        let tmp_bound = self.pick_temp_gpr().unwrap();
        self.reserve_gpr(tmp_bound);

        // Load base into temporary register.
        self.move_location(Size::S64, base_loc, Location::GPR(tmp_base));

        // Load bound into temporary register, if needed.
        if need_check {
            self.move_location(Size::S64, bound_loc, Location::GPR(tmp_bound));

            // Wasm -> Effective.
            // Assuming we never underflow - should always be true on Linux/macOS and Windows >=8,
            // since the first page from 0x0 to 0x1000 is not accepted by mmap.

            // This `lea` calculates the upper bound allowed for the beginning of the word.
            // Since the upper bound of the memory is (exclusively) `tmp_bound + tmp_base`,
            // the maximum allowed beginning of word is (inclusively)
            // `tmp_bound + tmp_base - value_size`.
            self.location_address(
                Size::S64,
                Location::Memory2(tmp_bound, tmp_base, Multiplier::One, -(value_size as i32)),
                Location::GPR(tmp_bound),
            );
        }

        // Load effective address.
        // `base_loc` and `bound_loc` becomes INVALID after this line, because `tmp_addr`
        // might be reused.
        self.move_location(Size::S32, addr, Location::GPR(tmp_addr));

        // Add offset to memory address.
        if memarg.offset != 0 {
            self.location_add(
                Size::S32,
                Location::Imm32(memarg.offset),
                Location::GPR(tmp_addr),
                true,
            );

            // Trap if offset calculation overflowed.
            self.jmp_on_overflow(heap_access_oob);
        }

        // Wasm linear memory -> real memory
        self.location_add(
            Size::S64,
            Location::GPR(tmp_base),
            Location::GPR(tmp_addr),
            false,
        );

        if need_check {
            // Trap if the end address of the requested area is above that of the linear memory.
            self.location_cmp(Size::S64, Location::GPR(tmp_bound), Location::GPR(tmp_addr));

            // `tmp_bound` is inclusive. So trap only if `tmp_addr > tmp_bound`.
            self.jmp_on_above(heap_access_oob);
        }

        self.release_gpr(tmp_bound);
        self.release_gpr(tmp_base);

        let align = memarg.align;
        if check_alignment && align != 1 {
            self.location_test(
                Size::S32,
                Location::Imm32((align - 1).into()),
                Location::GPR(tmp_addr),
            );
            self.jmp_on_different(heap_access_oob);
        }

        self.get_offset().0
    }

    fn memory_op_end(&mut self, tmp_addr: GPR) -> usize {
        let end = self.get_offset().0;
        self.release_gpr(tmp_addr);

        end
    }

    fn emit_atomic_cmpxchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location,
        cmp: Location,
        addr: GPR,
        ret: Location,
    ) {
        let compare = GPR::RAX;
        // we have to take into account that there maybe no free tmp register
        let val = self.pick_temp_gpr();
        let value = match val {
            Some(value) => {
                self.release_gpr(value);
                value
            }
            _ => {
                if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                }
            }
        };
        if val.is_none() {
            self.assembler.emit_push(Size::S64, Location::GPR(value));
        }

        self.assembler
            .emit_mov(size_op, cmp, Location::GPR(compare));
        self.assembler.emit_mov(size_op, new, Location::GPR(value));
        self.assembler
            .emit_lock_cmpxchg(size_val, Location::GPR(value), Location::Memory(addr, 0));
        self.move_location_extend(size_val, signed, Location::GPR(compare), size_op, ret);
        if val.is_none() {
            self.assembler.emit_pop(Size::S64, Location::GPR(value));
        } else {
            self.used_gprs.remove(&value);
        }
    }
    fn emit_atomic_xchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location,
        addr: GPR,
        ret: Location,
    ) {
        // we have to take into account that there maybe no free tmp register
        let val = self.pick_temp_gpr();
        let value = match val {
            Some(value) => {
                self.release_gpr(value);
                value
            }
            _ => {
                if new == Location::GPR(GPR::R14) {
                    GPR::R13
                } else {
                    GPR::R14
                }
            }
        };
        if val.is_none() {
            self.assembler.emit_push(Size::S64, Location::GPR(value));
        }

        self.move_location_extend(size_val, signed, new, size_op, Location::GPR(value));
        self.assembler
            .emit_xchg(size_val, Location::GPR(value), Location::Memory(addr, 0));
        self.assembler.emit_mov(size_val, Location::GPR(value), ret);
        if val.is_none() {
            self.assembler.emit_pop(Size::S64, Location::GPR(value));
        } else {
            self.used_gprs.remove(&value);
        }
    }
    // atomic xadd if it exist
    fn has_atomic_xadd(&mut self) -> bool {
        true
    }
    fn emit_atomic_xadd(&mut self, size_op: Size, new: Location, ret: Location) {
        self.assembler.emit_lock_xadd(size_op, new, ret);
    }

    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) {
        self.move_location_extend(size_val, signed, source, size_op, dest);
        self.assembler.emit_neg(size_val, dest);
    }

    fn emit_imul_imm32(&mut self, size: Size, imm32: u32, gpr: GPR) {
        match size {
            Size::S64 => {
                self.assembler.emit_imul_imm32_gpr64(imm32, gpr);
            }
            _ => {
                unreachable!();
            }
        }
    }

    // relaxed binop based...
    fn emit_relaxed_mov(&mut self, sz: Size, src: Location, dst: Location) {
        self.emit_relaxed_binop(Assembler::emit_mov, sz, src, dst);
    }
    fn emit_relaxed_cmp(&mut self, sz: Size, src: Location, dst: Location) {
        self.emit_relaxed_binop(Assembler::emit_cmp, sz, src, dst);
    }
    fn emit_relaxed_atomic_xchg(&mut self, sz: Size, src: Location, dst: Location) {
        self.emit_relaxed_binop(Assembler::emit_xchg, sz, src, dst);
    }
    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) {
        if (sz_src == Size::S32 || sz_src == Size::S64) && sz_dst == Size::S64 {
            self.emit_relaxed_binop(Assembler::emit_mov, sz_src, src, dst);
        } else {
            self.emit_relaxed_zx_sx(Assembler::emit_movzx, sz_src, src, sz_dst, dst);
        }
    }
    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) {
        self.emit_relaxed_zx_sx(Assembler::emit_movsx, sz_src, src, sz_dst, dst);
    }
}

pub type Machine = AbstractMachine<GPR, XMM, MachineX86_64, X64Register>;

#[cfg(test)]
mod test {
    use super::*;
    use dynasmrt::x64::Assembler;

    #[test]
    fn test_release_locations_keep_state_nopanic() {
        let mut machine = Machine::new();
        let mut assembler = Assembler::new().unwrap();
        let locs = machine.acquire_locations(
            &mut assembler,
            &(0..10)
                .map(|_| (WpType::I32, MachineValue::Undefined))
                .collect::<Vec<_>>(),
            false,
        );

        machine.release_locations_keep_state(&mut assembler, &locs);
    }
}
