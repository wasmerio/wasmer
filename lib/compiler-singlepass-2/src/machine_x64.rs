use crate::machine::*;
use crate::codegen::{Local};
use crate::common_decl::{Size};

use dynasmrt::x64::Assembler;
use dynasmrt::{dynasm, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmer_types::{FunctionType, FunctionIndex, Type};
use wasmer_vm::VMOffsets;

use wasmer_compiler::wasmparser::Type as WpType;

use std::fmt::Debug;

use wasmer_compiler::{Relocation, RelocationTarget, RelocationKind};

use crate::machine_utils::{
    LocalManager,
    Emitter,
    In2Out1 as AbstractIn2Out1,
    In2Out0 as AbstractIn2Out0,
    In1Out1 as AbstractIn1Out1,
    In1Out0 as AbstractIn1Out0,
    In0Out1 as AbstractIn0Out1,
    Reg as AbstractReg,
    Location as AbstractLocation,
    Descriptor as AbstractDescriptor,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Reg {
    RAX = 0,
    RCX = 1,
    RDX = 2,
    RBX = 3,
    RSP = 4,
    RBP = 5,
    RSI = 6,
    RDI = 7,
    R8  = 8,
    R9  = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

const FREE_REGS: [Reg; 2] = [
    Reg::RAX, /*Reg::RBX,*/ Reg::R11,
    // Reg::R12, Reg::R13, Reg::R14, Reg::R15
];

const ARG_REGS: [Reg; 6] = [
    Reg::RDI, Reg::RSI, Reg::RDX, Reg::RCX, Reg::R8, Reg::R9
];

impl AbstractReg for Reg {
    fn is_callee_save(self) -> bool {
        const IS_CALLEE_SAVE: [bool; 16] = [
            false,false,false,true,true,true,false,false,false,false,false,false,true,true,true,true
        ];
        IS_CALLEE_SAVE[self as usize]
    }
    fn is_reserved(self) -> bool {
        self == Reg::RSP || self == Reg::RBP || self == Reg::R10 || self == Reg::R15
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<Reg, ()>
    {
        const REGS: [Reg; 16] = [
            Reg::RAX, Reg::RCX, Reg::RDX, Reg::RBX,
            Reg::RSP, Reg::RBP, Reg::RSI, Reg::RDI,
            Reg::R8,  Reg::R9,  Reg::R10, Reg::R11,
            Reg::R12, Reg::R13, Reg::R14, Reg::R15
        ];
        match n {
            0..=15 => {
                Ok(REGS[n])
            },
            _ => {
                Err(())
            }
        }
    }
}

type Location = AbstractLocation<Reg>;
type In2Out1<'a> = AbstractIn2Out1<'a, Reg, Assembler>;
type In2Out0<'a> = AbstractIn2Out0<'a, Reg, Assembler>;
type In1Out1<'a> = AbstractIn1Out1<'a, Reg, Assembler>;
type In1Out0<'a> = AbstractIn1Out0<'a, Reg, Assembler>;
type In0Out1<'a> = AbstractIn0Out1<'a, Reg, Assembler>;

impl Emitter<Reg> for Assembler {
    fn grow_stack(&mut self, n: usize) -> usize {
        dynasm!(self ; .arch x64 ; sub rsp, n as i32 * 8);
        return n;
    }
    fn shrink_stack(&mut self, offset: u32) {
        dynasm!(self ; .arch x64 ; add rsp, offset as i32);
    }
    fn move_imm32_to_mem(&mut self, sz: Size, val: u32, base: Reg, offset: i32) {
        let base = base.into_index() as u8;
        match sz {
            Size::S32 => dynasm!(self ; .arch x64 ; mov DWORD [Rq(base) + offset], val as i32),
            Size::S64 => dynasm!(self ; .arch x64 ; mov QWORD [Rq(base) + offset], val as i32),
            _ => unimplemented!(),
        }
    }
    fn move_imm32_to_reg(&mut self, _sz: Size, val: u32, reg: Reg) {
        let reg = reg.into_index() as u8;
        dynasm!(self ; .arch x64 ; mov Rd(reg), val as i32);
    }
    fn move_reg_to_reg(&mut self, sz: Size, reg1: Reg, reg2: Reg) {
        let reg1 = reg1.into_index() as u8;
        let reg2 = reg2.into_index() as u8;
        match sz {
            Size::S32 => dynasm!(self ; .arch x64 ; mov Rd(reg2), Rd(reg1)),
            Size::S64 => dynasm!(self ; .arch x64 ; mov Rq(reg2), Rq(reg1)),
            _ => unimplemented!(),
        }
    }
    fn move_reg_to_mem(&mut self, sz: Size, reg: Reg, base: Reg, offset: i32) {
        let reg = reg.into_index() as u8;
        let base = base.into_index() as u8;
        match sz {
            Size::S32 => dynasm!(self ; .arch x64 ; mov DWORD [Rq(base) + offset], Rd(reg)),
            Size::S64 => dynasm!(self ; .arch x64 ; mov QWORD [Rq(base) + offset], Rq(reg)),
            _ => unimplemented!(),
        }
    }
    fn move_mem_to_reg(&mut self, sz: Size, base: Reg, offset: i32, reg: Reg) {
        let reg = reg.into_index() as u8;
        let base = base.into_index() as u8;
        match sz {
            Size::S32 => dynasm!(self ; .arch x64 ; mov Rd(reg), DWORD [Rq(base) + offset]),
            Size::S64 => dynasm!(self ; .arch x64 ; mov Rq(reg), QWORD [Rq(base) + offset]),
            _ => unimplemented!(),
        }
    }
    fn move_mem_to_mem(&mut self, sz: Size, base1: Reg, offset1: i32, base2: Reg, offset2: i32) {
        self.move_mem_to_reg(sz, base1, offset1, Reg::R10);
        self.move_reg_to_mem(sz, Reg::R10, base2, offset2);
    }
}

struct Descriptor();
impl AbstractDescriptor<Reg> for Descriptor {
    const FP: Reg = Reg::RBP;
    const VMCTX: Reg = Reg::R15;
    const REG_COUNT: usize = 15;
    const WORD_SIZE: usize = 8;
    const STACK_GROWS_DOWN: bool = true;
    const FP_STACK_ARG_OFFSET: i32 = 24;
    const ARG_REG_COUNT: usize = 6;
    fn callee_save_regs() -> Vec<Reg> {
        vec![
            Reg::RBX, Reg::RSP, Reg::RBP, Reg::R12, Reg::R13,
            Reg::R14, Reg::R15,
        ]
    }
    fn caller_save_regs() -> Vec<Reg> {
        vec![
            Reg::RAX, Reg::RCX, Reg::RDX, Reg::RSI, Reg::RDI,
            Reg::R8, Reg::R9, Reg::R10, Reg::R11,
        ]
    }
    fn callee_param_location(n: usize) -> Location {
        match n {
            0..=5 => Location::Reg(ARG_REGS[n]),
            _ => Location::Memory(Reg::RBP, 24 + (n-6) as i32 * 8),
        }
    }
    fn caller_arg_location(n: usize) -> Location {
        match n {
            0..=5 => Location::Reg(ARG_REGS[n]),
            _ => Location::Memory(Reg::RSP, (n-5) as i32  * 8),
        }
    }
    fn return_location() -> Location {
        Location::Reg(Reg::RAX)
    }
}

pub struct X64Machine {
    assembler: Assembler,
    relocation_info: Vec<(RelocationTarget, usize)>,
    manager: LocalManager<Reg, Assembler, Descriptor>,
}

impl X64Machine {
    fn new() -> Self {
        Self {
            assembler: Assembler::new().unwrap(),
            manager: LocalManager::new(),
            relocation_info: vec![],
        }
    }
}

impl Machine for X64Machine {
    type Location = Location;
    type Label = DynamicLabel;
    const BR_INSTR_SIZE: usize = 4;

    fn new() -> Self {
        X64Machine::new()
    }
    
    fn get_assembly_offset(&mut self) -> usize {
        self.assembler.offset().0
    }

    fn new_label(&mut self) -> DynamicLabel {
        self.assembler.new_dynamic_label()
    }

    fn do_const_i32(&mut self, n: i32) -> Local<Location> {
        Local::new(Location::Imm32(n as u32), Size::S32)
    }

    // N.B. `n_locals` includes `n_params`; `n_locals` will always be >= `n_params`
    fn func_begin(&mut self, n_locals: usize, n_params: usize) -> Vec<Local<Location>> {
        // save FP and VMCTX regs
        dynasm!(&mut self.assembler
            ; .arch x64
            ; push rbp
            ; push r15
            ; mov rbp, rsp
            ; mov r15, rdi);
        
        let mut free_regs = vec![Reg::RDI];
        for r in (n_params + 1)..=5 {
            free_regs.push(ARG_REGS[r]);
        }
        free_regs.extend(&FREE_REGS);

        self.manager.init_locals(n_params, n_locals, &free_regs)
    }

    fn func_end(&mut self, end_label: DynamicLabel) -> Vec<Relocation> {
        dynasm!(self.assembler ; .arch x64 ; =>end_label);
        
        // restore SP
        self.manager.restore_stack_offset(&mut self.assembler, 0);
        
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
    }

    fn block_begin(&mut self) {
        self.manager.block_begin();
    }

    fn block_end(&mut self, end_label: DynamicLabel) {
        self.manager.block_end(&mut self.assembler);
        dynasm!(self.assembler ; .arch x64 ; =>end_label);
    }

    fn do_normalize_local(&mut self, local: Local<Location>) -> Local<Location> {
        self.manager.normalize_local(&mut self.assembler, local)
    }

    fn do_restore_local(&mut self, local: Local<Location>, location: Location) -> Local<Location> {
        self.manager.restore_local(&mut self.assembler, local, location)
    }

    fn do_add_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .commutative(true)
        .max_imm_width(32)
        .reg_imm(|e, dst, src| {
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; add Rd(dst), src as i32);
        })
        .reg_reg(|e, dst, src| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; add Rd(dst), Rd(src));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_add_p(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .commutative(true)
        .size(Size::S64)
        .max_imm_width(32)
        .reg_imm(|e, dst, src| {
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; add Rq(dst), src as i32);
        })
        .reg_reg(|e, dst, src| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; add Rq(dst), Rq(src));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_sub_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(32)
        .reg_imm(|e, dst, src| {
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; sub Rd(dst), src as i32);
        })
        .reg_reg(|e, dst, src| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; sub Rd(dst), Rd(src));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_mul_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .commutative(true)
        .reg_exact_reg_with_clobber(Reg::RAX, &[Reg::RDX], |e, src| {
            let src = src.into_index() as u8;
            dynasm!(e ; .arch x64 ; mul Rd(src));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_and_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(32)
        .reg_imm(|e, dst, src| {
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; and Rd(dst), src as i32);
        })
        .reg_reg(|e, dst, src| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; and Rd(dst), Rd(src));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_le_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(32)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), src2 as i32 ; setbe Rb(dst) ; and Rq(dst), 0xff);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let src2 = src2.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), Rd(src2) ; setbe Rb(dst) ; and Rq(dst), 0xff);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_lt_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(32)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), src2 as i32; setb Rb(dst) ; and Rq(dst), 0xff);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let src2 = src2.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), Rd(src2); setb Rb(dst) ; and Rq(dst), 0xff);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_ge_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(32)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), src2 as i32 ; setae Rb(dst) ; and Rq(dst), 0xff);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u8;
            let src2 = src2.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src1), Rd(src2) ; setae Rb(dst) ; and Rq(dst), 0xff);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_eqz_i32(&mut self, src: Local<Location>) -> Local<Location> {
        In1Out1::new()
        .size(Size::S32)
        .reg_reg(|e, src, dst| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rd(src), 0 ; sete Rb(dst) ; and Rq(dst), 0xff);
        })
        .execute(&mut self.manager, &mut self.assembler, src)
    }

    fn do_br_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rq(src), 0 ; jne =>label);
        })
        .execute(&mut self.manager, &mut self.assembler, cond);
    }

    fn do_br_not_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u8;
            dynasm!(e ; .arch x64 ; cmp Rq(src), 0 ; je =>label);
        })
        .execute(&mut self.manager, &mut self.assembler, cond);
    }

    fn do_br_location(&mut self, loc: Local<Location>, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u8;
            dynasm!(e ; .arch x64 ; jmp Rq(src));
        })
        .execute(&mut self.manager, &mut self.assembler, loc);
    }

    fn do_br_label(&mut self, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        dynasm!(self.assembler ; .arch x64 ; jmp =>label);
    }

    fn do_load_label(&mut self, label: DynamicLabel) -> Local<Location> {
        In0Out1::new()
        .size(Size::S64)
        .reg(|e, dst| {
            let dst = dst.into_index() as u8;
            dynasm!(e ; .arch x64 ; lea Rq(dst), [=>label]);
        })
        .execute(&mut self.manager, &mut self.assembler)
    }

    fn do_emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self.assembler ; .arch x64 ; =>label);
    }

    fn do_load_from_vmctx(&mut self, sz: Size, offset: u32) -> Local<Location> {
        In0Out1::new()
        .size(sz)
        .reg(|e, reg| {
            let reg = reg.into_index() as u8;
            match sz {
                Size::S64 => { dynasm!(e ; .arch x64 ; mov Rq(reg), [r15 + offset as i32]); },
                _ => { unimplemented!(); },
            }
        })
        .execute(&mut self.manager, &mut self.assembler)
    }

    fn do_deref(&mut self, sz: Size, loc: Local<Location>) -> Local<Location> {
        assert!(if let Location::Reg(_) = loc.location() { true } else { false });
        In1Out1::new()
        .size(sz)
        .reg_reg(|e, src, dst| {
            let src = src.into_index() as u8;
            let dst = dst.into_index() as u8;
            match sz {
                Size::S32 => { dynasm!(e ; .arch x64 ; mov Rd(dst), [Rq(src)]); },
                Size::S64 => { dynasm!(e ; .arch x64 ; mov Rq(dst), [Rq(src)]); },
                _ => { unimplemented!(); },
            }
        })
        .execute(&mut self.manager, &mut self.assembler, loc)
    }

    fn do_deref_write(&mut self, sz: Size, ptr: Local<Location>, val: Local<Location>) {
        In2Out0::new()
        .size(sz)
        .reg_reg(|e, ptr, val| {
            let ptr = ptr.into_index() as u8;
            let val = val.into_index() as u8;
            
            match sz {
                Size::S32 => { dynasm!(e ; .arch x64 ; mov DWORD [Rq(ptr)], Rd(val)); },
                _ => { unimplemented!(); },
            }
        })
        .execute(&mut self.manager, &mut self.assembler, ptr, val);
    }

    fn do_ptr_offset(&mut self, sz: Size, ptr: Local<Location>, offset: i32) -> Local<Location> {
        In1Out0::new().reg(|_, _|{}).execute(&mut self.manager, &mut self.assembler, ptr.clone());
        let reg = if let Location::Reg(reg) = ptr.location() {reg} else {unreachable!()};
        Local::new(Location::Memory(reg, offset), sz)
    }

    fn do_vmctx_ptr_offset(&mut self, sz: Size, offset: i32) -> Local<Location> {
        Local::new(Location::Memory(Descriptor::VMCTX, offset), sz)
    }

    fn do_call(&mut self, reloc_target: RelocationTarget,
        args: &[Local<Location>], return_types: &[WpType]) -> CallInfo<Location> {
        
        const X64_MOV64_IMM_OFFSET: usize = 2;
        
        // here we align the stack to 8 bytes because call pushes the return address, 
        // which will align the stack to 16 bytes at the beginning of the frame
        // this is not required by all x86 platforms, but some conventions require it
        // (e.g. darwin)
        let stack_offset = self.manager.get_stack_offset();
        let even_stack_args = (args.len() + 1) > 6 && (args.len() + 1) % 2 == 0;
        let did_align_stack = if stack_offset % 16 == 0 && even_stack_args {
            dynasm!(self.assembler ; .arch x64 ; sub rsp, 8);
            true
        } else {
            false
        };

        self.manager.before_call(&mut self.assembler, args);

        let fn_addr_offset = self.assembler.offset().0 + X64_MOV64_IMM_OFFSET;
        dynasm!(self.assembler ; .arch x64 ; mov rax, 0x7f_ff_ff_ff_ff_ff_ff_ff);
        
        let before_call = self.assembler.offset().0;
        dynasm!(self.assembler ; .arch x64 ; call rax);
        let after_call = self.assembler.offset().0;
        
        let returns = self.manager.after_call(&mut self.assembler, return_types);
        
        if did_align_stack {
            dynasm!(self.assembler ; .arch x64 ; add rsp, 8);
        }
        
        self.relocation_info.push((reloc_target, fn_addr_offset));

        CallInfo::<Location> { returns, before_call, after_call }
    }

    fn do_return(&mut self, ty: Option<WpType>, ret_val: Option<Local<Location>>, end_label: DynamicLabel) {
        match ty {
            Some(WpType::F32) => { unimplemented!(); }
            Some(WpType::F64) => { unimplemented!(); }
            _ => {}
        }
        
        if let Some(ret_val) = ret_val {
            self.manager.set_return_values(&mut self.assembler, &[ret_val]);
        }

        dynasm!(self.assembler ; .arch x64 ; jmp =>end_label);
    }

    fn release_location(&mut self, local: Local<Location>) {
        self.manager.release_location(local);
    }
    
    fn finalize(self) -> Vec<u8> {
        self.assembler.finalize().unwrap().to_vec()
    }
    
    fn gen_std_trampoline(sig: &FunctionType) -> Vec<u8> {
        let mut m = Self::new();

        let fptr = Reg::R12;
        let args = Reg::R13;
        dynasm!(m.assembler
            ; .arch x64
            ; push Rq(fptr.into_index() as u8)
            ; push Rq(args.into_index() as u8)
            ; mov Rq(fptr.into_index() as u8), rsi
            ; mov Rq(args.into_index() as u8), rdx);

        let stack_args = sig.params().len().saturating_sub(6);
        let mut stack_offset = 0;
        if stack_args > 0 {
            stack_offset = stack_args as i32 * 8;
        }
        // here we align the stack to 8 bytes because call pushes the return address, 
        // which will align the stack to 16 bytes at the beginning of the frame
        // this is not required by all x86 platforms, but some conventions require it
        // (e.g. darwin)
        if stack_offset % 16 == 0 {
            stack_offset += 8;
        }
        
        dynasm!(m.assembler ; .arch x64 ; sub rsp, stack_offset);

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        for (i, param) in sig.params().iter().enumerate() {
            let sz = match *param {
                Type::I32 => Size::S32,
                Type::I64 => Size::S64,
                _ => unimplemented!(),
            };
            match i {
                0..=4 => {
                    m.assembler.move_mem_to_reg(sz, args, (i * 16) as i32, ARG_REGS[i + 1]);
                },
                _ => {
                    m.assembler.move_mem_to_mem(sz, args, (i * 16) as i32, Reg::RSP, (i as i32 - 5) * 8);
                },
            }
        }

        dynasm!(m.assembler ; .arch x64 ; call Rq(fptr.into_index() as u8));

        // Write return value.
        if !sig.results().is_empty() {
            m.assembler.move_reg_to_mem(Size::S64, Reg::RAX, args, 0);
        }

        // Restore stack.
        dynasm!(m.assembler
            ; .arch x64
            ; add rsp, stack_offset
            ; pop Rq(args.into_index() as u8)
            ; pop Rq(fptr.into_index() as u8)
            ; ret);
        
        m.assembler.finalize().unwrap().to_vec()
    }

    fn gen_std_dynamic_import_trampoline(
        _vmoffsets: &VMOffsets,
        _sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch x64 ; ret);
        a.finalize().unwrap().to_vec()
    }
    
    fn gen_import_call_trampoline(
        _vmoffsets: &VMOffsets,
        _index: FunctionIndex,
        _sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch x64 ; ret);
        a.finalize().unwrap().to_vec()
    }
}
