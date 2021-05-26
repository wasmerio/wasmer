use crate::common_decl::*;
use crate::machine::*;
use crate::codegen::{Local, WeakLocal};

use dynasmrt::aarch64::Assembler;
use dynasmrt::{dynasm, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmer_types::{FunctionType, FunctionIndex};
use wasmer_vm::VMOffsets;

use smallvec::{smallvec, SmallVec};
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer::Value;
use array_init::array_init;

// const NATIVE_PAGE_SIZE: usize = 4096;

use std::collections::{BTreeMap};
use std::cmp::min;

use wasmer_compiler::{Relocation, RelocationTarget, RelocationKind};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Reg {
    X(u32),
    SP,
    XZR,
}

impl Reg {
    fn reg_index(&self) -> Option<u32> {
        if let Reg::X(n) = *self {
            Some(n)
        } else {
            None
        }
    }
}

// impl Reg {
//     pub const REG_COUNT: usize = 32;

//     pub fn to_index(self) -> Option<usize> {
//         match self {
//             Reg::X(n) => {
//                 if n < 31 {
//                     Some(n as usize)
//                 } else {
//                     None
//                 }
//             },
//             XZR => Some(31),
//             SP => Some(32),
//         }
//     }

//     pub fn from_index(n: usize) -> Option<Reg> {
//         match n {
//             0..=30 => Some(Reg::X(n as u32)),
//             31 => Some(XZR),
//             32 => Some(SP),
//             _ => None
//         }
//     }
// }

pub const X0:  Reg = Reg::X(0);
pub const X1:  Reg = Reg::X(1);
pub const X2:  Reg = Reg::X(2);
// pub const X3:  Reg = Reg::X(3);
// pub const X4:  Reg = Reg::X(4);
// pub const X5:  Reg = Reg::X(5);
// pub const X6:  Reg = Reg::X(6);
// pub const X7:  Reg = Reg::X(7);
// pub const X8:  Reg = Reg::X(8);
// pub const X9:  Reg = Reg::X(9);
// pub const X10: Reg = Reg::X(10);
// pub const X11: Reg = Reg::X(11);
// pub const X12: Reg = Reg::X(12);
// pub const X13: Reg = Reg::X(13);
// pub const X14: Reg = Reg::X(14);
// pub const X15: Reg = Reg::X(15);
// pub const X16: Reg = Reg::X(16);
// pub const X17: Reg = Reg::X(17);
// pub const X18: Reg = Reg::X(18);
pub const X19: Reg = Reg::X(19);
pub const X20: Reg = Reg::X(20);
// pub const X21: Reg = Reg::X(21);
// pub const X22: Reg = Reg::X(22);
// pub const X23: Reg = Reg::X(23);
// pub const X24: Reg = Reg::X(24);
// pub const X25: Reg = Reg::X(25);
// pub const X26: Reg = Reg::X(26);
// pub const X27: Reg = Reg::X(27);
// pub const X28: Reg = Reg::X(28);
// pub const X29: Reg = Reg::X(29);
pub const X30: Reg = Reg::X(30);
// pub const XZR: Reg = Reg::XZR;
pub const SP:  Reg = Reg::SP;
pub const FP:  Reg = Reg::X(29);
pub const VMCTX: Reg = Reg::X(28);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Location {
    Imm32(u32),
    Reg(Reg),
    Memory(Reg, i32),
}

impl MaybeImmediate for Location {
    fn imm_value(&self) -> Option<Value> {
        match *self {
            Location::Imm32(imm) => Some(Value::I32(imm as i32)),
            _ => None
        }
    }
}

pub struct Aarch64Machine {
    assembler: Assembler,
    n_stack_params: usize,
    regs: [WeakLocal<Location>; 28], // x28, x29, and x30 are off limits!
    stack: Vec<WeakLocal<Location>>,
    reg_counter: u32,
    free_regs: Vec<u32>,
    free_callee_save: Vec<u32>,
    free_stack: Vec<i32>,
    stack_offset: i32,
    stack_offsets: Vec<i32>,
    relocation_info: Vec<(RelocationTarget, DynamicLabel)>,

    state: MachineState,
}

macro_rules! do_bin_op_i32 {
    ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident, $imm_ty:ty) => {
        let (src1_reg, dst_reg) = 
            if let (Location::Reg(Reg::X(src1_reg)), Location::Reg(Reg::X(dst_reg))) = 
                ($src1.location(), $dst.location()) {
                (src1_reg, dst_reg)
            } else {
                unreachable!();
            };

        match $src2.location() {
            Location::Reg(Reg::X(src2_reg)) => {
                dynasm!($self.assembler ; .arch aarch64 ; $op W(dst_reg), W(src1_reg), W(src2_reg));
            },
            Location::Imm32(src2_val) => {
                dynasm!($self.assembler ; .arch aarch64 ; $op W(dst_reg), W(src1_reg), src2_val as $imm_ty);
            },
            _ => {  
                unreachable!()
            },
        }
    }
}

macro_rules! do_bin_op_i64 {
    ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident, $imm_ty:ty) => {
        let (src1_reg, dst_reg) = 
            if let (Location::Reg(Reg::X(src1_reg)), Location::Reg(Reg::X(dst_reg))) = 
                ($src1.location(), $dst.location()) {
                (src1_reg, dst_reg)
            } else {
                unreachable!();
            };

        match $src2.location() {
            Location::Reg(Reg::X(src2_reg)) => {
                dynasm!($self.assembler ; .arch aarch64 ; $op X(dst_reg), X(src1_reg), X(src2_reg));
            },
            Location::Imm32(src2_val) => {
                dynasm!($self.assembler ; .arch aarch64 ; $op X(dst_reg), X(src1_reg), src2_val as $imm_ty);
            },
            _ => {  
                unreachable!()
            },
        }
    }
}

macro_rules! do_bin_op_i32_no_imm {
    ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident) => {
        let (src1_reg, dst_reg) = 
            if let (Location::Reg(Reg::X(src1_reg)), Location::Reg(Reg::X(dst_reg))) = 
                ($src1.location(), $dst.location()) {
                (src1_reg, dst_reg)
            } else {
                unreachable!();
            };

        match $src2.location() {
            Location::Reg(Reg::X(src2_reg)) => {
                dynasm!($self.assembler ; .arch aarch64 ; $op W(dst_reg), W(src1_reg), W(src2_reg));
            },
            _ => {  
                unreachable!()
            },
        }
    }
}

macro_rules! do_cmp_op_i32 {
    ($self:ident, $op:tt, $src1:ident, $src2:ident, $dst:ident) => {
        let (src1_reg, dst_reg) = 
            if let (Location::Reg(Reg::X(src1_reg)), Location::Reg(Reg::X(dst_reg))) = 
                ($src1.location(), $dst.location()) {
                (src1_reg, dst_reg)
            } else {
                unreachable!();
            };

        match $src2.location() {
            Location::Reg(Reg::X(src2_reg)) => {
                dynasm!($self.assembler ; .arch aarch64 ; cmp W(src1_reg), W(src2_reg));
            },
            Location::Reg(Reg::XZR) => {
                dynasm!($self.assembler ; .arch aarch64 ; cmp W(src1_reg), wzr);
            },
            Location::Imm32(src2_val) => {
                dynasm!($self.assembler ; .arch aarch64 ; cmp W(src1_reg), src2_val);
            },
            _ => {
                unreachable!()
            }
        }

        dynasm!($self.assembler ; .arch aarch64 ; cset X(dst_reg), $op);
    }
}

impl Aarch64Machine {
    pub fn new() -> Self {
        Self {
            assembler: Assembler::new().unwrap(),
            regs: array_init(|_| WeakLocal::new()),
            stack: Vec::new(),
            reg_counter: 0,
            n_stack_params: 0,
            free_regs: vec![8,9,10,11,12,13,14,15,16,17,18],
            free_callee_save: vec![/*19,20,21,22,23,24,25,26,27*/],
            free_stack: vec![],
            stack_offset: 0,
            stack_offsets: vec![],
            relocation_info: vec![],

            state: Self::new_state(),
        }
    }

    fn prep_unary_op(&mut self, src: Local<Location>) -> (Local<Location>, Local<Location>) {
        let r = self.move_to_reg(src.clone(), &[]);
        if src.ref_ct() < 1 {
            (src.clone(), src)
        } else {
            let dst = self.get_free_reg(&[r]);
            (src, self.new_local_from_reg(dst))
        }
    }

    fn stack_idx(&self, offset: i32) -> usize {
        if offset >= 0 {
            (offset / 8) as usize
        } else {
            self.n_stack_params + (offset / -8) as usize - 1 
        }
    }

    fn prep_bin_op_no_imm(&mut self,
        src1: Local<Location>, src2: Local<Location>) ->
        (Local<Location>, Local<Location>, Local<Location>) {
        let r1 = self.move_to_reg(src1.clone(), &[]);
        let r2 = self.move_to_reg(src2.clone(), &[r1]);

        if src1.ref_ct() < 1 {
            (src1.clone(), src2, src1)
        } else if src2.ref_ct() < 1 {
            (src1, src2.clone(), src2)
        } else {
            let dst = self.get_free_reg(&[r1, r2]);
            (src1, src2, self.new_local_from_reg(dst))
        }
    }

    fn prep_bin_op(&mut self,
        src1: Local<Location>, src2: Local<Location>, commutative: bool, imm_bits: u8) ->
        (Local<Location>, Local<Location>, Local<Location>) {
        
        let (src1, src2) = match (src1.location(), src2.location(), commutative) {
            (Location::Imm32(_), Location::Imm32(_), _) => {
                self.move_to_reg(src1.clone(), &[]);
                (src1, src2)
            },
            (_, Location::Imm32(_), _) => {
                self.move_to_reg(src1.clone(), &[]);
                (src1, src2)
            },
            (Location::Imm32(_), _, true) => {
                self.move_to_reg(src2.clone(), &[]);
                (src2, src1)
            },
            (_, Location::Reg(Reg::X(r2)), _) => {
                let r1 = self.move_to_reg(src1.clone(), &[r2]);
                self.move_to_reg(src2.clone(), &[r1]);
                (src1, src2)
            }
            _ => {
                let r1 = self.move_to_reg(src1.clone(), &[]);
                self.move_to_reg(src2.clone(), &[r1]);
                (src1, src2)
            }
        };
        
        let src2_is_imm = if let Location::Imm32(_) = src2.location() { true } else { false };

        if let Some(Value::I32(val)) = src2.location().imm_value() {
            let mask: u32 = 0xffffffff << imm_bits;
            
            if mask & val as u32 != 0 {
                // value is too large to be represented as an immediate value
                let r1 = if let Location::Reg(Reg::X(r1)) = src1.location() {r1} else {unreachable!()};
                self.move_to_reg(src2.clone(), &[r1]);
            }
        }

        if src1.ref_ct() < 1 {
            (src1.clone(), src2, src1)
        } else if !src2_is_imm && src2.ref_ct() < 1 {
            (src1, src2.clone(), src2)
        } else {
            let mut dont_use: SmallVec<[_; 2]> = SmallVec::new();
            if let Location::Reg(Reg::X(r1)) = src1.location() {
                dont_use.push(r1)
            }
            if let Location::Reg(Reg::X(r2)) = src2.location() {
                dont_use.push(r2)
            }
            let dst = self.get_free_reg(&dont_use);
            (src1, src2, self.new_local_from_reg(dst))
        }
    }

    fn get_param_location(i: usize) -> Location {
        match i {
            0..=7 => Location::Reg(Reg::X(i as u32)),
            _ => Location::Memory(FP, 32 + (i-8) as i32 * 8),
        }
    }

    // assumes the returned stack index will be used
    fn get_free_stack(&mut self) -> i32 {
        if let Some(idx) = self.free_stack.pop() {
            return idx;
        }

        let idx = self.stack_offset - 8;
        self.stack_offset -= 16;
        self.free_stack.push(self.stack_offset);
        self.stack.push(WeakLocal::new());
        self.stack.push(WeakLocal::new());

        dynasm!(self.assembler ; sub sp, sp, 16);
        
        idx
    }

    // loc.target must be a numbered register
    fn move_to_stack(&mut self, loc: Local<Location>) {
        let stack = self.get_free_stack();
        
        match loc.location() {
            Location::Reg(Reg::X(reg)) => {
                dynasm!(self.assembler ; .arch aarch64 ; stur X(reg), [x29, stack]);
            }
            _ => {
                unreachable!();
            }
        }

        self.release_location(loc.clone());
        loc.replace_location(Location::Memory(FP, stack));
        let idx = self.stack_idx(stack);
        self.stack[idx] = loc.downgrade();
    }

    // assumes the returned reg will be eventually released with Machine::release_location()
    fn get_free_reg(&mut self, dont_use: &[u32]) -> u32 {
        if let Some(reg) = self.free_callee_save.pop() {
            reg
        } else if let Some(reg) = self.free_regs.pop() {
            reg
        } else {
            // better not put all the regs in here or this loop will deadlock!!!
            while dont_use.contains(&self.reg_counter) {
                self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u32;
            }
            
            let reg = self.reg_counter;
            self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u32;
            match self.regs[reg as usize].upgrade() {
                Some(loc) => {
                    self.move_to_stack(loc);
                    return self.get_free_reg(dont_use);
                },
                _ => {
                    unreachable!();
                }
            }
        }
    }

    fn new_local_from_reg(&mut self, reg: u32) -> Local<Location> {
        let local = Local::new(Location::Reg(Reg::X(reg)));
        self.regs[reg as usize] = local.downgrade();
        local
    }
    
    fn move_to_reg(&mut self, loc: Local<Location>, dont_use: &[u32]) -> u32 {
        if let Location::Reg(Reg::X(reg)) = loc.location() {
            return reg;
        }
        
        let reg = self.get_free_reg(dont_use);
        match loc.location() {
            Location::Imm32(n) => {
                if n & 0xffff0000 != 0 {
                    dynasm!(self.assembler; .arch aarch64; mov X(reg), (n & 0xffff) as u64);
                    dynasm!(self.assembler; .arch aarch64; movk X(reg), n >> 16, LSL 16);
                } else {
                    dynasm!(self.assembler; .arch aarch64; mov X(reg), n as u64);
                }
            },
            Location::Memory(Reg::X(base_reg), n) => {
                dynasm!(self.assembler; .arch aarch64; ldur X(reg), [X(base_reg), n]);
                if base_reg == FP.reg_index().unwrap() {
                    self.free_stack.push(n);
                }
            },
            _ => {
                unreachable!();
            },
        }

        self.regs[reg as usize] = loc.downgrade();
        loc.replace_location(Location::Reg(Reg::X(reg)));

        reg
    }

    fn move_data(&mut self, sz: Size, src: Location, dst: Location) {
        match (src, dst) {
            // reg -> reg
            (Location::Reg(src), Location::Reg(dst)) => {
                match (src, dst) {
                    (Reg::X(src), Reg::X(dst)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; mov X(dst), X(src));
                    },
                    (Reg::XZR, Reg::X(dst)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; mov X(dst), xzr);
                    },
                    (Reg::SP, Reg::X(dst)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; mov X(dst), sp);
                    },
                    (Reg::X(src), Reg::SP) => {
                        dynasm!(self.assembler ; .arch aarch64 ; mov sp, X(src));
                    },
                    _ => unreachable!()
                }
            },
            // imm -> reg
            (Location::Imm32(src), Location::Reg(dst)) => {
                match dst {
                    Reg::X(dst) => {
                        dynasm!(self.assembler ; .arch aarch64 ; mov X(dst), src as u64);
                    },
                    _ => unreachable!()
                }
            },
            // mem -> reg
            (Location::Memory(reg, idx), Location::Reg(dst)) => {
                match (reg, dst) {
                    (Reg::X(reg), Reg::X(dst)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; ldur X(dst), [X(reg), idx]);
                    },
                    (Reg::SP, Reg::X(dst)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; ldur X(dst), [sp, idx]);
                    },
                    _ => unreachable!()
                }
            },
            // reg -> mem
            (Location::Reg(src), Location::Memory(reg, idx)) => {
                match (src, reg) {
                    (Reg::X(src), Reg::X(reg)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(src), [X(reg), idx]);
                    },
                    (Reg::X(src), Reg::SP) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(src), [sp, idx]);
                    },
                    (Reg::XZR, Reg::X(reg)) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur xzr, [X(reg), idx]);
                    },
                    (Reg::XZR, Reg::SP) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur xzr, [sp, idx]);
                    },
                    _ => unreachable!()
                }
            },
            // imm -> mem
            (Location::Imm32(src), Location::Memory(reg, idx)) => {
                let mut dont_use: SmallVec<[u32; 1]> = smallvec![];
                if let Some(r) = reg.reg_index() {
                    dont_use.push(r);
                }
                let tmp = self.get_free_reg(&dont_use);
                dynasm!(self.assembler ; .arch aarch64 ; mov X(tmp), src as u64);
                match reg {
                    Reg::X(reg) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(tmp), [X(reg), idx]);
                    },
                    Reg::SP => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(tmp), [sp, idx]);
                    },
                    _ => unreachable!()
                }
                self.release_location(Local::new(Location::Reg(Reg::X(tmp))));
            },
            // mem -> mem
            (Location::Memory(src, src_idx), Location::Memory(dst, dst_idx)) => {
                let mut dont_use: SmallVec<[u32; 2]> = smallvec![];
                if let Some(r) = src.reg_index() {
                    dont_use.push(r);
                }
                if let Some(r) = dst.reg_index() {
                    dont_use.push(r);
                }
                let tmp = self.get_free_reg(&dont_use);
                match src {
                    Reg::X(src) => {
                        dynasm!(self.assembler ; .arch aarch64 ; ldur X(tmp), [X(src), src_idx]);
                    },
                    Reg::SP => {
                        dynasm!(self.assembler ; .arch aarch64 ; ldur X(tmp), [sp, src_idx]);
                    },
                    _ => unreachable!()
                }
                match dst {
                    Reg::X(dst) => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(tmp), [X(dst), dst_idx]);
                    },
                    Reg::SP => {
                        dynasm!(self.assembler ; .arch aarch64 ; stur X(tmp), [sp, dst_idx]);
                    },
                    _ => unreachable!()
                }
                self.release_location(Local::new(Location::Reg(Reg::X(tmp))));
            },
            _ => {
                unreachable!();
            },
        }
    }
}

impl Machine for Aarch64Machine {
    type Location = Location;
    type Label = DynamicLabel;
    const BR_INSTR_SIZE: usize = 4;
    
    fn get_assembly_offset(&mut self) -> usize {
        self.assembler.offset().0
    }

    fn new_label(&mut self) -> DynamicLabel {
        self.assembler.new_dynamic_label()
    }

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

    fn do_const_i32(&mut self, n: i32) -> Local<Location> {
        Local::new(Location::Imm32(n as u32))
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
                let idx = self.stack_idx(offset);
                self.stack[idx] = WeakLocal::new();
                self.free_stack.push(offset);
            },
            // Location::Memory(Reg::X(n), _) => {
            //     if n < 19 {
            //         self.free_regs.push(n);
            //     } else {
            //         self.free_callee_save.push(n);
            //     }
            // },
            Location::Imm32(_) => {},
            _ => {
                unreachable!()
            },
        }
    }

    // N.B. `n_locals` includes `n_params`; `n_locals` will always be >= `n_params`
    fn func_begin(&mut self, n_locals: usize, n_params: usize) -> Vec<Local<Location>> {
        // save LR, FP, and VMCTX regs
        dynasm!(&mut self.assembler
            ; .arch aarch64
            ; sub sp, sp, 32
            ; stp x29, x30, [sp]
            ; str x28, [sp, 16]
            ; mov x29, sp
            ; mov x28, x0);
        
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
        self.free_regs.push(0);
        for r in (n_params + 1)..=7 {
            self.free_regs.push(r as u32);
        }

        self.n_stack_params = n_params.saturating_sub(7);

        // // Save R15 for vmctx use.
        // self.stack_offset.0 += 8;
        // self.move_data(
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

        // // // Stack probe.
        // // //
        // // // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // // // so here we probe it explicitly when needed.
        // // for i in (n_params..n).step_by(NATIVE_PAGE_SIZE / 8).skip(1) {
        // //     self.assembler.emit_mov(Size::S64, Location::Imm32(0), locations[i]);
        // // }
        
        locals
    }

    fn func_end(&mut self, end_label: DynamicLabel) -> Vec<Relocation> {
        dynasm!(self.assembler ; .arch aarch64 ; =>end_label);

        // // Make a copy of the return value in XMM0, as required by the SysV CC.
        // match self.signature.results() {
        //     [x] if *x == Type::F32 || *x == Type::F64 => {
        //         self.assembler.emit_mov(
        //             Size::S64,
        //             Location::GPR(GPR::RAX),
        //             Location::XMM(XMM::XMM0),
        //         );
        //     }
        //     _ => {}
        // }
        
        // restore SP
        if self.stack_offset != 0 {
            assert!(self.stack_offset < 0);
            dynasm!(self.assembler ; .arch aarch64 ; add sp, sp, -self.stack_offset as u32);
        }
        
        // restore LR, FP, and VMCTX regs
        dynasm!(self.assembler
            ; .arch aarch64
            ; ldp x29, x30, [sp]
            ; ldr x28, [sp, 16]
            ; add sp, sp, 32
            ; ret);
        
        // reserve space for relocated function pointers
        let mut relocations = vec![];
        for (reloc_target, fn_addr_label) in self.relocation_info.iter().copied() {
            let reloc_at = self.assembler.offset().0 as u32;
            dynasm!(self.assembler ; =>fn_addr_label ; nop ; nop);
            relocations.push(Relocation {
                kind: RelocationKind::Abs8,
                reloc_target,
                offset: reloc_at,
                addend: 0,
            });
        }

        relocations
        // // Unwind stack to the "save area".
        // self.assembler.emit_lea(
        //     Size::S64,
        //     Location::Memory(
        //         GPR::RBP,
        //         -(self.save_area_offset.as_ref().unwrap().0 as i32),
        //     ),
        //     Location::GPR(GPR::RSP),
        // );

        // // Restore callee-saved registers.
        // for loc in locations.iter().rev() {
        //     if let Location::Reg(_) = *loc {
        //         // self.assembler.emit_pop(Size::S64, *loc);
        //         unimplemented!();
        //     }
        // }
    }

    fn block_begin(&mut self) {
        self.stack_offsets.push(self.stack_offset);
    }

    fn block_end(&mut self, end_label: DynamicLabel) {
        let prev_stack_offset = self.stack_offsets.pop().unwrap();
        let diff = -(self.stack_offset - prev_stack_offset) as u32;
        self.stack_offset = prev_stack_offset;

        if diff > 0 {
            dynasm!(self.assembler ; .arch aarch64 ; add sp, sp, diff);
        }
        dynasm!(self.assembler ; .arch aarch64 ; =>end_label);
    }

    fn do_normalize_local(&mut self, local: Local<Location>) -> Local<Location> {
        if let Location::Imm32(n) = local.location() {
            let new_local = Local::new(Location::Imm32(n));
            self.move_to_reg(new_local.clone(), &[]);
            new_local
        } else if local.ref_ct() > 1 {
            let reg = self.get_free_reg(&[]);
            self.move_data(Size::S64, local.location(), Location::Reg(Reg::X(reg)));
            self.new_local_from_reg(reg)
        } else {
            local
        }
    }

    fn do_restore_local(&mut self, local: Local<Location>, location: Location) -> Local<Location> {
        let local = self.do_normalize_local(local);

        if local.location() == location {
            return local;
        }
        
        // we cannot replace the locations of any locals while restoring the locals' states
        // if we must, we will temporarily steal x0 and restore at the end of this function
        let mut x0_restore = None;
        macro_rules! require_free_reg {
            () => {
                {
                    if self.free_regs.len() + self.free_callee_save.len() > 0 {
                    } else if let Some(_) = x0_restore {
                    } else {
                        x0_restore = {
                            let x0_restore = self.regs[0].upgrade().unwrap();
                            self.move_to_stack(x0_restore.clone());
                            assert!(self.free_regs.len() == 1);
                            Some(x0_restore)
                        };
                    }
                }
            }
        }

        let src_is_reg = if let Location::Reg(_) = local.location() { true } else { false };
        let dst_is_reg = if let Location::Reg(_) = location { true } else { false };

        if !src_is_reg && !dst_is_reg {
            require_free_reg!();
        }
        
        // ensure destination is available
        match location {
            Location::Reg(Reg::X(n)) => {
                if let Some(other) = self.regs[n as usize].upgrade() {
                    self.move_to_stack(other);
                    if n < 19 {
                        assert!(n == self.free_regs.pop().unwrap());
                    } else {
                        assert!(n == self.free_callee_save.pop().unwrap());
                    }
                }
            },
            Location::Memory(FP, offset) => {
                if let Some(other) = self.stack[self.stack_idx(offset)].upgrade() {
                    require_free_reg!();
                    self.move_to_reg(other.clone(), &[]);
                    assert!(offset == self.free_stack.pop().unwrap());
                    self.move_to_stack(other);
                }
            },
            _ => {
                unreachable!();
            },
        }

        self.move_data(Size::S64, local.location(), location);
        self.release_location(local.clone());
        local.replace_location(location);
        match location {
            Location::Reg(Reg::X(n)) => {
                self.regs[n as usize] = local.downgrade();
            },
            Location::Memory(FP, offset) => {
                let idx = self.stack_idx(offset);
                self.stack[idx] = local.downgrade();
            },
            _ => {
                unreachable!();
            },
        }

        // put the local from x0 back into x0 (if we stole it)
        if let Some(x0_restore) = x0_restore {
            self.move_data(Size::S64, x0_restore.location(), Location::Reg(X0));
            self.regs[0] = x0_restore.downgrade();
        }

        local
    }

    fn do_add_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, true, 12);
        do_bin_op_i32!(self, add, src1, src2, dst, u32);
        dst
    }

    fn do_add_p(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, true, 12);
        do_bin_op_i64!(self, add, src1, src2, dst, u32);
        dst
    }

    fn do_sub_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 12);
        do_bin_op_i32!(self, sub, src1, src2, dst, u32);
        dst
    }

    fn do_mul_i32(&mut self, src1: Local<Location>, src2: Local<Location>/*, l: DynamicLabel*/) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op_no_imm(src1, src2);
        do_bin_op_i32_no_imm!(self, mul, src1, src2, dst);
        // dynasm!(self.assembler ; mov x0, x5 ; b =>l);
        dst
    }

    fn do_and_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op_no_imm(src1, src2);
        do_bin_op_i32_no_imm!(self, and, src1, src2, dst);
        dst
    }

    fn do_le_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 12);
        do_cmp_op_i32!(self, ls, src1, src2, dst);
        dst
    }

    fn do_ge_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 12);
        do_cmp_op_i32!(self, cs, src1, src2, dst);
        dst
    }

    fn do_eqz_i32(&mut self, src: Local<Location>) -> Local<Location> {
        let (src, dst) = self.prep_unary_op(src);
        let xzr = Local::new(Location::Reg(Reg::XZR));
        do_cmp_op_i32!(self, eq, src, xzr, dst);
        dst
    }

    fn do_br_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel) {
        let r = self.move_to_reg(cond, &[]);
        dynasm!(self.assembler ; cmp X(r), xzr ; b.ne =>label);
    }

    fn do_br_location(&mut self, loc: Local<Location>) {
        let r = self.move_to_reg(loc, &[]);
        dynasm!(self.assembler ; br X(r));
    }

    fn do_br_label(&mut self, label: DynamicLabel) {
        dynasm!(self.assembler ; b =>label);
    }

    fn do_load_label(&mut self, label: DynamicLabel) -> Local<Location> {
        let r = self.get_free_reg(&[]);
        dynasm!(self.assembler ; adr X(r), =>label);
        self.new_local_from_reg(r)
    }

    fn do_emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self.assembler ; =>label);
    }

    fn do_load_from_vmctx(&mut self, sz: Size, offset: u32) -> Local<Location> {
        let reg = self.get_free_reg(&[]);
        dynasm!(self.assembler ; .arch aarch64 ; ldr X(reg), [x28, offset]);
        self.new_local_from_reg(reg)
    }

    fn do_deref(&mut self, sz: Size, loc: Local<Location>) -> Local<Location> {
        assert!(if let Location::Reg(_) = loc.location() { true } else { false });
        
        let src = self.move_to_reg(loc.clone(), &[]);
        let (dst, dst_local) = if loc.ref_ct() < 1 {
            (src, loc.clone())
        } else {
            let r = self.get_free_reg(&[src]);
            (r, self.new_local_from_reg(r))
        };
        
        match sz {
            Size::S32 => {
                dynasm!(self.assembler ; .arch aarch64 ; ldr W(dst), [X(src)]);
            },
            Size::S64 => {
                dynasm!(self.assembler ; .arch aarch64 ; ldr X(dst), [X(src)]);
            },
            _ => {
                unreachable!();
            },
        }
        
        dst_local
    }

    fn do_deref_write(&mut self, sz: Size, ptr: Local<Location>, val: Local<Location>) {
        let ptr_reg = self.move_to_reg(ptr.clone(), &[]);
        let val_reg = self.move_to_reg(val.clone(), &[ptr_reg]);
        dynasm!(self.assembler ; .arch aarch64 ; str X(val_reg), [X(ptr_reg)]);
    }

    fn do_ptr_offset(&mut self, ptr: Local<Location>, offset: i32) -> Local<Location> {
        let r = self.move_to_reg(ptr, &[]);
        Local::new(Location::Memory(Reg::X(r), offset))
    }

    fn do_vmctx_ptr_offset(&mut self, offset: i32) -> Local<Location> {
        Local::new(Location::Memory(VMCTX, offset))
    }

    fn do_call(&mut self, reloc_target: RelocationTarget,
        params: &[Local<Location>], return_types: &[WpType]) -> CallInfo<Location> {

        let reg_arg_ct = min(params.len(), 7);
        let stack_arg_ct  = params.len().saturating_sub(7);
        let stack_offset = stack_arg_ct * 8 + if stack_arg_ct % 2 == 1 { 8 } else { 0 };

        // we save one extra because vmctx is passed as the first arg
        for n in 0..=reg_arg_ct {
            if let Some(local) = self.regs[n].upgrade() {
                self.move_to_stack(local);
            }
        }

        if stack_offset > 0 {
            dynasm!(self.assembler ; sub sp, sp, stack_offset as u32);
        }

        for (n, local) in params.iter().enumerate() {
            match n {
                0..=6 => {
                    let reg = n as u32 + 1;
                    self.move_data(Size::S64, local.location(), Location::Reg(Reg::X(reg)));
                },
                _ => {
                    self.move_data(Size::S64, local.location(), Location::Memory(Reg::SP, (n as i32 - 7) * 8));
                }
            }
        }

        let fn_addr = self.new_label();
        let fn_addr_reg = 18;
        if let Some(fn_addr_reg) = self.regs[18].upgrade() {
            self.move_to_stack(fn_addr_reg);
        }

        dynasm!(self.assembler
            ; adr X(fn_addr_reg), =>fn_addr
            ; ldr X(fn_addr_reg), [X(fn_addr_reg)]
        );
        
        let before_call = self.assembler.offset().0;
        dynasm!(self.assembler ; blr X(fn_addr_reg));
        let after_call = self.assembler.offset().0;
        
        if stack_offset > 0 {
            dynasm!(self.assembler ; add sp, sp, stack_offset as u32);
        }
        
        let mut returns: SmallVec<[Local<Location>; 1]> = smallvec![];

        if !return_types.is_empty() {
            assert!(return_types.len() == 1);
            
            match return_types[0] {
                WpType::I32|WpType::I64 => {
                    returns.push(Local::new(Location::Reg(Reg::X(0))));
                },
                _ => {
                    unimplemented!();
                },
            }
        }

        self.relocation_info.push((reloc_target, fn_addr));

        CallInfo::<Location> { returns, before_call, after_call }
    }

    fn do_return(&mut self, ty: Option<WpType>, ret_val: Option<Local<Location>>, end_label: DynamicLabel) {
        match ty {
            Some(WpType::F32|WpType::F64) => { unimplemented!(); }
            _ => {}
        }
        
        if let Some(ret_val) = ret_val {
            match ret_val.location() {
                Location::Reg(X0) => {}
                loc => {
                    self.move_data(Size::S64, loc, Location::Reg(X0));
                }
            }
        }

        dynasm!(self.assembler ; .arch aarch64 ; b =>end_label);
    }
    
    fn finalize(self) -> Vec<u8> {
        // Generate actual code for special labels.
        // self.assembler
        //     .emit_label(self.special_labels.integer_division_by_zero);
        // self.mark_address_with_trap_code(TrapCode::IntegerDivisionByZero);
        // self.assembler.emit_ud2();

        // self.assembler
        //     .emit_label(self.special_labels.heap_access_oob);
        // self.mark_address_with_trap_code(TrapCode::HeapAccessOutOfBounds);
        // self.assembler.emit_ud2();

        // self.assembler
        //     .emit_label(self.special_labels.table_access_oob);
        // self.mark_address_with_trap_code(TrapCode::TableAccessOutOfBounds);
        // self.assembler.emit_ud2();

        // self.assembler
        //     .emit_label(self.special_labels.indirect_call_null);
        // self.mark_address_with_trap_code(TrapCode::IndirectCallToNull);
        // self.assembler.emit_ud2();

        // self.assembler.emit_label(self.special_labels.bad_signature);
        // self.mark_address_with_trap_code(TrapCode::BadSignature);
        // self.assembler.emit_ud2();

        // Notify the assembler backend to generate necessary code at end of function.
        // dynasm!(
        //     self
        //     ; .arch x64
        //     ; const_neg_one_32:
        //     ; .dword -1
        //     ; const_zero_32:
        //     ; .dword 0
        //     ; const_pos_one_32:
        //     ; .dword 1
        // );
        
        self.assembler.finalize().unwrap().to_vec()
    }

    fn gen_std_trampoline(sig: &FunctionType) -> Vec<u8> {
        let mut m = Self::new();
        // let mut a = Assembler::new().unwrap();

        let mut stack_offset: i32 = (3 + sig.params().len().saturating_sub(8)) as i32 * 8;
        if stack_offset % 16 != 0 {
            stack_offset += 8;
            assert!(stack_offset % 16 == 0);
        }

        let x19_save = Location::Memory(SP, stack_offset     );
        let x20_save = Location::Memory(SP, stack_offset -  8);
        let x30_save = Location::Memory(SP, stack_offset - 16);

        dynasm!(m.assembler ; .arch aarch64 ; sub sp, sp, stack_offset as u32);
        m.move_data(Size::S64, Location::Reg(X19), x19_save);
        m.move_data(Size::S64, Location::Reg(X20), x20_save);
        m.move_data(Size::S64, Location::Reg(X30), x30_save);
        
        let fptr_loc = Location::Reg(X19);
        let args_reg = X20;
        let args_loc = Location::Reg(args_reg);

        m.move_data(Size::S64, Location::Reg(X1), fptr_loc);
        m.move_data(Size::S64, Location::Reg(X2), args_loc);

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        for (i, _param) in sig.params().iter().enumerate() {
            let src = Location::Memory(args_reg, (i * 16) as _); // args_rets[i]
            
            let dst = match i {
                0..=6 => Location::Reg(Reg::X(1 + i as u32)),
                _ =>     Location::Memory(SP, (i as i32 - 7) * 8),
            };

            m.move_data(Size::S64, src, dst);
        }

        dynasm!(m.assembler ; .arch aarch64 ; blr x19);

        // Write return value.
        if !sig.results().is_empty() {
            m.move_data(
                Size::S64,
                Location::Reg(X0),
                Location::Memory(args_reg, 0),
            );
        }

        m.move_data(Size::S64, x19_save, Location::Reg(X19));
        m.move_data(Size::S64, x20_save, Location::Reg(X20));
        m.move_data(Size::S64, x30_save, Location::Reg(X30));

        // Restore stack.
        dynasm!(m.assembler
            ; .arch aarch64
            ; add sp, sp, stack_offset as u32
            ; ret);
        
        m.assembler.finalize().unwrap().to_vec()
    }

    fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }
    fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }
}