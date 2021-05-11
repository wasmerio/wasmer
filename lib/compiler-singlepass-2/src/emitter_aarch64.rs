use crate::emitter::{Emitter, Size};
use crate::machine_aarch64::{Aarch64Machine as Machine, Location, Reg, X0, X1, X2, X19, X20, X30, SP};
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
    
    fn emit_prologue(&mut self) {
        // save LR and FP
        dynasm!(self
            ; .arch aarch64
            // ; ldr x7, [x7]
            ; sub sp, sp, 16
            ; stp x29, x30, [sp]
            ; mov x29, sp);
    }

    fn get_offset(&self) -> AssemblyOffset {
        self.offset()
    }

    fn gen_std_trampoline(sig: &FunctionType) -> Vec<u8> {
        let mut a = Self::new().unwrap();

        let mut stack_offset: i32 = (3 + sig.params().len().saturating_sub(8)) as i32 * 8;
        if stack_offset % 16 != 0 {
            stack_offset += 8;
            assert!(stack_offset % 16 == 0);
        }

        let x19_save = Location::Memory(SP, stack_offset     );
        let x20_save = Location::Memory(SP, stack_offset -  8);
        let x30_save = Location::Memory(SP, stack_offset - 16);

        dynasm!(a ; .arch aarch64 ; sub sp, sp, stack_offset as u32);
        a.emit_move(Size::S64, Location::Reg(X19), x19_save);
        a.emit_move(Size::S64, Location::Reg(X20), x20_save);
        a.emit_move(Size::S64, Location::Reg(X30), x30_save);
        
        let fptr_loc = Location::Reg(X19);
        let args_reg = X20;
        let args_loc = Location::Reg(args_reg);

        a.emit_move(Size::S64, Location::Reg(X1), fptr_loc);
        a.emit_move(Size::S64, Location::Reg(X2), args_loc);

        // Move arguments to their locations.
        // `callee_vmctx` is already in the first argument register, so no need to move.
        for (i, _param) in sig.params().iter().enumerate() {
            let src = Location::Memory(args_reg, (i * 16) as _); // args_rets[i]
            
            let dst = match i {
                0..=6 => Location::Reg(Reg::X(1 + i as u32)),
                _ =>     Location::Memory(SP, (i as i32 - 7) * 8),
            };

            a.emit_move(Size::S64, src, dst);
        }

        a.emit_call_location(fptr_loc);

        // Write return value.
        if !sig.results().is_empty() {
            a.emit_move(
                Size::S64,
                Location::Reg(X0),
                Location::Memory(args_reg, 0),
            );
        }

        a.emit_move(Size::S64, x19_save, Location::Reg(X19));
        a.emit_move(Size::S64, x20_save, Location::Reg(X20));
        a.emit_move(Size::S64, x30_save, Location::Reg(X30));

        // Restore stack.
        dynasm!(a
            ; .arch aarch64
            ; add sp, sp, stack_offset as u32
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

    fn emit_return(&mut self, loc: Option<Location>) {
        if let Some(loc) = loc {
            match loc {
                Location::Reg(X0) => {}
                _ => {
                    self.emit_move(Size::S64, loc, Location::Reg(X0));
                }
            }
        }

        // restore LR and FP
        dynasm!(self
            ; .arch aarch64
            ; ldp x29, x30, [sp]
            ; add sp, sp, 16
            ; ret);
    }

    fn emit_move(&mut self, sz: Size, src: Location, dst: Location) {
        match (src, dst) {
            // reg -> reg
            (Location::Reg(src), Location::Reg(dst)) => {
                match (src, dst) {
                    (Reg::X(src), Reg::X(dst)) => {
                        dynasm!(self ; .arch aarch64 ; mov X(dst), X(src));
                    },
                    (Reg::XZR, Reg::X(dst)) => {
                        dynasm!(self ; .arch aarch64 ; mov X(dst), xzr);
                    },
                    (Reg::SP, Reg::X(dst)) => {
                        dynasm!(self ; .arch aarch64 ; mov X(dst), sp);
                    },
                    (Reg::X(src), Reg::SP) => {
                        dynasm!(self ; .arch aarch64 ; mov sp, X(src));
                    },
                    _ => unreachable!()
                }
            },
            // imm -> reg
            (Location::Imm32(src), Location::Reg(dst)) => {
                match dst {
                    Reg::X(dst) => {
                        dynasm!(self ; .arch aarch64 ; mov X(dst), src as u64);
                    },
                    _ => unreachable!()
                }
            },
            // mem -> reg
            (Location::Memory(reg, idx), Location::Reg(dst)) => {
                match (reg, dst) {
                    (Reg::X(reg), Reg::X(dst)) => {
                        dynasm!(self ; .arch aarch64 ; ldur X(dst), [X(reg), idx]);
                    },
                    (Reg::SP, Reg::X(dst)) => {
                        dynasm!(self ; .arch aarch64 ; mov X(dst), sp);
                        dynasm!(self ; .arch aarch64 ; ldur X(dst), [sp, idx]);
                    },
                    _ => unreachable!()
                }
            },
            // reg -> mem
            (Location::Reg(src), Location::Memory(reg, idx)) => {
                match (src, reg) {
                    (Reg::X(src), Reg::X(reg)) => {
                        dynasm!(self ; .arch aarch64 ; stur X(src), [X(reg), idx]);
                    },
                    (Reg::X(src), Reg::SP) => {
                        dynasm!(self ; .arch aarch64 ; stur X(src), [sp, idx]);
                    },
                    (Reg::XZR, Reg::X(reg)) => {
                        dynasm!(self ; .arch aarch64 ; stur xzr, [X(reg), idx]);
                    },
                    (Reg::XZR, Reg::SP) => {
                        dynasm!(self ; .arch aarch64 ; stur xzr, [sp, idx]);
                    },
                    _ => unreachable!()
                }
            },
            // imm -> mem
            (Location::Imm32(src), Location::Memory(reg, idx)) => {
                dynasm!(self ; .arch aarch64 ; mov x18, src as u64);
                match reg {
                    Reg::X(reg) => {
                        dynasm!(self ; .arch aarch64 ; stur x18, [X(reg), idx]);
                    },
                    Reg::SP => {
                        dynasm!(self ; .arch aarch64 ; stur x18, [sp, idx]);
                    },
                    _ => unreachable!()
                }
            },
            // mem -> mem
            (Location::Memory(src, src_idx), Location::Memory(dst, dst_idx)) => {
                match src {
                    Reg::X(src) => {
                        dynasm!(self ; .arch aarch64 ; ldur x18, [X(src), src_idx]);
                    },
                    Reg::SP => {
                        dynasm!(self ; .arch aarch64 ; ldur x18, [sp, src_idx]);
                    },
                    _ => unreachable!()
                }
                match dst {
                    Reg::X(dst) => {
                        dynasm!(self ; .arch aarch64 ; stur x18, [X(dst), dst_idx]);
                    },
                    Reg::SP => {
                        dynasm!(self ; .arch aarch64 ; stur x18, [sp, dst_idx]);
                    },
                    _ => unreachable!()
                }
            },
            _ => {
                unreachable!();
            },
        }
    }

    fn emit_add_i32(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        if let (Location::Reg(Reg::X(src1)), Location::Reg(Reg::X(src2)), Location::Reg(Reg::X(dst))) = (src1, src2, dst) {
            dynasm!(self ; .arch aarch64 ; add X(dst), X(src1), X(src2));
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

    fn emit_sub_i32(&mut self, sz: Size, src1: Location, src2: Location, dst: Location) {
        if let (Location::Reg(Reg::X(src1)), Location::Reg(Reg::X(src2)), Location::Reg(Reg::X(dst))) = (src1, src2, dst) {
            dynasm!(self ; .arch aarch64 ; sub X(dst), X(src1), X(src2));
        } else if let (Location::Reg(Reg::X(src1)), Location::Imm32(src2), Location::Reg(Reg::X(dst))) = (src1, src2, dst) {
            dynasm!(self ; .arch aarch64 ; sub X(dst), X(src1), src2);
        } else if let (Location::Reg(Reg::SP), Location::Imm32(src2), Location::Reg(Reg::SP)) = (src1, src2, dst) {
            dynasm!(self ; .arch aarch64 ; sub sp, sp, src2);
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

    fn emit_call_location(&mut self, loc: Location) {
        match loc {
            Location::Reg(Reg::X(x)) => dynasm!(self ; .arch aarch64 ; blr X(x)),
            _ => unimplemented!()
        }
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
        
        (self as Assembler).finalize().unwrap().to_vec()
    }
}