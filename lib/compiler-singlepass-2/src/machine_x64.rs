use crate::common_decl::*;
use crate::machine::*;
use crate::codegen::{Local, WeakLocal};

use dynasmrt::x64::Assembler;
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

// pub const X0:  Reg = Reg::X(0);
// pub const X1:  Reg = Reg::X(1);
// pub const X2:  Reg = Reg::X(2);
// // pub const X3:  Reg = Reg::X(3);
// // pub const X4:  Reg = Reg::X(4);
// // pub const X5:  Reg = Reg::X(5);
// // pub const X6:  Reg = Reg::X(6);
// // pub const X7:  Reg = Reg::X(7);
// // pub const X8:  Reg = Reg::X(8);
// // pub const X9:  Reg = Reg::X(9);
// // pub const X10: Reg = Reg::X(10);
// // pub const X11: Reg = Reg::X(11);
// // pub const X12: Reg = Reg::X(12);
// // pub const X13: Reg = Reg::X(13);
// // pub const X14: Reg = Reg::X(14);
// // pub const X15: Reg = Reg::X(15);
// // pub const X16: Reg = Reg::X(16);
// // pub const X17: Reg = Reg::X(17);
// // pub const X18: Reg = Reg::X(18);
// pub const X19: Reg = Reg::X(19);
// pub const X20: Reg = Reg::X(20);
// // pub const X21: Reg = Reg::X(21);
// // pub const X22: Reg = Reg::X(22);
// // pub const X23: Reg = Reg::X(23);
// // pub const X24: Reg = Reg::X(24);
// // pub const X25: Reg = Reg::X(25);
// // pub const X26: Reg = Reg::X(26);
// // pub const X27: Reg = Reg::X(27);
// // pub const X28: Reg = Reg::X(28);
// // pub const X29: Reg = Reg::X(29);
// pub const X30: Reg = Reg::X(30);
// // pub const XZR: Reg = Reg::XZR;
pub const SP_GPR: u8 = 4;
pub const FP_GPR: u8 = 5;
pub const VMCTX_GPR: u8 = 15;

const IS_CALLEE_SAVE: [bool; 16] = [false,true,false,false,true,true,false,false,false,false,false,false,true,true,true,true];
const ARG_REGS: [u8; 6] = [7,6,3,2,8,9];
const X64_MOV64_IMM_OFFSET: usize = 2;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Location {
    Imm32(u32),
    GPR(u8),
    XMM(u8),
    Memory(u8, i32),
}

impl MaybeImmediate for Location {
    fn imm_value(&self) -> Option<Value> {
        match *self {
            Location::Imm32(imm) => Some(Value::I32(imm as i32)),
            _ => None
        }
    }
}

pub struct X64Machine {
    assembler: Assembler,
    n_stack_params: usize,
    regs: [WeakLocal<Location>; 28], // x28, x29, and x30 are off limits!
    stack: Vec<WeakLocal<Location>>,
    reg_counter: u8,
    free_regs: Vec<u8>,
    free_callee_save: Vec<u8>,
    free_stack: Vec<i32>,
    stack_offset: i32,
    relocation_info: Vec<(RelocationTarget, usize)>,

    saved_stack_offsets: Vec<i32>,
    saved_free_regs: Vec<Vec<u8>>,
    saved_free_callee_save: Vec<Vec<u8>>,
    saved_free_stack: Vec<Vec<i32>>,

    state: MachineState,
}

macro_rules! do_bin_op_i32 {
    ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident, $imm_ty:ty) => {
        if $src1.location() != $dst.location() {
            $self.move_data(Size::S32, $src1.location(), $dst.location());
        }

        match ($dst.location(), $src2.location()) {
            (Location::GPR(src1_reg), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rd(src1_reg), Rd(src2_reg));
            },
            (Location::GPR(src1_reg), Location::Imm32(src2_val)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rd(src1_reg), src2_val as $imm_ty);
            },
            (Location::GPR(src1_reg), Location::Memory(src2_reg, src2_offset)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rd(src1_reg), DWORD [Rq(src2_reg) + src2_offset]);
            },
            (Location::Memory(src1_reg, src1_offset), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; $op DWORD [Rq(src1_reg) + src1_offset], Rd(src2_reg));
            },
            _ => {  
                unreachable!()
            },
        }
    }
}

macro_rules! do_bin_op_i64 {
    ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident, $imm_ty:ty) => {
        if $src1.location() != $dst.location() {
            $self.move_data(Size::S64, $src1.location(), $dst.location());
        }

        match ($dst.location(), $src2.location()) {
            (Location::GPR(src1_reg), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rq(src1_reg), Rq(src2_reg));
            },
            (Location::GPR(src1_reg), Location::Imm32(src2_val)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rq(src1_reg), src2_val as $imm_ty);
            },
            (Location::GPR(src1_reg), Location::Memory(src2_reg, src2_offset)) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rq(src1_reg), QWORD [Rq(src2_reg) + src2_offset]);
            },
            (Location::Memory(src1_reg, src1_offset), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; $op QWORD [Rq(src1_reg) + src1_offset], Rq(src2_reg));
            },
            _ => {  
                unreachable!()
            },
        }
    }
}

// macro_rules! do_bin_op_i32_no_imm {
//     ($self:ident, $op:ident, $src1:ident, $src2:ident, $dst:ident) => {
//         let (src1_reg, dst_reg) = 
//             if let (Location::Reg(Reg::X(src1_reg)), Location::Reg(Reg::X(dst_reg))) = 
//                 ($src1.location(), $dst.location()) {
//                 (src1_reg, dst_reg)
//             } else {
//                 unreachable!();
//             };

//         match $src2.location() {
//             Location::Reg(Reg::X(src2_reg)) => {
//                 dynasm!($self.assembler ; .arch x64 ; $op W(dst_reg), W(src1_reg), W(src2_reg));
//             },
//             _ => {  
//                 unreachable!()
//             },
//         }
//     }
// }

macro_rules! do_cmp_op_i32 {
    ($self:ident, $op:tt, $src1:ident, $src2:ident, $dst:ident, $imm_ty:ty) => {
        match ($src1.location(), $src2.location()) {
            (Location::GPR(src1_reg), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; cmp Rd(src1_reg), Rd(src2_reg));
            },
            (Location::GPR(src1_reg), Location::Imm32(src2_val)) => {
                dynasm!($self.assembler ; .arch x64 ; cmp Rd(src1_reg), src2_val as $imm_ty);
            },
            (Location::GPR(src1_reg), Location::Memory(src2_reg, src2_offset)) => {
                dynasm!($self.assembler ; .arch x64 ; cmp Rd(src1_reg), DWORD [Rq(src2_reg) + src2_offset]);
            },
            (Location::Memory(src1_reg, src1_offset), Location::GPR(src2_reg)) => {
                dynasm!($self.assembler ; .arch x64 ; cmp DWORD [Rq(src1_reg) + src1_offset], Rd(src2_reg));
            },
            _ => {  
                unreachable!()
            },
        }

        match $dst.location() {
            Location::GPR(dst_reg) => {
                dynasm!($self.assembler ; .arch x64 ; $op Rb(dst_reg));
            },
            Location::Memory(dst_reg, dst_offset) => {
                dynasm!($self.assembler ; .arch x64 ; $op BYTE [Rq(dst_reg) + dst_offset]);
            },
            _ => {
                unreachable!()
            }
        }
    }
}

impl X64Machine {
    pub fn new() -> Self {
        Self {
            assembler: Assembler::new().unwrap(),
            regs: array_init(|_| WeakLocal::new()),
            stack: Vec::new(),
            reg_counter: 0,
            n_stack_params: 0,
            free_regs: vec![0,2,3,10,11],
            free_callee_save: vec![/**/],
            free_stack: vec![],
            stack_offset: 0,
            relocation_info: vec![],

            saved_stack_offsets: vec![],
            saved_free_regs: vec![],
            saved_free_callee_save: vec![],
            saved_free_stack: vec![],

            state: Self::new_state(),
        }
    }

    fn emit_restore_stack_offset(&mut self, prev_offset: i32) -> bool {
        let diff = -(self.stack_offset - prev_offset);

        if diff > 0 {
            dynasm!(self.assembler ; .arch x64 ; add rsp, diff);
            true
        } else {
            false
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

    // fn prep_bin_op_no_imm(&mut self,
    //     src1: Local<Location>, src2: Local<Location>) ->
    //     (Local<Location>, Local<Location>, Local<Location>) {
    //     let r1 = self.move_to_reg(src1.clone(), &[]);
    //     let r2 = self.move_to_reg(src2.clone(), &[r1]);

    //     if src1.ref_ct() < 1 {
    //         (src1.clone(), src2, src1)
    //     } else if src2.ref_ct() < 1 {
    //         (src1, src2.clone(), src2)
    //     } else {
    //         let dst = self.get_free_reg(&[r1, r2]);
    //         (src1, src2, self.new_local_from_reg(dst))
    //     }
    // }

    fn prep_bin_op(&mut self,
        src1: Local<Location>, src2: Local<Location>, commutative: bool, imm_bits: u8) ->
        (Local<Location>, Local<Location>, Local<Location>) {
        
        let (src1, src2) = match (src1.location(), src2.location(), commutative) {
            (Location::Imm32(_), Location::Imm32(_), _) => {
                self.move_to_reg(src1.clone(), &[]);
                (src1, src2)
            },
            // (_, Location::Imm32(_), _) => {
            //     self.move_to_reg(src1.clone(), &[]);
            //     (src1, src2)
            // },
            (Location::Imm32(_), _, true) => {
                self.move_to_reg(src2.clone(), &[]);
                (src2, src1)
            },
            (Location::Memory(_, _), Location::Memory(r2, _), _) => {
                self.move_to_reg(src1.clone(), &[r2]);
                (src1, src2)
            }
            _ => {
                // let r1 = self.move_to_reg(src1.clone(), &[]);
                // self.move_to_reg(src2.clone(), &[r1]);
                (src1, src2)
            }
        };
        
        let src2_is_imm = if let Location::Imm32(_) = src2.location() { true } else { false };

        // if let Some(Value::I32(val)) = src2.location().imm_value() {
        //     let mask: u32 = 0xffffffff << imm_bits;
            
        //     if mask & val as u32 != 0 {
        //         // value is too large to be represented as an immediate value
        //         let r1 = if let Location::Reg(Reg::X(r1)) = src1.location() {r1} else {unreachable!()};
        //         self.move_to_reg(src2.clone(), &[r1]);
        //     }
        // }

        if src1.ref_ct() < 1 {
            (src1.clone(), src2, src1)
        } else if !src2_is_imm && src2.ref_ct() < 1 {
            (src1, src2.clone(), src2)
        } else {
            let mut dont_use: SmallVec<[_; 2]> = SmallVec::new();
            if let Location::GPR(r1) = src1.location() {
                dont_use.push(r1)
            }
            if let Location::GPR(r2) = src2.location() {
                dont_use.push(r2)
            }
            let dst = self.get_free_reg(&dont_use);
            (src1, src2, self.new_local_from_reg(dst))
        }
    }

    fn get_param_location(i: usize) -> Location {
        match i {
            0..=5 => Location::GPR(ARG_REGS[i]),
            _ => Location::Memory(FP_GPR, 32 + (i-8) as i32 * 8),
        }
    }

    // assumes the returned stack index will be used
    fn get_free_stack(&mut self) -> i32 {
        if let Some(idx) = self.free_stack.pop() {
            return idx;
        }

        self.stack_offset -= 8;
        self.stack.push(WeakLocal::new());

        dynasm!(self.assembler ; .arch x64 ; sub rsp, 8);
        
        self.stack_offset
    }

    // loc.target must be a numbered register
    fn move_to_stack(&mut self, loc: Local<Location>) {
        let stack = self.get_free_stack();
        
        match loc.location() {
            Location::GPR(reg) => {
                dynasm!(self.assembler ; .arch x64 ; mov [rbp + stack], Rq(reg));
            }
            _ => {
                unreachable!();
            }
        }

        self.release_location(loc.clone());
        loc.replace_location(Location::Memory(FP_GPR, stack));
        let idx = self.stack_idx(stack);
        self.stack[idx] = loc.downgrade();
    }

    // assumes the returned reg will be eventually released with Machine::release_location()
    fn get_free_reg(&mut self, dont_use: &[u8]) -> u8 {
        if let Some(reg) = self.free_callee_save.pop() {
            reg
        } else if let Some(reg) = self.free_regs.pop() {
            reg
        } else {
            // better not put all the regs in here or this loop will deadlock!!!
            while dont_use.contains(&self.reg_counter) {
                self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u8;
            }
            
            let reg = self.reg_counter;
            self.reg_counter = (self.reg_counter + 1) % self.regs.len() as u8;
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

    fn new_local_from_reg(&mut self, reg: u8) -> Local<Location> {
        let local = Local::new(Location::GPR(reg));
        self.regs[reg as usize] = local.downgrade();
        local
    }
    
    fn move_to_reg(&mut self, loc: Local<Location>, dont_use: &[u8]) -> u8 {
        if let Location::GPR(reg) = loc.location() {
            return reg;
        }
        
        let reg = self.get_free_reg(dont_use);
        match loc.location() {
            Location::Imm32(n) => {
                // if n & 0xffff0000 != 0 {
                //     dynasm!(self.assembler; .arch x64; mov X(reg), (n & 0xffff) as u64);
                //     dynasm!(self.assembler; .arch x64; movk X(reg), n >> 16, LSL 16);
                // } else {
                    dynasm!(self.assembler; .arch x64; mov Rq(reg), n as i32);
                // }
            },
            Location::Memory(base_reg, n) => {
                dynasm!(self.assembler; .arch x64; mov Rq(reg), [Rq(base_reg) + n]);
                if base_reg == FP_GPR {
                    self.free_stack.push(n);
                }
            },
            _ => {
                unreachable!();
            },
        }

        self.regs[reg as usize] = loc.downgrade();
        self.release_location(loc.clone());
        loc.replace_location(Location::GPR(reg));

        reg
    }

    fn move_data(&mut self, sz: Size, src: Location, dst: Location) {
        match (src, dst) {
            // reg -> reg
            (Location::GPR(src), Location::GPR(dst)) => {
                dynasm!(self.assembler ; .arch x64 ; mov Rq(dst), Rq(src));
            },
            // imm -> reg
            (Location::Imm32(src), Location::GPR(dst)) => {
                dynasm!(self.assembler ; .arch x64 ; mov Rq(dst), src as i32);
            },
            // mem -> reg
            (Location::Memory(reg, idx), Location::GPR(dst)) => {
                dynasm!(self.assembler ; .arch x64 ; mov Rq(dst), [Rq(reg) + idx]);
            },
            // reg -> mem
            (Location::GPR(src), Location::Memory(reg, idx)) => {
                dynasm!(self.assembler ; .arch x64 ; mov [Rq(reg) + idx], Rq(src));
            },
            // imm -> mem
            (Location::Imm32(src), Location::Memory(reg, idx)) => {
                dynasm!(self.assembler ; .arch x64 ; mov DWORD [Rq(reg) + idx], src as i32);
            },
            // mem -> mem
            (Location::Memory(src, src_idx), Location::Memory(dst, dst_idx)) => {
                let tmp = self.get_free_reg(&[src, dst]);
                dynasm!(self.assembler
                    ; .arch x64
                    ; mov Rq(tmp), [Rq(src) + src_idx]
                    ; mov [Rq(dst) + dst_idx], Rq(tmp));
                self.release_location(Local::new(Location::GPR(tmp)));
            },
            _ => {
                unreachable!();
            },
        }
    }
}

impl Machine for X64Machine {
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
            Location::GPR(n) => {
                self.regs[n as usize] = WeakLocal::new();
                if IS_CALLEE_SAVE[n as usize] {
                    self.free_callee_save.push(n);
                } else {
                    self.free_regs.push(n);
                }
            },
            Location::Memory(FP_GPR, offset) => {
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
        // save FP and VMCTX regs
        // call pushes the return address, aligning the stack to 8 bytes
        // we need to align to 8 again in order to achieve 16 byte alignment
        // this is not required by all x86 platforms, but some conventions require is (e.g. darwin)
        dynasm!(&mut self.assembler
            ; .arch x64
            ; sub rsp, 8
            ; push rbp
            ; push r15
            ; mov rbp, rsp
            ; mov r15, rsi);
        
            let mut locals: Vec<Local<Location>> = Vec::with_capacity(n_locals);
        for i in 0..n_params {
            let loc = Local::new(Self::get_param_location(i + 1));
            if let Location::GPR(n) = loc.location() {
                self.regs[n as usize] = loc.downgrade()
            }
            locals.push(loc);
        }
        for _ in n_params..n_locals {
            locals.push(Local::new(Location::Imm32(0)));
        }
        self.free_regs.push(0);
        for r in (n_params + 1)..=5 {
            self.free_regs.push(ARG_REGS[r]);
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
        dynasm!(self.assembler ; .arch x64 ; =>end_label);

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
            dynasm!(self.assembler ; .arch x64 ; add rsp, -self.stack_offset);
        }
        
        // restore FP and VMCTX regs
        dynasm!(self.assembler
            ; .arch x64
            ; pop r15
            ; pop rbp
            ; ret);
        
        // reserve space for relocated function pointers
        let mut relocations = vec![];
        for (reloc_target, fn_addr_offset) in self.relocation_info.iter().copied() {
            relocations.push(Relocation {
                kind: RelocationKind::Abs8,
                reloc_target,
                offset: fn_addr_offset as u32,
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
        self.saved_stack_offsets.push(self.stack_offset);
        self.saved_free_regs.push(self.free_regs.clone());
        self.saved_free_callee_save.push(self.free_callee_save.clone());
        self.saved_free_stack.push(self.free_stack.clone());
    }

    fn block_end(&mut self, end_label: DynamicLabel) {
        self.free_regs = self.saved_free_regs.pop().unwrap();
        self.free_callee_save = self.saved_free_callee_save.pop().unwrap();
        self.free_stack = self.saved_free_stack.pop().unwrap();

        let prev_offset = self.saved_stack_offsets.pop().unwrap();
        if self.emit_restore_stack_offset(prev_offset) {
            self.stack_offset = prev_offset;
            self.stack.truncate(self.n_stack_params + (self.stack_offset / -8) as usize);
        }
        dynasm!(self.assembler ; .arch x64 ; =>end_label);
    }

    fn do_normalize_local(&mut self, local: Local<Location>) -> Local<Location> {
        if let Location::Imm32(n) = local.location() {
            let new_local = Local::new(Location::Imm32(n));
            self.move_to_reg(new_local.clone(), &[]);
            new_local
        } else if local.ref_ct() > 1 {
            let reg = self.get_free_reg(&[]);
            self.move_data(Size::S64, local.location(), Location::GPR(reg));
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

        let src_is_reg = if let Location::GPR(_) = local.location() { true } else { false };
        let dst_is_reg = if let Location::GPR(_) = location { true } else { false };

        if !src_is_reg && !dst_is_reg {
            require_free_reg!();
        }
        
        // ensure destination is available
        match location {
            Location::GPR(n) => {
                if let Some(other) = self.regs[n as usize].upgrade() {
                    self.move_to_stack(other);
                    if n < 19 {
                        assert!(n == self.free_regs.pop().unwrap());
                    } else {
                        assert!(n == self.free_callee_save.pop().unwrap());
                    }
                }
            },
            Location::Memory(FP_GPR, offset) => {
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
            Location::GPR(n) => {
                self.regs[n as usize] = local.downgrade();
            },
            Location::Memory(FP_GPR, offset) => {
                let idx = self.stack_idx(offset);
                self.stack[idx] = local.downgrade();
            },
            _ => {
                unreachable!();
            },
        }

        // put the local from x0 back into x0 (if we stole it)
        if let Some(x0_restore) = x0_restore {
            self.move_data(Size::S64, x0_restore.location(), Location::GPR(0));
            self.regs[0] = x0_restore.downgrade();
        }

        local
    }

    fn do_add_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, true, 32);
        do_bin_op_i32!(self, add, src1, src2, dst, i32);
        dst
    }

    fn do_add_p(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, true, 32);
        do_bin_op_i64!(self, add, src1, src2, dst, i32);
        dst
    }

    fn do_sub_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 12);
        do_bin_op_i32!(self, sub, src1, src2, dst, i32);
        dst
    }

    fn do_mul_i32(&mut self, src1: Local<Location>, src2: Local<Location>/*, l: DynamicLabel*/) -> Local<Location> {
        if let Some(rdx) = self.regs[3].upgrade() {
            self.move_to_stack(rdx);
        }
        let (rax, operand) = if let Location::GPR(0) = src1.location() {
            if src1.ref_ct() > 1 {
                self.move_to_stack(src1);
                (self.new_local_from_reg(0), src2)
            } else {
                (src1, src2)
            }
        } else if let Location::GPR(0) = src2.location() {
            if src2.ref_ct() > 1 {
                self.move_to_stack(src2);
                (self.new_local_from_reg(0), src1)
            } else {
                (src2, src1)
            }
        } else {
            if let Some(rax) = self.regs[0].upgrade() {
                self.move_to_stack(rax);
            }
            self.move_data(Size::S64, src1.location(), Location::GPR(0));
            if src1.ref_ct() > 1 {
                (self.new_local_from_reg(0), src2)
            } else {

                self.regs[0] = src1.downgrade();
                self.release_location(src1.clone());
                src1.replace_location(Location::GPR(0));
                (src1, src2)
            }
        };
        if operand.location().is_imm() {
            self.move_to_reg(operand.clone(), &[0, 3]);
        }

        match operand.location() {
            Location::GPR(reg) => {
                dynasm!(self.assembler ; .arch x64 ; mul Rq(reg));
            },
            Location::Memory(reg, offset) => {
                dynasm!(self.assembler ; .arch x64 ; mul [Rq(reg) + offset]);
            },
            _ => {
                unreachable!();
            }
        }

        rax
    }

    fn do_and_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 32);
        do_bin_op_i32!(self, and, src1, src2, dst, i32);
        dst
    }

    fn do_le_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 32);
        do_cmp_op_i32!(self, setbe, src1, src2, dst, i32);
        dst
    }

    fn do_lt_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 32);
        do_cmp_op_i32!(self, setb, src1, src2, dst, i32);
        dst
    }

    fn do_ge_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        let (src1, src2, dst) = self.prep_bin_op(src1, src2, false, 32);
        do_cmp_op_i32!(self, setae, src1, src2, dst, i32);
        dst
    }

    fn do_eqz_i32(&mut self, src: Local<Location>) -> Local<Location> {
        let (src, dst) = self.prep_unary_op(src);
        let zero = Local::new(Location::Imm32(0));
        do_cmp_op_i32!(self, sete, src, zero, dst, i32);
        dst
    }

    fn do_br_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        if depth > 0 {
            let idx = self.saved_stack_offsets.len() - depth as usize;
            self.emit_restore_stack_offset(self.saved_stack_offsets[idx]);
        }
        let r = self.move_to_reg(cond, &[]);
        dynasm!(self.assembler ; .arch x64 ; cmp Rq(r), 0 ; jne =>label);
    }

    fn do_br_not_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        if depth > 0 {
            let idx = self.saved_stack_offsets.len() - depth as usize;
            self.emit_restore_stack_offset(self.saved_stack_offsets[idx]);
        }
        let r = self.move_to_reg(cond, &[]);
        dynasm!(self.assembler ; .arch x64 ; cmp Rq(r), 0 ; je =>label);
    }

    fn do_br_location(&mut self, loc: Local<Location>, depth: u32) {
        if depth > 0 {
            let idx = self.saved_stack_offsets.len() - depth as usize;
            self.emit_restore_stack_offset(self.saved_stack_offsets[idx]);
        }
        let r = self.move_to_reg(loc, &[]);
        dynasm!(self.assembler ; .arch x64 ; jmp Rq(r));
    }

    fn do_br_label(&mut self, label: DynamicLabel, depth: u32) {
        if depth > 0 {
            let idx = self.saved_stack_offsets.len() - depth as usize;
            self.emit_restore_stack_offset(self.saved_stack_offsets[idx]);
        }
        dynasm!(self.assembler ; .arch x64 ; jmp =>label);
    }

    fn do_load_label(&mut self, label: DynamicLabel) -> Local<Location> {
        let r = self.get_free_reg(&[]);
        // TODO: not sure LEA is correct here
        dynasm!(self.assembler ; .arch x64 ; lea Rq(r), [=>label]);
        self.new_local_from_reg(r)
    }

    fn do_emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self.assembler ; =>label);
    }

    fn do_load_from_vmctx(&mut self, sz: Size, offset: u32) -> Local<Location> {
        let reg = self.get_free_reg(&[]);
        dynasm!(self.assembler ; .arch x64 ; mov Rq(reg), [Rq(VMCTX_GPR) + offset as i32]);
        self.new_local_from_reg(reg)
    }

    fn do_deref(&mut self, sz: Size, loc: Local<Location>) -> Local<Location> {
        assert!(if let Location::GPR(_) = loc.location() { true } else { false });
        
        let src = self.move_to_reg(loc.clone(), &[]);
        let (dst, dst_local) = if loc.ref_ct() < 1 {
            (src, loc.clone())
        } else {
            let r = self.get_free_reg(&[src]);
            (r, self.new_local_from_reg(r))
        };
        
        match sz {
            Size::S32 => {
                dynasm!(self.assembler ; .arch x64 ; mov Rd(dst), [Rq(src)]);
            },
            Size::S64 => {
                dynasm!(self.assembler ; .arch x64 ; mov Rq(dst), [Rq(src)]);
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
        dynasm!(self.assembler ; .arch x64 ; mov [Rq(ptr_reg)], Rq(val_reg));
    }

    fn do_ptr_offset(&mut self, ptr: Local<Location>, offset: i32) -> Local<Location> {
        let r = self.move_to_reg(ptr, &[]);
        Local::new(Location::Memory(r, offset))
    }

    fn do_vmctx_ptr_offset(&mut self, offset: i32) -> Local<Location> {
        Local::new(Location::Memory(VMCTX_GPR, offset))
    }

    fn do_call(&mut self, reloc_target: RelocationTarget,
        params: &[Local<Location>], return_types: &[WpType]) -> CallInfo<Location> {

        // let reg_arg_ct = min(params.len(), 7);
        let stack_arg_ct  = params.len().saturating_sub(5);
        let stack_offset = stack_arg_ct * 8 + if stack_arg_ct % 2 == 1 { 8 } else { 0 };

        for (i, &n) in IS_CALLEE_SAVE.iter().enumerate() {
            if n { continue; }
            if let Some(local) = self.regs[i].upgrade() {
                self.move_to_stack(local);
            }
        }

        if stack_offset > 0 {
            dynasm!(self.assembler ; .arch x64 ; sub rsp, stack_offset as i32);
        }

        // vmctx is always passed as the first argument
        dynasm!(self.assembler ; .arch x64 ; mov rsi, Rq(VMCTX_GPR));

        for (n, local) in params.iter().enumerate() {
            match n {
                0..=4 => {
                    let reg = ARG_REGS[n + 1];
                    self.move_data(Size::S64, local.location(), Location::GPR(reg));
                },
                _ => {
                    self.move_data(Size::S64, local.location(), Location::Memory(SP_GPR, (n as i32 - 5) * 8));
                }
            }
        }

        // let fn_addr = self.new_label();
        // let fn_addr_reg = 18;
        // if let Some(fn_addr_reg) = self.regs[18].upgrade() {
        //     self.move_to_stack(fn_addr_reg);
        // }
        
        let fn_addr_offset = self.assembler.offset().0 + X64_MOV64_IMM_OFFSET;
        dynasm!(self.assembler ; .arch x64 ; mov rax, 0x00_00_00_00_00_00_00_00);
        
        let before_call = self.assembler.offset().0;
        dynasm!(self.assembler ; .arch x64 ; call rax);
        let after_call = self.assembler.offset().0;
        
        if stack_offset > 0 {
            dynasm!(self.assembler ; .arch x64 ; add rsp, stack_offset as i32);
        }
        
        let mut returns: SmallVec<[Local<Location>; 1]> = smallvec![];

        if !return_types.is_empty() {
            assert!(return_types.len() == 1);
            
            match return_types[0] {
                WpType::I32|WpType::I64 => {
                    returns.push(self.new_local_from_reg(0));
                },
                _ => {
                    unimplemented!();
                },
            }
        }

        self.relocation_info.push((reloc_target, fn_addr_offset));

        CallInfo::<Location> { returns, before_call, after_call }
    }

    fn do_return(&mut self, ty: Option<WpType>, ret_val: Option<Local<Location>>, end_label: DynamicLabel) {
        match ty {
            Some(WpType::F32|WpType::F64) => { unimplemented!(); }
            _ => {}
        }
        
        if let Some(ret_val) = ret_val {
            match ret_val.location() {
                Location::GPR(0) => {}
                loc => {
                    self.move_data(Size::S64, loc, Location::GPR(0));
                }
            }
        }

        dynasm!(self.assembler ; .arch x64 ; jmp =>end_label);
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

        let mut stack_offset: i32 = (3 + sig.params().len().saturating_sub(6)) as i32 * 8;
        // call pushes the return address, aligning the stack to 8 bytes
        // we need to align to 8 again in order to achieve 16 byte alignment
        // this is not required by all x86 platforms, but some conventions require is (e.g. darwin)
        if stack_offset % 16 == 0 {
            stack_offset += 8;
        }
        assert!(stack_offset % 8 == 0);

        let rbp_save = Location::Memory(SP_GPR, stack_offset     );
        let vmctx_save = Location::Memory(SP_GPR, stack_offset -  8);

        dynasm!(m.assembler ; .arch x64 ; sub rsp, stack_offset as i32);
        m.move_data(Size::S64, Location::GPR(FP_GPR), rbp_save);
        m.move_data(Size::S64, Location::GPR(VMCTX_GPR), vmctx_save);
        
        let fptr_loc = Location::GPR(0);
        let args_reg = 12;
        let args_loc = Location::GPR(args_reg);

        m.move_data(Size::S64, Location::GPR(ARG_REGS[1]), fptr_loc);
        m.move_data(Size::S64, Location::GPR(ARG_REGS[2]), args_loc);

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        for (i, _param) in sig.params().iter().enumerate() {
            let src = Location::Memory(args_reg, (i * 16) as _); // args_rets[i]
            
            let dst = match i {
                0..=4 => Location::GPR(ARG_REGS[1 + i]),
                _ =>     Location::Memory(SP_GPR, (i as i32 - 5) * 8),
            };

            m.move_data(Size::S64, src, dst);
        }

        dynasm!(m.assembler ; .arch x64 ; call r12);

        // Write return value.
        if !sig.results().is_empty() {
            m.move_data(
                Size::S64,
                Location::GPR(0),
                Location::Memory(args_reg, 0),
            );
        }

        m.move_data(Size::S64, rbp_save, Location::GPR(FP_GPR));
        m.move_data(Size::S64, vmctx_save, Location::GPR(VMCTX_GPR));

        // Restore stack.
        dynasm!(m.assembler
            ; .arch x64
            ; add rsp, stack_offset as i32
            ; ret);
        
        m.assembler.finalize().unwrap().to_vec()
    }

    fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch x64 ; ret);
        a.finalize().unwrap().to_vec()
    }
    fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch x64 ; ret);
        a.finalize().unwrap().to_vec()
    }
}