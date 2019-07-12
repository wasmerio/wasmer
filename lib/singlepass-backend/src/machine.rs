use crate::emitter_x64::*;
use smallvec::SmallVec;
use std::collections::HashSet;
use wasmer_runtime_core::state::x64::X64Register;
use wasmer_runtime_core::state::*;
use wasmparser::Type as WpType;

struct MachineStackOffset(usize);

pub struct Machine {
    used_gprs: HashSet<GPR>,
    used_xmms: HashSet<XMM>,
    stack_offset: MachineStackOffset,
    save_area_offset: Option<MachineStackOffset>,
    pub state: MachineState,
    pub(crate) track_state: bool,
}

impl Machine {
    pub fn new() -> Self {
        Machine {
            used_gprs: HashSet::new(),
            used_xmms: HashSet::new(),
            stack_offset: MachineStackOffset(0),
            save_area_offset: None,
            state: x64::new_machine_state(),
            track_state: true,
        }
    }

    pub fn get_stack_offset(&self) -> usize {
        self.stack_offset.0
    }

    pub fn get_used_gprs(&self) -> Vec<GPR> {
        self.used_gprs.iter().cloned().collect()
    }

    pub fn get_used_xmms(&self) -> Vec<XMM> {
        self.used_xmms.iter().cloned().collect()
    }

    pub fn get_vmctx_reg() -> GPR {
        GPR::R15
    }

    /// Picks an unused general purpose register for local/stack/argument use.
    ///
    /// This method does not mark the register as used.
    pub fn pick_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &'static [GPR] = &[RSI, RDI, R8, R9, R10, R11];
        for r in REGS {
            if !self.used_gprs.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    /// Picks an unused general purpose register for internal temporary use.
    ///
    /// This method does not mark the register as used.
    pub fn pick_temp_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &'static [GPR] = &[RAX, RCX, RDX];
        for r in REGS {
            if !self.used_gprs.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    /// Acquires a temporary GPR.
    pub fn acquire_temp_gpr(&mut self) -> Option<GPR> {
        let gpr = self.pick_temp_gpr();
        if let Some(x) = gpr {
            self.used_gprs.insert(x);
        }
        gpr
    }

    /// Releases a temporary GPR.
    pub fn release_temp_gpr(&mut self, gpr: GPR) {
        assert_eq!(self.used_gprs.remove(&gpr), true);
    }

    /// Picks an unused XMM register.
    ///
    /// This method does not mark the register as used.
    pub fn pick_xmm(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &'static [XMM] = &[XMM3, XMM4, XMM5, XMM6, XMM7];
        for r in REGS {
            if !self.used_xmms.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    /// Picks an unused XMM register for internal temporary use.
    ///
    /// This method does not mark the register as used.
    pub fn pick_temp_xmm(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &'static [XMM] = &[XMM0, XMM1, XMM2];
        for r in REGS {
            if !self.used_xmms.contains(r) {
                return Some(*r);
            }
        }
        None
    }

    /// Acquires a temporary XMM register.
    pub fn acquire_temp_xmm(&mut self) -> Option<XMM> {
        let xmm = self.pick_temp_xmm();
        if let Some(x) = xmm {
            self.used_xmms.insert(x);
        }
        xmm
    }

    /// Releases a temporary XMM register.
    pub fn release_temp_xmm(&mut self, xmm: XMM) {
        assert_eq!(self.used_xmms.remove(&xmm), true);
    }

    /// Acquires locations from the machine state.
    ///
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    pub fn acquire_locations<E: Emitter>(
        &mut self,
        assembler: &mut E,
        tys: &[(WpType, MachineValue)],
        zeroed: bool,
    ) -> SmallVec<[Location; 1]> {
        let mut ret = smallvec![];
        let mut delta_stack_offset: usize = 0;

        for (ty, mv) in tys {
            let loc = match *ty {
                WpType::F32 | WpType::F64 => self.pick_xmm().map(Location::XMM),
                WpType::I32 | WpType::I64 => self.pick_gpr().map(Location::GPR),
                _ => unreachable!(),
            };

            let loc = if let Some(x) = loc {
                x
            } else {
                self.stack_offset.0 += 8;
                delta_stack_offset += 8;
                Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32))
            };
            if let Location::GPR(x) = loc {
                self.used_gprs.insert(x);
                self.state.register_values[X64Register::GPR(x).to_index().0] = *mv;
            } else if let Location::XMM(x) = loc {
                self.used_xmms.insert(x);
                self.state.register_values[X64Register::XMM(x).to_index().0] = *mv;
            } else {
                self.state.stack_values.push(*mv);
            }
            self.state.wasm_stack.push(WasmAbstractValue::Runtime);
            ret.push(loc);
        }

        if delta_stack_offset != 0 {
            assembler.emit_sub(
                Size::S64,
                Location::Imm32(delta_stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
        }
        if zeroed {
            for i in 0..tys.len() {
                assembler.emit_mov(Size::S64, Location::Imm32(0), ret[i]);
            }
        }
        ret
    }

    /// Releases locations used for stack value.
    pub fn release_locations<E: Emitter>(&mut self, assembler: &mut E, locs: &[Location]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    assert_eq!(self.used_gprs.remove(x), true);
                    self.state.register_values[X64Register::GPR(*x).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::XMM(ref x) => {
                    assert_eq!(self.used_xmms.remove(x), true);
                    self.state.register_values[X64Register::XMM(*x).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::Memory(GPR::RBP, x) => {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != self.stack_offset.0 {
                        unreachable!();
                    }
                    self.stack_offset.0 -= 8;
                    delta_stack_offset += 8;
                    self.state.stack_values.pop().unwrap();
                }
                _ => {}
            }
            self.state.wasm_stack.pop().unwrap();
        }

        if delta_stack_offset != 0 {
            assembler.emit_add(
                Size::S64,
                Location::Imm32(delta_stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
        }
    }

    pub fn release_locations_only_regs(&mut self, locs: &[Location]) {
        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    assert_eq!(self.used_gprs.remove(x), true);
                    self.state.register_values[X64Register::GPR(*x).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::XMM(ref x) => {
                    assert_eq!(self.used_xmms.remove(x), true);
                    self.state.register_values[X64Register::XMM(*x).to_index().0] =
                        MachineValue::Undefined;
                }
                _ => {}
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }
    }

    pub fn release_locations_only_stack<E: Emitter>(
        &mut self,
        assembler: &mut E,
        locs: &[Location],
    ) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::Memory(GPR::RBP, x) => {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != self.stack_offset.0 {
                        unreachable!();
                    }
                    self.stack_offset.0 -= 8;
                    delta_stack_offset += 8;
                    self.state.stack_values.pop().unwrap();
                }
                _ => {}
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }

        if delta_stack_offset != 0 {
            assembler.emit_add(
                Size::S64,
                Location::Imm32(delta_stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
        }
    }

    pub fn release_locations_only_osr_state(&mut self, n: usize) {
        for _ in 0..n {
            self.state.wasm_stack.pop().unwrap();
        }
    }

    pub fn release_locations_keep_state<E: Emitter>(&self, assembler: &mut E, locs: &[Location]) {
        let mut delta_stack_offset: usize = 0;
        let mut stack_offset = self.stack_offset.0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::Memory(GPR::RBP, x) => {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != stack_offset {
                        unreachable!();
                    }
                    stack_offset -= 8;
                    delta_stack_offset += 8;
                }
                _ => {}
            }
        }

        if delta_stack_offset != 0 {
            assembler.emit_add(
                Size::S64,
                Location::Imm32(delta_stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
        }
    }

    pub fn init_locals<E: Emitter>(
        &mut self,
        a: &mut E,
        n: usize,
        n_params: usize,
    ) -> Vec<Location> {
        // Use callee-saved registers for locals.
        fn get_local_location(idx: usize) -> Location {
            match idx {
                0 => Location::GPR(GPR::R12),
                1 => Location::GPR(GPR::R13),
                2 => Location::GPR(GPR::R14),
                3 => Location::GPR(GPR::RBX),
                _ => Location::Memory(GPR::RBP, -(((idx - 3) * 8) as i32)),
            }
        }

        let mut locations: Vec<Location> = vec![];
        let mut allocated: usize = 0;

        // Determine locations for parameters.
        for i in 0..n_params {
            let loc = Self::get_param_location(i + 1);
            locations.push(match loc {
                Location::GPR(_) => {
                    let old_idx = allocated;
                    allocated += 1;
                    get_local_location(old_idx)
                }
                Location::Memory(_, _) => {
                    let old_idx = allocated;
                    allocated += 1;
                    get_local_location(old_idx)
                }
                _ => unreachable!(),
            });
        }

        // Determine locations for normal locals.
        for _ in n_params..n {
            locations.push(get_local_location(allocated));
            allocated += 1;
        }

        for (i, loc) in locations.iter().enumerate() {
            match *loc {
                Location::GPR(x) => {
                    self.state.register_values[X64Register::GPR(x).to_index().0] =
                        MachineValue::WasmLocal(i);
                }
                Location::Memory(_, _) => {
                    self.state.stack_values.push(MachineValue::WasmLocal(i));
                }
                _ => unreachable!(),
            }
        }

        // How many machine stack slots did all the locals use?
        let num_mem_slots = locations
            .iter()
            .filter(|&&loc| match loc {
                Location::Memory(_, _) => true,
                _ => false,
            })
            .count();

        // Move RSP down to reserve space for machine stack slots.
        if num_mem_slots > 0 {
            a.emit_sub(
                Size::S64,
                Location::Imm32((num_mem_slots * 8) as u32),
                Location::GPR(GPR::RSP),
            );
            self.stack_offset.0 += num_mem_slots * 8;
        }

        // Save callee-saved registers.
        for loc in locations.iter() {
            if let Location::GPR(x) = *loc {
                a.emit_push(Size::S64, *loc);
                self.stack_offset.0 += 8;
                self.state.stack_values.push(MachineValue::PreserveRegister(
                    X64Register::GPR(x).to_index(),
                ));
            }
        }

        // Save R15 for vmctx use.
        a.emit_push(Size::S64, Location::GPR(GPR::R15));
        self.stack_offset.0 += 8;
        self.state.stack_values.push(MachineValue::PreserveRegister(
            X64Register::GPR(GPR::R15).to_index(),
        ));

        // Save the offset of static area.
        self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // Load in-register parameters into the allocated locations.
        for i in 0..n_params {
            let loc = Self::get_param_location(i + 1);
            match loc {
                Location::GPR(_) => {
                    a.emit_mov(Size::S64, loc, locations[i]);
                }
                Location::Memory(_, _) => match locations[i] {
                    Location::GPR(_) => {
                        a.emit_mov(Size::S64, loc, locations[i]);
                    }
                    Location::Memory(_, _) => {
                        a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                        a.emit_mov(Size::S64, Location::GPR(GPR::RAX), locations[i]);
                    }
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }
        }

        // Load vmctx.
        a.emit_mov(
            Size::S64,
            Self::get_param_location(0),
            Location::GPR(GPR::R15),
        );

        // Initialize all normal locals to zero.
        for i in n_params..n {
            a.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        }

        locations
    }

    pub fn finalize_locals<E: Emitter>(&mut self, a: &mut E, locations: &[Location]) {
        // Unwind stack to the "save area".
        a.emit_lea(
            Size::S64,
            Location::Memory(
                GPR::RBP,
                -(self.save_area_offset.as_ref().unwrap().0 as i32),
            ),
            Location::GPR(GPR::RSP),
        );

        // Restore R15 used by vmctx.
        a.emit_pop(Size::S64, Location::GPR(GPR::R15));

        // Restore callee-saved registers.
        for loc in locations.iter().rev() {
            if let Location::GPR(_) = *loc {
                a.emit_pop(Size::S64, *loc);
            }
        }
    }

    pub fn get_param_location(idx: usize) -> Location {
        match idx {
            0 => Location::GPR(GPR::RDI),
            1 => Location::GPR(GPR::RSI),
            2 => Location::GPR(GPR::RDX),
            3 => Location::GPR(GPR::RCX),
            4 => Location::GPR(GPR::R8),
            5 => Location::GPR(GPR::R9),
            _ => Location::Memory(GPR::RBP, (16 + (idx - 6) * 8) as i32),
        }
    }
}

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
            &[(WpType::I32, MachineValue::Undefined); 10],
            false,
        );

        machine.release_locations_keep_state(&mut assembler, &locs);
    }

}
