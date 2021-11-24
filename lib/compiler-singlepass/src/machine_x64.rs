use crate::common_decl::*;
use crate::emitter_x64::*;
use crate::machine::Machine as AbstractMachine;
use crate::machine::{MachineSpecific, MemoryImmediate};
use crate::x64_decl::new_machine_state;
use crate::x64_decl::{X64Register, GPR};
use dynasmrt::x64::Assembler;
use std::collections::HashSet;
use wasmer_compiler::CallingConvention;

pub struct MachineX86_64 {
    pub assembler: Assembler, //temporary public
    used_gprs: HashSet<GPR>,
    used_simd: HashSet<XMM>,
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
    fn location_cmp(&mut self, size: Size, source: Location, dest: Location) {
        self.assembler.emit_cmp(size, source, dest);
    }
    // (un)conditionnal jmp
    fn jmp_on_equal(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Equal, label);
    }
    fn jmp_on_different(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::NotEqual, label);
    }
    fn jmp_on_above(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Above, label);
    }
    fn jmp_on_overflow(&mut self, label: Label) {
        self.assembler.emit_jmp(Condition::Carry, label);
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
        match size_val {
            Size::S64 => self
                .assembler
                .emit_mov(size_val, Location::GPR(compare), ret),
            Size::S32 => {
                if signed && size_op == Size::S64 {
                    self.assembler
                        .emit_movsx(size_val, Location::GPR(compare), size_op, ret);
                } else {
                    self.assembler
                        .emit_mov(size_val, Location::GPR(compare), ret);
                }
            }
            Size::S16 | Size::S8 => {
                if signed {
                    self.assembler
                        .emit_movsx(size_val, Location::GPR(compare), size_op, ret);
                } else {
                    self.assembler
                        .emit_movzx(size_val, Location::GPR(compare), size_op, ret);
                }
            }
        }
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

        match size_val {
            Size::S64 => self.assembler.emit_mov(size_val, new, Location::GPR(value)),
            Size::S32 => {
                if signed && size_op == Size::S64 {
                    self.assembler
                        .emit_movsx(size_val, new, size_op, Location::GPR(value));
                } else {
                    self.assembler.emit_mov(size_val, new, Location::GPR(value));
                }
            }
            Size::S16 | Size::S8 => {
                if signed {
                    self.assembler
                        .emit_movsx(size_val, new, size_op, Location::GPR(value));
                } else {
                    self.assembler
                        .emit_movzx(size_val, new, size_op, Location::GPR(value));
                }
            }
        }
        self.assembler
            .emit_xchg(size_val, Location::GPR(value), Location::Memory(addr, 0));
        self.assembler.emit_mov(size_val, Location::GPR(value), ret);
        if val.is_none() {
            self.assembler.emit_pop(Size::S64, Location::GPR(value));
        } else {
            self.used_gprs.remove(&value);
        }
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
