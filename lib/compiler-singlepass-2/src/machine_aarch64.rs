use crate::common_decl::*;
use crate::emitter::*;
use crate::machine::*;

use dynasmrt::aarch64::Assembler;

// use crate::x64_decl::{new_machine_state, X64Register};
use smallvec::smallvec;
use smallvec::SmallVec;
// use std::cmp;
use std::collections::HashSet;
use wasmer_compiler::wasmparser::Type as WpType;

// const NATIVE_PAGE_SIZE: usize = 4096;

use std::{collections::BTreeMap};


/// General-purpose registers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Reg {
    X(u32),
    SP,
    XZR,
}

impl Reg {
    pub const REG_COUNT: usize = 32;

    pub fn to_index(self) -> Option<usize> {
        match self {
            Reg::X(n) => {
                if n < 31 {
                    Some(n as usize)
                } else {
                    None
                }
            },
            XZR => Some(31),
            SP => Some(32),
        }
    }

    pub fn from_index(n: usize) -> Option<Reg> {
        match n {
            0..=30 => Some(Reg::X(n as u32)),
            31 => Some(XZR),
            32 => Some(SP),
            _ => None
        }
    }
}

pub const X0:  Reg = Reg::X(0);
pub const X1:  Reg = Reg::X(1);
pub const X2:  Reg = Reg::X(2);
pub const X3:  Reg = Reg::X(3);
pub const X4:  Reg = Reg::X(4);
pub const X5:  Reg = Reg::X(5);
pub const X6:  Reg = Reg::X(6);
pub const X7:  Reg = Reg::X(7);
pub const X8:  Reg = Reg::X(8);
pub const X9:  Reg = Reg::X(9);
pub const X10: Reg = Reg::X(10);
pub const X11: Reg = Reg::X(11);
pub const X12: Reg = Reg::X(12);
pub const X13: Reg = Reg::X(13);
pub const X14: Reg = Reg::X(14);
pub const X15: Reg = Reg::X(15);
pub const X16: Reg = Reg::X(16);
pub const X17: Reg = Reg::X(17);
pub const X18: Reg = Reg::X(18);
pub const X19: Reg = Reg::X(19);
pub const X20: Reg = Reg::X(20);
pub const X21: Reg = Reg::X(21);
pub const X22: Reg = Reg::X(22);
pub const X23: Reg = Reg::X(23);
pub const X24: Reg = Reg::X(24);
pub const X25: Reg = Reg::X(25);
pub const X26: Reg = Reg::X(26);
pub const X27: Reg = Reg::X(27);
pub const X28: Reg = Reg::X(28);
pub const X29: Reg = Reg::X(29);
pub const X30: Reg = Reg::X(30);
pub const XZR: Reg = Reg::XZR;
pub const SP:  Reg = Reg::SP;
pub const FP:  Reg = X29;

struct MachineStackOffset(usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Location {
    Imm32(u32),
    Reg(Reg),
    Memory(Reg, i32),
}

// pub struct Machine {
//     used_xmms: HashSet<XMM>,
//     save_area_offset: Option<MachineStackOffset>,
//     pub(crate) track_state: bool,
// }

pub struct Aarch64Machine {
    used_regs: HashSet<Reg>,
    stack_offset: MachineStackOffset,
    state: MachineState,
}

impl Aarch64Machine {
    pub fn new() -> Self {
        Self {
            state: Self::new_state(),
            used_regs: HashSet::new(),
            stack_offset: MachineStackOffset(0),
        }
    }

    /// Picks an unused general purpose register for local/stack/argument use.
    ///
    /// This method does not mark the register as used.
    fn pick_reg(&self, ty: WpType) -> Option<Reg> {
        static REGS: &[Reg] = &[X0, X1, X2, X3, X4, X5, X6, X7];
        for r in REGS {
            if !self.used_regs.contains(r) {
                return Some(*r);
            }
        }
        None
    }
}

impl Aarch64Machine {
    pub fn get_param_location(i: usize) -> Location {
        match i {
            0..=7 => Location::Reg(Reg::X(i as u32)),
            _ => Location::Memory(FP, (16 + (i - 8) * 8) as i32),
        }
    }
}

impl Machine for Aarch64Machine {
    type Location = Location;
    type Emitter = Assembler;

    fn new_state() -> MachineState {
        MachineState {
            stack_values: vec![],
            register_values: vec![MachineValue::Undefined; 31],
            prev_frame: BTreeMap::new(),
            wasm_stack: vec![],
            wasm_inst_offset: std::usize::MAX,
        }
    }

    fn get_state(&mut self) -> &mut MachineState {
        &mut self.state
    }

    fn imm32(&mut self, a: &mut Self::Emitter, n: u32) -> Location {
        Location::Imm32(n)
    }

    fn do_return(&mut self, a: &mut Self::Emitter, loc: Option<Location>) {
        if let Some(loc) = loc {
            match loc {
                Location::Reg(X0) => {}
                _ => {
                    a.emit_move(Size::S64, loc, Location::Reg(X0));
                }
            }
        }

        a.emit_return();
    }

    // N.B. `n_locals` includes `n_params`; `n_locals` will always be >= `n_params`
    fn init_locals(&mut self, a: &mut Self::Emitter, n_locals: usize, n_params: usize,) -> Vec<Location> {
        // // Determine whether a local should be allocated on the stack.
        // fn is_local_on_stack(idx: usize) -> bool {
        //     idx > 10
        // }

        // Determine a local's location.
        fn get_local_location(idx: usize, callee_saved_regs_size: usize) -> Location {
            // Use callee-saved registers for the first locals.
            match idx {
                0..=9 => Location::Reg(Reg::X(idx as u32 + 19)),
                _ => unimplemented!(),//Location::Memory(FP, idx as i32 - 9),//Memory(GPR::RBP, -(((idx - 3) * 8 + callee_saved_regs_size) as i32)),
            }
        }

        // // How many machine stack slots will all the locals use?
        // let num_mem_slots = (0..n_locals).filter(|&x| is_local_on_stack(x)).count();

        // // Total size (in bytes) of the pre-allocated "static area" for this function's
        // // locals and callee-saved registers.
        // let mut static_area_size: usize = 0;

        // // Callee-saved registers used for locals.
        // // Keep this consistent with the "Save callee-saved registers" code below.
        // for i in 0..n_locals {
        //     // If a local is not stored on stack, then it is allocated to a callee-saved register.
        //     if !is_local_on_stack(i) {
        //         static_area_size += 8;
        //     }
        // }

        // // Callee-saved R15 for vmctx.
        // static_area_size += 8;

        // Total size of callee saved registers.
        let callee_saved_regs_size = 0;//static_area_size;

        // Now we can determine concrete locations for locals.
        let locations: Vec<Location> = (0..n_locals)
            .map(|i| get_local_location(i, callee_saved_regs_size))
            .collect();

        // // Add size of locals on stack.
        // static_area_size += num_mem_slots * 8;

        // // Allocate save area, without actually writing to it.
        // a.emit_sub(
        //     Size::S64,
        //     Location::Imm32(static_area_size as _),
        //     Location::GPR(GPR::RSP),
        // );

        // // Save callee-saved registers.
        // for loc in locations.iter() {
        //     if let Location::GPR(x) = *loc {
        //         self.stack_offset.0 += 8;
        //         a.emit_mov(
        //             Size::S64,
        //             *loc,
        //             Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)),
        //         );
        //         self.state.stack_values.push(MachineValue::PreserveRegister(
        //             X64Register::GPR(x).to_index(),
        //         ));
        //     }
        // }

        // // Save R15 for vmctx use.
        // self.stack_offset.0 += 8;
        // a.emit_mov(
        //     Size::S64,
        //     Location::GPR(GPR::R15),
        //     Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)),
        // );
        // self.state.stack_values.push(MachineValue::PreserveRegister(
        //     X64Register::GPR(GPR::R15).to_index(),
        // ));

        // // Save the offset of register save area.
        // self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // // Save location information for locals.
        // for (i, loc) in locations.iter().enumerate() {
        //     match *loc {
        //         Location::GPR(x) => {
        //             self.state.register_values[X64Register::GPR(x).to_index().0] =
        //                 MachineValue::WasmLocal(i);
        //         }
        //         Location::Memory(_, _) => {
        //             self.state.stack_values.push(MachineValue::WasmLocal(i));
        //         }
        //         _ => unreachable!(),
        //     }
        // }

        // Load in-register parameters into the allocated locations.
        // Locals are allocated on the stack from higher address to lower address,
        // so we won't skip the stack guard page here.
        for i in 0..n_params {
            let loc = Self::get_param_location(i + 1);
            a.emit_move(Size::S64, loc, locations[i])
        }

        // // Load vmctx into R15.
        // a.emit_mov(
        //     Size::S64,
        //     Self::get_param_location(0),
        //     Location::GPR(GPR::R15),
        // );

        // // Stack probe.
        // //
        // // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // // so here we probe it explicitly when needed.
        // for i in (n_params..n).step_by(NATIVE_PAGE_SIZE / 8).skip(1) {
        //     a.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        // }

        // // Initialize all normal locals to zero.
        // let mut init_stack_loc_cnt = 0;
        // let mut last_stack_loc = Location::Memory(GPR::RBP, i32::MAX);
        // for i in n_params..n {
        //     match locations[i] {
        //         Location::Memory(_, _) => {
        //             init_stack_loc_cnt += 1;
        //             last_stack_loc = cmp::min(last_stack_loc, locations[i]);
        //         }
        //         Location::GPR(_) => {
        //             a.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        //         }
        //         _ => unreachable!(),
        //     }
        // }
        // if init_stack_loc_cnt > 0 {
        //     // Since these assemblies take up to 24 bytes, if more than 2 slots are initialized, then they are smaller.
        //     a.emit_mov(
        //         Size::S64,
        //         Location::Imm64(init_stack_loc_cnt as u64),
        //         Location::GPR(GPR::RCX),
        //     );
        //     a.emit_xor(Size::S64, Location::GPR(GPR::RAX), Location::GPR(GPR::RAX));
        //     a.emit_lea(Size::S64, last_stack_loc, Location::GPR(GPR::RDI));
        //     a.emit_rep_stosq();
        // }

        // // Add the size of all locals allocated to stack.
        // self.stack_offset.0 += static_area_size - callee_saved_regs_size;

        locations
    }

    /// Acquires locations from the machine state.
    ///
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    fn acquire_locations(
        &mut self,
        assembler: &mut Assembler,
        tys: &[(WpType, MachineValue)],
        zeroed: bool,
    ) -> SmallVec<[Location; 1]> {
        let mut ret = smallvec![];
        let mut delta_stack_offset: usize = 0;

        for (ty, mv) in tys {
            let loc = self.pick_reg(*ty).map(Location::Reg);
            let loc = if let Some(x) = loc {
                x
            } else {
                self.stack_offset.0 += 8;
                delta_stack_offset += 8;
                Location::Memory(FP, -(self.stack_offset.0 as i32))
            };
            if let Location::Reg(x) = loc {
                self.used_regs.insert(x);
                self.state.register_values[x.to_index().unwrap()] = mv.clone();
            } else {
                self.state.stack_values.push(mv.clone());
            }
            self.state.wasm_stack.push(WasmAbstractValue::Runtime);
            ret.push(loc);
        }

        if delta_stack_offset != 0 {
            unimplemented!("can't do stack locations yet");
            // assembler.emit_sub(
            //     Size::S64,
            //     Location::Imm32(delta_stack_offset as u32),
            //     Location::GPR(GPR::RSP),
            // );
        }
        // if zeroed {
        //     for i in 0..tys.len() {
        //         assembler.emit_mov(Size::S64, Location::Imm32(0), ret[i]);
        //     }
        // }
        ret
    }

    /// Releases locations used for stack value.
    fn release_locations(&mut self, assembler: &mut Assembler, locs: &[Location]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::Reg(x) => {
                    assert_eq!(self.used_regs.remove(&x), true);
                    self.state.register_values[x.to_index().unwrap()] = MachineValue::Undefined;
                },
                Location::Memory(FP, x) => {
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
                },
                _ => {}
            }
            self.state.wasm_stack.pop().unwrap();
        }

        if delta_stack_offset != 0 {
            assembler.emit_add(
                Size::S64,
                Location::Imm32(delta_stack_offset as u32),
                Location::Reg(SP),
            );
        }
    }

    fn finalize_stack(&mut self, a: &mut Self::Emitter, locations: &[Location]) {
        // // Unwind stack to the "save area".
        // a.emit_lea(
        //     Size::S64,
        //     Location::Memory(
        //         GPR::RBP,
        //         -(self.save_area_offset.as_ref().unwrap().0 as i32),
        //     ),
        //     Location::GPR(GPR::RSP),
        // );

        // // Restore R15 used by vmctx.
        // a.emit_pop(Size::S64, Location::GPR(GPR::R15));

        // // Restore callee-saved registers.
        // for loc in locations.iter().rev() {
        //     if let Location::GPR(_) = *loc {
        //         a.emit_pop(Size::S64, *loc);
        //     }
        // }
    }

    // pub fn get_param_location(idx: usize) -> Location {
    //     match idx {
    //         0 => Location::GPR(GPR::RDI),
    //         1 => Location::GPR(GPR::RSI),
    //         2 => Location::GPR(GPR::RDX),
    //         3 => Location::GPR(GPR::RCX),
    //         4 => Location::GPR(GPR::R8),
    //         5 => Location::GPR(GPR::R9),
    //         _ => Location::Memory(GPR::RBP, (16 + (idx - 6) * 8) as i32),
    //     }
    // }
}