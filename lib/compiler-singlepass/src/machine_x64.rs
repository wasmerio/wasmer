use crate::common_decl::*;
use crate::emitter_x64::*;
use crate::machine::Machine as AbstractMachine;
use crate::machine::MachineSpecific;
use crate::x64_decl::new_machine_state;
use crate::x64_decl::{X64Register, GPR};
use std::collections::HashSet;
use wasmer_compiler::CallingConvention;

pub struct MachineX86_64 {}

impl MachineSpecific<GPR, XMM> for MachineX86_64 {
    fn new() -> Self {
        MachineX86_64 {}
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
    fn local_on_stack(&self, stack_offset: i32) -> Location {
        Location::Memory(GPR::RBP, -stack_offset)
    }

    // Adjust stack for locals
    fn adjust_stack<E: Emitter>(&self, assembler: &mut E, delta_stack_offset: u32) {
        assembler.emit_sub(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        );
    }
    // Pop stack of locals
    fn pop_stack_locals<E: Emitter>(&self, assembler: &mut E, delta_stack_offset: u32) {
        assembler.emit_add(
            Size::S64,
            Location::Imm32(delta_stack_offset),
            Location::GPR(GPR::RSP),
        );
    }

    // Zero a location that is 32bits
    fn zero_location<E: Emitter>(&self, assembler: &mut E, size: Size, location: Location) {
        assembler.emit_mov(size, Location::Imm32(0), location);
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
    fn move_local<E: Emitter>(&self, assembler: &mut E, stack_offset: i32, location: Location) {
        assembler.emit_mov(
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
    fn move_location<E: Emitter>(&self, assembler: &mut E, source: Location, dest: Location) {
        match source {
            Location::GPR(_) => {
                assembler.emit_mov(Size::S64, source, dest);
            }
            Location::Memory(_, _) => match dest {
                Location::GPR(_) => {
                    assembler.emit_mov(Size::S64, source, dest);
                }
                Location::Memory(_, _) => {
                    assembler.emit_mov(Size::S64, source, Location::GPR(GPR::RAX));
                    assembler.emit_mov(Size::S64, Location::GPR(GPR::RAX), dest);
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }
    // Init the stack loc counter
    fn init_stack_loc<E: Emitter>(
        &self,
        assembler: &mut E,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location,
    ) {
        // Since these assemblies take up to 24 bytes, if more than 2 slots are initialized, then they are smaller.
        assembler.emit_mov(
            Size::S64,
            Location::Imm64(init_stack_loc_cnt),
            Location::GPR(GPR::RCX),
        );
        assembler.emit_xor(Size::S64, Location::GPR(GPR::RAX), Location::GPR(GPR::RAX));
        assembler.emit_lea(Size::S64, last_stack_loc, Location::GPR(GPR::RDI));
        assembler.emit_rep_stosq();
    }
    // Restore save_area
    fn restore_saved_area<E: Emitter>(&self, assembler: &mut E, saved_area_offset: i32) {
        assembler.emit_lea(
            Size::S64,
            Location::Memory(GPR::RBP, -saved_area_offset),
            Location::GPR(GPR::RSP),
        );
    }
    // Pop a location
    fn pop_location<E: Emitter>(&self, assembler: &mut E, location: Location) {
        assembler.emit_pop(Size::S64, location);
    }
    // Create a new `MachineState` with default values.
    fn new_machine_state() -> MachineState {
        new_machine_state()
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
