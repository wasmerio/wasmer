use crate::common_decl::*;
use crate::emitter::*;
// use crate::x64_decl::{new_machine_state, X64Register};
// use smallvec::smallvec;
use smallvec::SmallVec;
// use std::cmp;
// use std::collections::HashSet;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer::Value;

// const NATIVE_PAGE_SIZE: usize = 4096;

// struct MachineStackOffset(usize);

use std::{collections::BTreeMap};
use std::fmt::Debug;

// pub struct Machine {
//     used_gprs: HashSet<GPR>,
//     used_xmms: HashSet<XMM>,
//     stack_offset: MachineStackOffset,
//     save_area_offset: Option<MachineStackOffset>,
//     pub state: MachineState,
//     pub(crate) track_state: bool,
// }

// pub trait Loc {
//     fn immediate32(n: u32) -> Self;
// }

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
}

pub trait Machine {
    type Location: MaybeImmediate + Copy + Debug;
    type Emitter: Emitter<Location = Self::Location>;

    fn new_state() -> MachineState;

    fn get_state(&mut self) -> &mut MachineState;

    // fn do_return(&mut self, a: &mut Self::Emitter, loc: Option<Self::Location>);

    fn imm32(&mut self, a: &mut Self::Emitter, n: u32) -> Local<Self::Location>;

    // fn acquire_locations(
    //     &mut self,
    //     assembler: &mut Self::Emitter,
    //     tys: &[(WpType, MachineValue)],
    //     zeroed: bool,
    // ) -> SmallVec<[Self::Location; 1]>;

    // fn release_locations(&mut self, assembler: &mut Self::Emitter, locs: &[Self::Location]);

    fn release_location(&mut self, loc: Local<Self::Location>);

    // fn get_stack_offset(&self) -> usize {
    //     self.stack_offset.0
    // }

    // fn get_used_gprs(&self) -> Vec<GPR> {
    //     self.used_gprs.iter().cloned().collect()
    // }

    // fn get_used_xmms(&self) -> Vec<XMM> {
    //     self.used_xmms.iter().cloned().collect()
    // }

    // fn get_vmctx_reg() -> GPR {
    //     GPR::R15
    // }

    // /// Picks an unused general purpose register for internal temporary use.
    // ///
    // /// This method does not mark the register as used.
    // pub fn pick_temp_gpr(&self) -> Option<GPR> {
    //     use GPR::*;
    //     static REGS: &[GPR] = &[RAX, RCX, RDX];
    //     for r in REGS {
    //         if !self.used_gprs.contains(r) {
    //             return Some(*r);
    //         }
    //     }
    //     None
    // }

    // /// Acquires a temporary GPR.
    // pub fn acquire_temp_gpr(&mut self) -> Option<GPR> {
    //     let gpr = self.pick_temp_gpr();
    //     if let Some(x) = gpr {
    //         self.used_gprs.insert(x);
    //     }
    //     gpr
    // }

    // /// Releases a temporary GPR.
    // pub fn release_temp_gpr(&mut self, gpr: GPR) {
    //     assert!(self.used_gprs.remove(&gpr));
    // }

    // /// Specify that a given register is in use.
    // pub fn reserve_unused_temp_gpr(&mut self, gpr: GPR) -> GPR {
    //     assert!(!self.used_gprs.contains(&gpr));
    //     self.used_gprs.insert(gpr);
    //     gpr
    // }

    // /// Picks an unused XMM register.
    // ///
    // /// This method does not mark the register as used.
    // pub fn pick_xmm(&self) -> Option<XMM> {
    //     use XMM::*;
    //     static REGS: &[XMM] = &[XMM3, XMM4, XMM5, XMM6, XMM7];
    //     for r in REGS {
    //         if !self.used_xmms.contains(r) {
    //             return Some(*r);
    //         }
    //     }
    //     None
    // }

    // /// Picks an unused XMM register for internal temporary use.
    // ///
    // /// This method does not mark the register as used.
    // pub fn pick_temp_xmm(&self) -> Option<XMM> {
    //     use XMM::*;
    //     static REGS: &[XMM] = &[XMM0, XMM1, XMM2];
    //     for r in REGS {
    //         if !self.used_xmms.contains(r) {
    //             return Some(*r);
    //         }
    //     }
    //     None
    // }

    // /// Acquires a temporary XMM register.
    // pub fn acquire_temp_xmm(&mut self) -> Option<XMM> {
    //     let xmm = self.pick_temp_xmm();
    //     if let Some(x) = xmm {
    //         self.used_xmms.insert(x);
    //     }
    //     xmm
    // }

    // /// Releases a temporary XMM register.
    // pub fn release_temp_xmm(&mut self, xmm: XMM) {
    //     assert_eq!(self.used_xmms.remove(&xmm), true);
    // }

    // pub fn release_locations_only_regs(&mut self, locs: &[Location]) {
    //     for loc in locs.iter().rev() {
    //         match *loc {
    //             Location::GPR(ref x) => {
    //                 assert_eq!(self.used_gprs.remove(x), true);
    //                 self.state.register_values[X64Register::GPR(*x).to_index().0] =
    //                     MachineValue::Undefined;
    //             }
    //             Location::XMM(ref x) => {
    //                 assert_eq!(self.used_xmms.remove(x), true);
    //                 self.state.register_values[X64Register::XMM(*x).to_index().0] =
    //                     MachineValue::Undefined;
    //             }
    //             _ => {}
    //         }
    //         // Wasm state popping is deferred to `release_locations_only_osr_state`.
    //     }
    // }

    // pub fn release_locations_only_stack<E: Emitter>(
    //     &mut self,
    //     assembler: &mut E,
    //     locs: &[Location],
    // ) {
    //     let mut delta_stack_offset: usize = 0;

    //     for loc in locs.iter().rev() {
    //         if let Location::Memory(GPR::RBP, x) = *loc {
    //             if x >= 0 {
    //                 unreachable!();
    //             }
    //             let offset = (-x) as usize;
    //             if offset != self.stack_offset.0 {
    //                 unreachable!();
    //             }
    //             self.stack_offset.0 -= 8;
    //             delta_stack_offset += 8;
    //             self.state.stack_values.pop().unwrap();
    //         }
    //         // Wasm state popping is deferred to `release_locations_only_osr_state`.
    //     }

    //     if delta_stack_offset != 0 {
    //         assembler.emit_add(
    //             Size::S64,
    //             Location::Imm32(delta_stack_offset as u32),
    //             Location::GPR(GPR::RSP),
    //         );
    //     }
    // }

    // pub fn release_locations_only_osr_state(&mut self, n: usize) {
    //     let new_length = self
    //         .state
    //         .wasm_stack
    //         .len()
    //         .checked_sub(n)
    //         .expect("release_locations_only_osr_state: length underflow");
    //     self.state.wasm_stack.truncate(new_length);
    // }

    // pub fn release_locations_keep_state<E: Emitter>(&self, assembler: &mut E, locs: &[Location]) {
    //     let mut delta_stack_offset: usize = 0;
    //     let mut stack_offset = self.stack_offset.0;

    //     for loc in locs.iter().rev() {
    //         if let Location::Memory(GPR::RBP, x) = *loc {
    //             if x >= 0 {
    //                 unreachable!();
    //             }
    //             let offset = (-x) as usize;
    //             if offset != stack_offset {
    //                 unreachable!();
    //             }
    //             stack_offset -= 8;
    //             delta_stack_offset += 8;
    //         }
    //     }

    //     if delta_stack_offset != 0 {
    //         assembler.emit_add(
    //             Size::S64,
    //             Location::Imm32(delta_stack_offset as u32),
    //             Location::GPR(GPR::RSP),
    //         );
    //     }
    // }

    fn init_locals(&mut self, a: &mut Self::Emitter, n_locals: usize, n_params: usize) -> Vec<Local<Self::Location>>;

    fn finalize_stack(&mut self, a: &mut Self::Emitter, locations: &[Local<Self::Location>]);




    fn emit_add_i32(&mut self, a: &mut Self::Emitter, sz: Size,
        src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn emit_sub_i32(&mut self, a: &mut Self::Emitter, sz: Size,
        src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
}

// #[cfg(test)]
// mod test {
//     use super::*;
//     use dynasmrt::x64::Assembler;

//     #[test]
//     fn test_release_locations_keep_state_nopanic() {
//         let mut machine = Machine::new();
//         let mut assembler = Assembler::new().unwrap();
//         let locs = machine.acquire_locations(
//             &mut assembler,
//             &(0..10)
//                 .map(|_| (WpType::I32, MachineValue::Undefined))
//                 .collect::<Vec<_>>(),
//             false,
//         );

//         machine.release_locations_keep_state(&mut assembler, &locs);
//     }
// }
