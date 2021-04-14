use crate::emitter::{Emitter, Size};
use crate::machine_aarch64::{Aarch64Machine as Machine, Location, Reg};
use crate::common_decl::*;
use crate::codegen::CodegenError;

use wasmer_types::{FunctionType, FunctionIndex};
use wasmer_vm::VMOffsets;

use dynasmrt::{dynasm, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};

pub use dynasmrt::aarch64::Assembler;

impl Emitter for Assembler {
    // type Label = DynamicLabel;
    type Offset = AssemblyOffset;
    type Location = Location;

    // fn new() -> Self {
    //     Assembler::new().unwrap()
    // }
    
    fn emit_prologue(&mut self) {}

    fn get_offset(&self) -> AssemblyOffset {
        self.offset()
    }

    fn gen_std_trampoline(
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Self::new().unwrap();





        
        // Calculate stack offset.
        let mut stack_offset: u32 = 0;
        for (i, _param) in sig.params().iter().enumerate() {
            if let Location::Memory(_, _) = Machine::get_param_location(1 + i) {
                unimplemented!();
                stack_offset += 8;
            }
        }

        // Align to 16 bytes. We push two 8-byte registers below, so here we need to ensure stack_offset % 16 == 8.
        // if stack_offset % 16 != 8 {
        //     stack_offset += 8;
        // }
        
        dynasm!(a
            ; sub sp, sp, #32
            ; str x19, [sp, #8]
            ; str x20, [sp, #16]
            ; str x30, [sp, #24]
            ; mov x19, x1
            ; mov x20, x2);
        // // Used callee-saved registers
        // a.emit_push(Size::S64, Location::GPR(GPR::R15));
        // a.emit_push(Size::S64, Location::GPR(GPR::R14));

        // // Prepare stack space.
        // a.emit_sub(
        //     Size::S64,
        //     Location::Imm32(stack_offset),
        //     Location::GPR(GPR::RSP),
        // );

        // // Arguments
        // a.emit_move(
        //     Size::S64,
        //     Machine::get_param_location(1),
        //     Location::GPR(GPR::R15),
        // ); // func_ptr
        // a.emit_mov(
        //     Size::S64,
        //     Machine::get_param_location(2),
        //     Location::GPR(GPR::R14),
        // ); // args_rets

        // // Move arguments to their locations.
        // // `callee_vmctx` is already in the first argument register, so no need to move.
        {
        //     let mut n_stack_args: usize = 0;
            for (i, _param) in sig.params().iter().enumerate() {
                let src = Location::Memory(Reg::X(20), (i * 16) as _); // args_rets[i]
                let dst = Machine::get_param_location(1 + i);
                a.emit_move(Size::S64, src, dst);
                // match dst_loc {
                //     Location::GPR(_) => {
                //         a.emit_mov(Size::S64, src_loc, dst_loc);
                //     }
                //     Location::Memory(_, _) => {
                //         // This location is for reading arguments but we are writing arguments here.
                //         // So recalculate it.
                //         a.emit_mov(Size::S64, src_loc, Location::GPR(GPR::RAX));
                //         a.emit_mov(
                //             Size::S64,
                //             Location::GPR(GPR::RAX),
                //             Location::Memory(GPR::RSP, (n_stack_args * 8) as _),
                //         );
                //         n_stack_args += 1;
                //     }
                //     _ => unreachable!(),
                // }
            }
        }

        // // Call.
        // a.emit_call_location(Location::GPR(GPR::R15));
        dynasm!(a ; blr x19);

        // // Restore stack.
        // a.emit_add(
        //     Size::S64,
        //     Location::Imm32(stack_offset),
        //     Location::GPR(GPR::RSP),
        // );

        // Write return value.
        if !sig.results().is_empty() {
            // a.emit_move(
            //     Size::S64,
            //     M::get_return_location(),
            //     Location::Memory(GPR::R14, 0),
            // );
            dynasm!(a ; str x0, [x20]);
        }

        // // Restore callee-saved registers.
        // a.emit_pop(Size::S64, Location::GPR(GPR::R14));
        // a.emit_pop(Size::S64, Location::GPR(GPR::R15));

        // a.emit_ret();

        dynasm!(a
            ; ldr x19, [sp, #8]
            ; ldr x20, [sp, #16]
            ; ldr x30, [sp, #24]
            ; add sp, sp, #32
            ; ret);
        a.finalize().unwrap().to_vec()
    }
    fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Self::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }
    fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType) -> Vec<u8> {
        let mut a = Self::new().unwrap();
        dynasm!(a ; .arch aarch64 ; ret);
        a.finalize().unwrap().to_vec()
    }

    fn emit_label(&mut self, label: DynamicLabel) {
        dynasm!(self ; .arch aarch64 ; => label);
    }

    fn new_label(&mut self) -> DynamicLabel {
        self.new_dynamic_label()
    }

    fn emit_return(&mut self) {
        dynasm!(self ; .arch aarch64 ; ret);
    }

    fn emit_move(&mut self, sz: Size, src: Location, dst: Location) {
        if let (Location::Reg(Reg::X(src)), Location::Reg(Reg::X(dst))) = (src, dst) {
            dynasm!(self ; .arch aarch64 ; mov X(dst), X(src));
        } else if let (Location::Imm32(src), Location::Reg(Reg::X(dst))) = (src, dst) {
            dynasm!(self ; .arch aarch64 ; mov X(dst), src as u64);
        } else if let (Location::Memory(Reg::X(reg), idx), Location::Reg(Reg::X(dst))) = (src, dst) {
            /*if idx > 0 {
            } else*/ {
                dynasm!(self ; .arch aarch64 ; ldur X(dst), [X(reg), idx]);
            }
        } else {
            unimplemented!();
        }
    }

    fn emit_add(&mut self, sz: Size, src: Self::Location, dst: Self::Location) {
        if let (Location::Reg(Reg::X(src)), Location::Reg(Reg::X(dst))) = (src, dst) {
            dynasm!(self ; .arch aarch64 ; add X(dst), X(dst), X(src));
        // } else if let (Location::Imm32(src), Location::Reg(Reg::X(dst))) = (src, dst) {
        //     dynasm!(self ; .arch aarch64 ; mov X(dst), src as u64);
        // } else if let (Location::Memory(Reg::X(reg), idx), Location::Reg(Reg::X(dst))) = (src, dst) {
        //     /*if idx > 0 {
        //     } else*/ {
        //         dynasm!(self ; .arch aarch64 ; ldur X(dst), [X(reg), idx]);
        //     }
        } else {
            unimplemented!();
        }
    }
    
    fn finalize(mut self) -> Vec<u8> {
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
        
        (self as Assembler).finalize().unwrap().to_vec()
    }
}