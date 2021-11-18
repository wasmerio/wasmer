use crate::common_decl::*;
use crate::emitter_x64::*;
use crate::machine::Machine as AbstractMachine;
use crate::machine::MachineSpecific;
use crate::x64_decl::new_machine_state;
use crate::x64_decl::{X64Register, GPR};
use dynasmrt::x64::Assembler;
use std::collections::HashSet;
use wasmer_compiler::CallingConvention;

pub struct MachineX86_64 {
    pub assembler: Assembler, //temporary public
}

impl MachineSpecific<GPR, XMM> for MachineX86_64 {
    fn new() -> Self {
        MachineX86_64 {
            assembler: Assembler::new().unwrap(),
        }
    }
    fn get_vmctx_reg() -> GPR {
        GPR::R15
    }

    fn pick_gpr(&self, used_gprs: &HashSet<GPR>) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[RSI, RDI, R8, R9, R10, R11];
        for r in REGS {
            if !used_gprs.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused general purpose register for internal temporary use.
    fn pick_temp_gpr(&self, used_gprs: &HashSet<GPR>) -> Option<GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[RAX, RCX, RDX];
        for r in REGS {
            if !used_gprs.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    fn get_cmpxchg_temp_gprs(&self) -> Vec<GPR> {
        vec![GPR::RAX]
    }

    // Picks an unused XMM register.
    fn pick_simd(&self, used_simd: &HashSet<XMM>) -> Option<XMM> {
        use XMM::*;
        static REGS: &[XMM] = &[XMM3, XMM4, XMM5, XMM6, XMM7];
        for r in REGS {
            if !used_simd.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    // Picks an unused XMM register for internal temporary use.
    fn pick_temp_simd(&self, used_simd: &HashSet<XMM>) -> Option<XMM> {
        use XMM::*;
        static REGS: &[XMM] = &[XMM0, XMM1, XMM2];
        for r in REGS {
            if !used_simd.contains(r) {
                return Some(*r);
            }
        }
        None
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

    fn emit_atomic_cmpxchg(
        &mut self,
        size_op: Size,
        size_val: Size,
        signed: bool,
        new: Location,
        cmp: Location,
        addr: GPR,
        ret: Location,
        used_gprs: &mut HashSet<GPR>,
    ) {
        let compare = GPR::RAX;
        // we have to take into account that there maybe no free tmp register
        let val = self.pick_temp_gpr(used_gprs);
        let value = match val {
            Some(value) => {
                used_gprs.insert(value);
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
            used_gprs.remove(&value);
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
