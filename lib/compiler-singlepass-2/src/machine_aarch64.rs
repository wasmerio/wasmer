use crate::common_decl::*;
use crate::emitter::*;
use crate::machine::*;

use dynasmrt::aarch64::Assembler;
use dynasmrt::{dynasm, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};

// use crate::x64_decl::{new_machine_state, X64Register};
use smallvec::smallvec;
use smallvec::SmallVec;
// use std::cmp;
use std::collections::HashSet;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer::Value;
use array_init::array_init;

// const NATIVE_PAGE_SIZE: usize = 4096;

use std::rc::{Rc, Weak};

use std::collections::{BTreeMap};


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

// impl Location {
//     pub fn new(loc: LocationDetail) -> Self {
//         Location {
//             loc,
//             active: false,
//         }
//     }
// }

impl MaybeImmediate for Location {
    fn imm_value(&self) -> Option<Value> {
        match *self {
            Location::Imm32(imm) => Some(Value::I32(imm as i32)),
            _ => None
        }
    }
}

// pub struct Machine {
//     used_xmms: HashSet<XMM>,
//     pub(crate) track_state: bool,
// }


pub struct Aarch64Machine {
    regs: [WeakLocal<Location>; 29],
    reg_counter: u32,
    free_regs: Vec<u32>,
    free_callee_save: Vec<u32>,
    free_stack: Vec<i32>,
    stack_offset: i32,

    state: MachineState,
}

impl Aarch64Machine {
    pub fn new() -> Self {
        Self {
            regs: array_init(|_| WeakLocal::new()),
            reg_counter: 0,
            free_regs: vec![8,9,10,11,12,13,14,15,16,17,18],
            free_callee_save: vec![],
            free_stack: vec![],
            stack_offset: 0,

            state: Self::new_state(),
        }
    }

    fn prep_bin_op(&mut self, a: &mut Assembler,
        src1: Local<Location>, src2: Local<Location>, commutative: bool) ->
        (Local<Location>, Local<Location>, Local<Location>) {
        
        let (src1, src2) = match (src1.location(), src2.location(), commutative) {
            (Location::Imm32(_), Location::Imm32(_), _) => {
                self.move_to_reg(a, src1.clone(), &[]);
                (src1, src2)
            },
            (_, Location::Imm32(_), _) => {
                self.move_to_reg(a, src1.clone(), &[]);
                (src1, src2)
            },
            (Location::Imm32(_), _, true) => {
                self.move_to_reg(a, src2.clone(), &[]);
                (src2, src1)
            },
            (_, Location::Reg(Reg::X(r2)), _) => {
                self.move_to_reg(a, src1.clone(), &[r2]);
                let r1 = if let Location::Reg(Reg::X(r1)) = src1.location() {
                    r1
                } else {
                    unreachable!();
                };
                self.move_to_reg(a, src2.clone(), &[r1]);
                (src1, src2)
            }
            _ => {
                self.move_to_reg(a, src1.clone(), &[]);
                let r1 = if let Location::Reg(Reg::X(r1)) = src1.location() {
                    r1
                } else {
                    unreachable!();
                };
                self.move_to_reg(a, src2.clone(), &[r1]);
                (src1, src2)
            }
        };
        
        if src1.ref_ct() < 2 {
            (src1.clone(), src2, src1)
        } else if src2.ref_ct() < 2 {
            (src1, src2.clone(), src2)
        } else {
            let mut dont_use: SmallVec<[_; 2]> = SmallVec::new();
            if let Location::Reg(Reg::X(r1)) = src1.location() {
                dont_use.push(r1)
            }
            if let Location::Reg(Reg::X(r2)) = src2.location() {
                dont_use.push(r2)
            }
            let dst = self.get_free_reg(a, &dont_use);
            let dst = Local::new(Location::Reg(dst));
            (src1, src2, dst)
        }
    }

    fn get_param_location(i: usize) -> Location {
        match i {
            0..=7 => Location::Reg(Reg::X(i as u32)),
            _ => Location::Memory(FP, 16 + (i-8) as i32 * 8),
        }
    }

    // assumes the returned stack index will be used
    fn get_free_stack(&mut self, a: &mut Assembler) -> i32 {
        if let Some(idx) = self.free_stack.pop() {
            return idx;
        }

        let idx = self.stack_offset - 8;
        self.stack_offset -= 16;
        self.free_stack.push(self.stack_offset);
        dynasm!(a ; sub sp, sp, 16);
        
        idx
    }

    // loc.target must be a numbered register
    fn move_to_stack(&mut self, a: &mut Assembler, loc: Local<Location>) {
        let stack = self.get_free_stack(a);
        
        match loc.location() {
            Location::Reg(Reg::X(reg)) => {
                dynasm!(a ; .arch aarch64 ; stur X(reg), [x29, stack]);
            }
            _ => {
                unreachable!();
            }
        }

        loc.replace_location(Location::Memory(FP, stack));
    }

    // assumes the returned reg will be used
    fn get_free_reg(&mut self, a: &mut Assembler, dont_use: &[u32]) -> Reg {
        let reg = if let Some(reg) = self.free_callee_save.pop() {
            reg
        } else if let Some(reg) = self.free_regs.pop() {
            reg
        } else {
            while dont_use.contains(&self.reg_counter) {
                self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u32;
            }
            
            let reg = self.reg_counter;
            self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u32;
            match self.regs[reg as usize].upgrade() {
                Some(loc) => {
                    self.move_to_stack(a, loc);
                },
                _ => {
                    unreachable!();
                }
            }

            reg as u32
        };

        Reg::X(reg)
    }
    
    fn move_to_reg(&mut self, a: &mut Assembler, loc: Local<Location>, dont_use: &[u32]) {
        // {
        //     let t = loc.location();
        //     println!("{:?}", t);
        // }

        if let Location::Reg(_) = loc.location() {
            return;
        }
        
        let reg = self.get_free_reg(a, dont_use);
        match (loc.location(), reg) {
            (Location::Imm32(n), Reg::X(reg)) => {
                dynasm!(a; .arch aarch64; mov X(reg), n as u64);
            },
            (Location::Memory(FP, n), Reg::X(reg)) => {
                // dynasm!(a; sub sp, sp, 1; ldp x29, x30, [sp]);
                dynasm!(a; .arch aarch64; ldur X(reg), [x29, n]);
                self.free_stack.push(n);
            },
            _ => {
                unreachable!();
            }
        }

        self.regs[reg.to_index().unwrap()] = loc.downgrade();
        loc.replace_location(Location::Reg(reg));
    }
}

// pub struct Aarch64Machine2 {
//     used_regs: HashSet<Reg>,
//     stack_offset: MachineStackOffset,
//     state: MachineState,
//     save_area_offset: Option<MachineStackOffset>,
//     free_stack: Vec<i32>,
// }

// impl Aarch64Machine2 {
//     pub fn new() -> Self {
//         Self {
//             state: Self::new_state(),
//             used_regs: HashSet::new(),
//             stack_offset: MachineStackOffset(0),
//             save_area_offset: None,
//             free_stack: vec![]
//         }
//     }

//     /// Picks an unused general purpose register for local/stack/argument use.
//     ///
//     /// This method does not mark the register as used.
//     fn pick_reg(&self, ty: WpType) -> Option<Reg> {
//         static REGS: &[Reg] = &[X0, X1, X2, X3, X4, X5, X6, X7];
//         for r in REGS {
//             if !self.used_regs.contains(r) {
//                 return Some(*r);
//             }
//         }
//         None
//     }
// }

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

    fn imm32(&mut self, a: &mut Assembler, n: u32) -> Local<Location> {
        Local::new(Location::Imm32(n))
    }

    fn release_location(&mut self, loc: Local<Location>) {
        match loc.location() {
            Location::Reg(Reg::X(n)) => {
                self.regs[n as usize] = WeakLocal::new();
                if n < 19 {
                    self.free_regs.push(n);
                } else {
                    self.free_callee_save.push(n);
                }
            },
            Location::Memory(FP, offset) => {
                self.free_stack.push(offset);
            },
            Location::Imm32(_) => {},
            _ => {
                unreachable!()
            },
        }
    }

    // N.B. `n_locals` includes `n_params`; `n_locals` will always be >= `n_params`
    fn init_locals(&mut self, a: &mut Self::Emitter, n_locals: usize, n_params: usize) -> Vec<Local<Location>> {
        let mut locals: Vec<Local<Location>> = Vec::with_capacity(n_locals);
        for i in 0..n_params {
            let loc = Local::new(Self::get_param_location(i + 1));
            if let Location::Reg(Reg::X(n)) = loc.location() {
                self.regs[n as usize] = loc.downgrade()
            }
            locals.push(loc);
        }
        for _ in n_params..n_locals {
            locals.push(Local::new(Location::Imm32(0)));
        }
        for r in (n_params + 1)..=7 {
            self.free_regs.push(r as u32);
        }

        // // Save R15 for vmctx use.
        // self.stack_offset.0 += 8;
        // a.emit_move(
        //     Size::S64,
        //     Location::Reg(X28),
        //     Location::Memory(FP, -(self.stack_offset.0 as i32)),
        // );
        // self.state.stack_values.push(MachineValue::PreserveRegister(
        //     RegisterIndex(X28.to_index().unwrap()))
        // );

        // // Save the offset of register save area.
        // self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // // Save location information for locals.
        // for (i, loc) in locations.iter().enumerate() {
        //     match *loc {
        //         Location::Reg(x) => {
        //             self.state.register_values[x.to_index().unwrap()] =
        //                 MachineValue::WasmLocal(i);
        //         }
        //         Location::Memory(_, _) => {
        //             self.state.stack_values.push(MachineValue::WasmLocal(i));
        //         }
        //         _ => unreachable!(),
        //     }
        // }

        // // Load in-register parameters into the allocated locations.
        // // Locals are allocated on the stack from higher address to lower address,
        // // so we won't skip the stack guard page here.
        // for i in 0..n_params {
        //     let loc = Self::get_param_location(i + 1);
        //     a.emit_move(Size::S64, loc, locations[i])
        // }

        // // Load vmctx into R15.
        // a.emit_move(
        //     Size::S64,
        //     Self::get_param_location(0),
        //     Location::Reg(X28),
        // );

        // // // Stack probe.
        // // //
        // // // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // // // so here we probe it explicitly when needed.
        // // for i in (n_params..n).step_by(NATIVE_PAGE_SIZE / 8).skip(1) {
        // //     a.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        // // }

        // // // Initialize all normal locals to zero.
        // // let mut init_stack_loc_cnt = 0;
        // // let mut last_stack_loc = Location::Memory(GPR::RBP, i32::MAX);
        // // for i in n_params..n {
        // //     match locations[i] {
        // //         Location::Memory(_, _) => {
        // //             init_stack_loc_cnt += 1;
        // //             last_stack_loc = cmp::min(last_stack_loc, locations[i]);
        // //         }
        // //         Location::GPR(_) => {
        // //             a.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        // //         }
        // //         _ => unreachable!(),
        // //     }
        // // }
        // // if init_stack_loc_cnt > 0 {
        // //     // Since these assemblies take up to 24 bytes, if more than 2 slots are initialized, then they are smaller.
        // //     a.emit_mov(
        // //         Size::S64,
        // //         Location::Imm64(init_stack_loc_cnt as u64),
        // //         Location::GPR(GPR::RCX),
        // //     );
        // //     a.emit_xor(Size::S64, Location::GPR(GPR::RAX), Location::GPR(GPR::RAX));
        // //     a.emit_lea(Size::S64, last_stack_loc, Location::GPR(GPR::RDI));
        // //     a.emit_rep_stosq();
        // // }

        // // Add the size of all locals allocated to stack.
        // self.stack_offset.0 += static_area_size - callee_saved_regs_size;

        // dynasm!(a; mov x0, x1);
        // dynasm!(a; add x0, x0, x2);
        // dynasm!(a; add x0, x0, x3);
        // dynasm!(a; add x0, x0, x4);
        // dynasm!(a; add x0, x0, x5);
        // dynasm!(a; add x0, x0, x6);
        // dynasm!(a; add x0, x0, x7);
        // dynasm!(a; ldur x18, [x29, 16]);
        // dynasm!(a; add x0, x0, x18);
        // dynasm!(a; ldur x18, [x29, 24]);
        // dynasm!(a; add x0, x0, x18);
        // dynasm!(a; ldur x18, [x29, 32]);
        // dynasm!(a; add x0, x0, x18);
        // dynasm!(a; ldur x18, [x29, 40]);
        // dynasm!(a; add x0, x0, x18);
        // dynasm!(a; ldur x18, [x29, 48]);
        // dynasm!(a; add x0, x0, x18);
        locals
    }

    // /// Acquires locations from the machine state.
    // ///
    // /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    // /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    // fn acquire_locations(
    //     &mut self,
    //     assembler: &mut Assembler,
    //     tys: &[(WpType, MachineValue)],
    //     zeroed: bool,
    // ) -> SmallVec<[Location; 1]> {
    //     let mut ret = smallvec![];
    //     let mut delta_stack_offset: usize = 0;

    //     for (ty, mv) in tys {
    //         let loc = self.pick_reg(*ty).map(Location::Reg);
    //         let loc = if let Some(x) = loc {
    //             x
    //         } else {
    //             self.stack_offset.0 += 8;
    //             delta_stack_offset += 8;
    //             Location::Memory(FP, -(self.stack_offset.0 as i32))
    //         };
    //         if let Location::Reg(x) = loc {
    //             self.used_regs.insert(x);
    //             self.state.register_values[x.to_index().unwrap()] = mv.clone();
    //         } else {
    //             self.state.stack_values.push(mv.clone());
    //         }
    //         self.state.wasm_stack.push(WasmAbstractValue::Runtime);
    //         ret.push(loc);
    //     }

    //     if delta_stack_offset != 0 {
    //         unimplemented!("can't do stack locations yet");
    //         // assembler.emit_sub(
    //         //     Size::S64,
    //         //     Location::Imm32(delta_stack_offset as u32),
    //         //     Location::GPR(GPR::RSP),
    //         // );
    //     }
    //     // if zeroed {
    //     //     for i in 0..tys.len() {
    //     //         assembler.emit_mov(Size::S64, Location::Imm32(0), ret[i]);
    //     //     }
    //     // }
    //     ret
    // }

    // /// Releases locations used for stack value.
    // fn release_locations(&mut self, assembler: &mut Assembler, locs: &[Location]) {
    //     let mut delta_stack_offset: usize = 0;

    //     for loc in locs.iter().rev() {
    //         match *loc {
    //             Location::Reg(x) => {
    //                 assert_eq!(self.used_regs.remove(&x), true);
    //                 self.state.register_values[x.to_index().unwrap()] = MachineValue::Undefined;
    //             },
    //             Location::Memory(FP, x) => {
    //                 if x >= 0 {
    //                     unreachable!();
    //                 }
    //                 let offset = (-x) as usize;
    //                 if offset != self.stack_offset.0 {
    //                     unreachable!();
    //                 }
    //                 self.stack_offset.0 -= 8;
    //                 delta_stack_offset += 8;
    //                 self.state.stack_values.pop().unwrap();
    //             },
    //             _ => {}
    //         }
    //         self.state.wasm_stack.pop().unwrap();
    //     }

    //     if delta_stack_offset != 0 {
    //         assembler.emit_add(
    //             Size::S64,
    //             Location::Reg(SP),
    //             Location::Imm32(delta_stack_offset as u32),
    //             Location::Reg(SP),
    //         );
    //     }
    // }

    fn finalize_stack(&mut self, a: &mut Self::Emitter, locations: &[Local<Location>]) {
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
        //     if let Location::Reg(_) = *loc {
        //         // a.emit_pop(Size::S64, *loc);
        //         unimplemented!();
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

    fn emit_add_i32(&mut self, a: &mut Assembler, sz: Size,
        src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(a, src1, src2, true);
        a.emit_add_i32(sz, src1.location(), src2.location(), dst.location());
        dst
    }

    fn emit_sub_i32(&mut self, a: &mut Assembler, sz: Size,
        src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(a, src1, src2, false);
        a.emit_sub_i32(sz, src1.location(), src2.location(), dst.location());
        dst
    }
}