use crate::machine::*;
use crate::codegen::{Local};
use crate::common_decl::{Size};

use dynasmrt::aarch64::Assembler;
use dynasmrt::{dynasm, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmer_types::{FunctionType, FunctionIndex, Type};
use wasmer_vm::VMOffsets;

use wasmer_compiler::wasmparser::Type as WpType;

use std::cmp::min;
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
    X0  = 0,  X1  = 1,  X2  = 2,  X3  = 3,  X4  = 4,
    X5  = 5,  X6  = 6,  X7  = 7,  X8  = 8,  X9  = 9,
    X10 = 10, X11 = 11, X12 = 12, X13 = 13, X14 = 14,
    X15 = 15, X16 = 16, X17 = 17, X18 = 18, X19 = 19,
    X20 = 20, X21 = 21, X22 = 22, X23 = 23, X24 = 24,
    X25 = 25, X26 = 26, X27 = 27, X28 = 28, X29 = 29,
    X30 = 30, XZR = 31, SP  = 32,
}

const FREE_REGS: [Reg; 10] = [
    Reg::X8,  Reg::X9,  Reg::X10, Reg::X11, Reg::X12,
    Reg::X13, Reg::X14, Reg::X15, Reg::X16, Reg::X17,
    // Reg::X19, Reg::X20, Reg::X21, Reg::X22, Reg::X23,
    // Reg::X24, Reg::X25, Reg::X26, Reg::X27,
];

impl AbstractReg for Reg {
    fn is_callee_save(self) -> bool {
        self as usize > 18
    }
    fn is_reserved(self) -> bool {
        match self.into_index() {
            0..=17|19..=27 => false,
            _ => true,
        }
    }
    fn into_index(self) -> usize {
        min(self as usize, 31) // xzr and sp have the same index; hence this slightly hacky workaround
    }
    fn from_index(n: usize) -> Result<Reg, ()>
    {
        const REGS: [Reg; 33] = [
            Reg::X0, Reg::X1, Reg::X2, Reg::X3, Reg::X4, Reg::X5, Reg::X6,
            Reg::X7, Reg::X8, Reg::X9, Reg::X10,Reg::X11,Reg::X12,Reg::X13,
            Reg::X14,Reg::X15,Reg::X16,Reg::X17,Reg::X18,Reg::X19,Reg::X20,
            Reg::X21,Reg::X22,Reg::X23,Reg::X24,Reg::X25,Reg::X26,Reg::X27,
            Reg::X28,Reg::X29,Reg::X30,Reg::XZR,Reg::SP];
        match n {
            0..=32 => {
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
        let n = if n < 2 { 2 } else { n };
        dynasm!(self ; .arch aarch64 ; sub sp, sp, n as u32 * 8);
        return n;
    }
    fn shrink_stack(&mut self, offset: u32) {
        dynasm!(self ; .arch aarch64 ; add sp, sp, offset);
    }
    fn move_imm32_to_mem(&mut self, sz: Size, val: u32, base: Reg, offset: i32) {
        self.move_imm32_to_reg(sz, val, Reg::X18);
        self.move_reg_to_mem(sz, Reg::X18, base, offset);
    }
    fn move_imm32_to_reg(&mut self, _sz: Size, val: u32, reg: Reg) {
        let reg = reg.into_index() as u32;
        if val & 0xffff0000 != 0 {
            dynasm!(self ; .arch aarch64 ; mov W(reg), (val & 0xffff) as u64 ; movk W(reg), val >> 16, LSL 16);
        } else {
            dynasm!(self ; .arch aarch64 ; mov W(reg), val as u64);
        }
    }
    fn move_reg_to_reg(&mut self, sz: Size, reg1: Reg, reg2: Reg) {
        let reg1 = reg1.into_index() as u32;
        let reg2 = reg2.into_index() as u32;
        match sz {
            Size::S32 => dynasm!(self ; .arch aarch64 ; mov W(reg2), W(reg1)),
            Size::S64 => dynasm!(self ; .arch aarch64 ; mov X(reg2), X(reg1)),
            _ => unimplemented!(),
        }
    }
    fn move_reg_to_mem(&mut self, sz: Size, reg: Reg, base: Reg, offset: i32) {
        let reg = reg.into_index() as u32;
        let base = base.into_index() as u32;
        match sz {
            Size::S32 => dynasm!(self ; .arch aarch64 ; stur W(reg), [XSP(base), offset]),
            Size::S64 => dynasm!(self ; .arch aarch64 ; stur X(reg), [XSP(base), offset]),
            _ => unimplemented!(),
        }
    }
    fn move_mem_to_reg(&mut self, sz: Size, base: Reg, offset: i32, reg: Reg) {
        let base = base.into_index() as u32;
        let reg = reg.into_index() as u32;
        match sz {
            Size::S32 => dynasm!(self ; .arch aarch64 ; ldur W(reg), [XSP(base), offset]),
            Size::S64 => dynasm!(self ; .arch aarch64 ; ldur X(reg), [XSP(base), offset]),
            _ => unimplemented!(),
        }
    }
    fn move_mem_to_mem(&mut self, sz: Size, base1: Reg, offset1: i32, base2: Reg, offset2: i32) {
        self.move_mem_to_reg(sz, base1, offset1, Reg::X18);
        self.move_reg_to_mem(sz, Reg::X18, base2, offset2);
    }
}

struct Descriptor();
impl AbstractDescriptor<Reg> for Descriptor {
    const FP: Reg = Reg::X29;
    const VMCTX: Reg = Reg::X28;
    const REG_COUNT: usize = 28;
    const WORD_SIZE: usize = 8;
    const STACK_GROWS_DOWN: bool = true;
    const FP_STACK_ARG_OFFSET: i32 = 32;
    const ARG_REG_COUNT: usize = 8;
    fn callee_save_regs() -> Vec<Reg> {
        vec![
            Reg::X19, Reg::X20, Reg::X21, Reg::X22, Reg::X23,
            Reg::X24, Reg::X25, Reg::X26, Reg::X27, Reg::X28,
            Reg::X29, Reg::X30
        ]
    }
    fn caller_save_regs() -> Vec<Reg> {
        vec![
            Reg::X0,  Reg::X1,  Reg::X2,  Reg::X3,  Reg::X4,
            Reg::X5,  Reg::X6,  Reg::X7,  Reg::X8,  Reg::X9,
            Reg::X10, Reg::X11, Reg::X12, Reg::X13, Reg::X14,
            Reg::X15, Reg::X16, Reg::X17, Reg::X18
        ]
    }
    fn callee_param_location(n: usize) -> Location {
        match n {
            0..=7 => Location::Reg(Reg::from_index(n).unwrap()),
            _ => Location::Memory(Self::FP, 32 + (n-8) as i32 * 8),
        }
    }
    fn caller_arg_location(n: usize) -> Location {
        match n {
            0..=7 => Location::Reg(Reg::from_index(n).unwrap()),
            _ => Location::Memory(Reg::SP, (n-7) as i32  * 8),
        }
    }
    fn return_location() -> Location {
        Location::Reg(Reg::X0)
    }
}

pub struct Aarch64Machine {
    assembler: Assembler,
    relocation_info: Vec<(RelocationTarget, DynamicLabel)>,
    manager: LocalManager<Reg, Assembler, Descriptor>,
}

impl Aarch64Machine {
    fn new() -> Self {
        Self {
            assembler: Assembler::new().unwrap(),
            manager: LocalManager::new(),
            relocation_info: vec![],
        }
    }
}

impl Machine for Aarch64Machine {
    type Location = Location;
    type Label = DynamicLabel;
    const BR_INSTR_SIZE: usize = 4;

    fn new() -> Self {
        Aarch64Machine::new()
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
        // save LR, FP, and VMCTX regs
        dynasm!(&mut self.assembler
            ; .arch aarch64
            ; sub sp, sp, 32
            ; stp x29, x30, [sp]
            ; str x28, [sp, 16]
            ; mov x29, sp
            ; mov x28, x0);
        
        let mut free_regs = vec![Reg::X0];
        for r in (n_params + 1)..=7 {
            free_regs.push(Reg::from_index(r).unwrap());
        }
        free_regs.extend(&FREE_REGS);

        self.manager.init_locals(n_params, n_locals, &free_regs)
    }

    fn func_end(&mut self, end_label: DynamicLabel) -> Vec<Relocation> {
        dynasm!(self.assembler ; .arch aarch64 ; =>end_label);
        
        // restore SP
        self.manager.restore_stack_offset(&mut self.assembler, 0);
        
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
            dynasm!(self.assembler ; .arch aarch64 ; =>fn_addr_label ; nop ; nop);
            relocations.push(Relocation {
                kind: RelocationKind::Abs8,
                reloc_target,
                offset: reloc_at,
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
        dynasm!(self.assembler ; .arch aarch64 ; =>end_label);
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
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; add W(dst), W(src1), src2);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; add W(dst), W(src1), W(src2));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_add_p(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .commutative(true)
        .size(Size::S64)
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; add X(dst), X(src1), src2);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; add X(dst), X(src1), X(src2));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_sub_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; sub W(dst), W(src1), src2);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; sub W(dst), W(src1), W(src2));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_mul_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .commutative(true)
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; mul W(dst), W(src1), W(src2));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_and_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; .arch aarch64 ; and W(dst), W(src1), W(src2));
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_le_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), src2 ; cset X(dst), ls);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), W(src2) ; cset X(dst), ls);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_lt_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), src2 ; cset X(dst), lo);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), W(src2) ; cset X(dst), lo);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_ge_u_i32(&mut self, src1: Local<Location>, src2: Local<Location>) -> Local<Location> {
        In2Out1::new()
        .max_imm_width(12)
        .reg_imm_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), src2 ; cset X(dst), hs);
        })
        .reg_reg_reg(|e, src1, src2, dst| {
            let src1 = src1.into_index() as u32;
            let src2 = src2.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src1), W(src2) ; cset X(dst), hs);
        })
        .execute(&mut self.manager, &mut self.assembler, src1, src2)
    }

    fn do_eqz_i32(&mut self, src: Local<Location>) -> Local<Location> {
        In1Out1::new()
        .reg_reg(|e, src, dst| {
            let src = src.into_index() as u32;
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp W(src), wzr ; cset X(dst), eq);
        })
        .execute(&mut self.manager, &mut self.assembler, src)
    }

    fn do_br_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp X(src), xzr ; b.ne =>label);
        })
        .execute(&mut self.manager, &mut self.assembler, cond);
    }

    fn do_br_not_cond_label(&mut self, cond: Local<Location>, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; cmp X(src), xzr ; b.eq =>label);
        })
        .execute(&mut self.manager, &mut self.assembler, cond);
    }

    fn do_br_location(&mut self, loc: Local<Location>, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        In1Out0::new()
        .reg(|e, src| {
            let src = src.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; br X(src));
        })
        .execute(&mut self.manager, &mut self.assembler, loc);
    }

    fn do_br_label(&mut self, label: DynamicLabel, depth: u32) {
        self.manager.br_depth(&mut self.assembler, depth);
        dynasm!(self.assembler ; .arch aarch64 ; b =>label);
    }

    fn do_load_label(&mut self, label: DynamicLabel) -> Local<Location> {
        In0Out1::new()
        .reg(|e, dst| {
            let dst = dst.into_index() as u32;
            dynasm!(e ; .arch aarch64 ; adr X(dst), =>label);
        })
        .execute(&mut self.manager, &mut self.assembler)
    }

    fn do_emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self.assembler ; .arch aarch64 ; =>label);
    }

    fn do_load_from_vmctx(&mut self, sz: Size, offset: u32) -> Local<Location> {
        In0Out1::new()
        .reg(|e, reg| {
            let reg = reg.into_index() as u32;
            match sz {
                Size::S64 => { dynasm!(e ; .arch aarch64 ; ldr X(reg), [x28, offset]); },
                _ => { unimplemented!(); },
            }
        })
        .execute(&mut self.manager, &mut self.assembler)
    }

    fn do_deref(&mut self, sz: Size, loc: Local<Location>) -> Local<Location> {
        assert!(if let Location::Reg(_) = loc.location() { true } else { false });
        In1Out1::new()
        .reg_reg(|e, src, dst| {
            let src = src.into_index() as u32;
            let dst = dst.into_index() as u32;
            match sz {
                Size::S32 => { dynasm!(e ; .arch aarch64 ; ldr W(dst), [X(src)]); },
                Size::S64 => { dynasm!(e ; .arch aarch64 ; ldr X(dst), [X(src)]); },
                _ => { unimplemented!(); },
            }
        })
        .execute(&mut self.manager, &mut self.assembler, loc)
    }

    fn do_deref_write(&mut self, sz: Size, ptr: Local<Location>, val: Local<Location>) {
        In2Out0::new()
        .reg_reg(|e, ptr, val| {
            let ptr = ptr.into_index() as u32;
            let val = val.into_index() as u32;
            
            match sz {
                Size::S32 => { dynasm!(e ; .arch aarch64 ; str W(val), [X(ptr)]); },
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

        self.manager.before_call(&mut self.assembler, args);

        let fn_addr = self.new_label();
        dynasm!(self.assembler
            ; .arch aarch64
            ; adr x18, =>fn_addr
            ; ldr x18, [x18]
        );
        let before_call = self.assembler.offset().0;
        dynasm!(self.assembler ; .arch aarch64 ; blr x18);
        let after_call = self.assembler.offset().0;
        
        let returns = self.manager.after_call(&mut self.assembler, return_types);

        self.relocation_info.push((reloc_target, fn_addr));

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

        dynasm!(self.assembler ; .arch aarch64 ; b =>end_label);
    }

    fn release_location(&mut self, local: Local<Location>) {
        self.manager.release_location(local);
    }
    
    fn finalize(self) -> Vec<u8> {
        self.assembler.finalize().unwrap().to_vec()
    }
    
    fn gen_std_trampoline(sig: &FunctionType) -> Vec<u8> {
        let mut m = Self::new();

        let fptr = Reg::X19;
        let args = Reg::X20;

        dynasm!(m.assembler
            ; .arch aarch64
            ; sub sp, sp, 32
            ; stp x29, x30, [sp]
            ; stp X(fptr as u32), X(args as u32), [sp, 16]
            ; mov x29, sp
            ; mov X(fptr as u32), x1
            ; mov X(args as u32), x2
        );

        let stack_args = sig.params().len().saturating_sub(8);
        let mut stack_offset = stack_args as u32 * 8;
        if stack_args > 0 {
            if stack_offset % 16 != 0 {
                stack_offset += 8;
                assert!(stack_offset % 16 == 0);
            }
            dynasm!(m.assembler ; .arch aarch64 ; sub sp, sp, stack_offset);
        }

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        for (i, param) in sig.params().iter().enumerate() {
            let sz = match *param {
                Type::I32 => Size::S32,
                Type::I64 => Size::S64,
                _ => unimplemented!(),
            };
            match i {
                0..=6 => {
                    m.assembler.move_mem_to_reg(sz, args, (i * 16) as i32, Reg::from_index(i + 1).unwrap());
                },
                _ => {
                    m.assembler.move_mem_to_mem(sz, args, (i * 16) as i32, Reg::SP, (i as i32 - 7) * 8);
                },
            }
        }

        dynasm!(m.assembler ; .arch aarch64 ; blr X(fptr as u32));

        // Write return value.
        if !sig.results().is_empty() {
            m.assembler.move_reg_to_mem(Size::S64, Reg::X0, args, 0);
        }

        // Restore stack.
        dynasm!(m.assembler
            ; .arch aarch64
            ; ldp X(fptr as u32), X(args as u32), [x29, 16]
            ; ldp x29, x30, [x29]
            ; add sp, sp, 32 + stack_offset as u32
            ; ret
        );
        
        m.assembler.finalize().unwrap().to_vec()
    }

    fn gen_std_dynamic_import_trampoline(
        _vmoffsets: &VMOffsets,
        _sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }
    
    fn gen_import_call_trampoline(
        _vmoffsets: &VMOffsets,
        _index: FunctionIndex,
        _sig: &FunctionType) -> Vec<u8> {
        let mut a = Assembler::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }
}
