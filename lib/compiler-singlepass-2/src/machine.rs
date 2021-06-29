use crate::common_decl::*;
use crate::codegen::Local;
// use smallvec::smallvec;
use smallvec::SmallVec;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer::Value;
use wasmer_types::{FunctionType, FunctionIndex};
use wasmer_vm::VMOffsets;

use wasmer_compiler::{Relocation, RelocationTarget};

use std::fmt::Debug;

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
    fn is_imm(&self) -> bool {
        self.imm_value().is_some()
    }
}

pub struct CallInfo<T: Copy> {
    pub returns: SmallVec<[Local<T>; 1]>,
    pub before_call: usize,
    pub after_call: usize,
}

pub trait Machine {
    type Location: MaybeImmediate + Copy + Eq + Debug;
    type Label: Copy;
    const BR_INSTR_SIZE: usize;

    fn new() -> Self;
    // fn new_state() -> MachineState;
    // fn get_state(&mut self) -> &mut MachineState;
    fn get_assembly_offset(&mut self) -> usize;
    fn new_label(&mut self) -> Self::Label;
    fn do_const_i32(&mut self, n: i32) -> Local<Self::Location>;
    fn release_location(&mut self, loc: Local<Self::Location>);
    fn func_begin(&mut self, n_locals: usize, n_params: usize) -> Vec<Local<Self::Location>>;
    fn func_end(&mut self, end_label: Self::Label) -> Vec<Relocation>;
    fn block_begin(&mut self);
    fn block_end(&mut self, end_label: Self::Label);
    fn do_add_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_add_p(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_sub_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_mul_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_le_u_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_lt_u_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_ge_u_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_and_i32(&mut self, src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn do_eqz_i32(&mut self, src: Local<Self::Location>) -> Local<Self::Location>;
    fn do_call(&mut self, reloc_target: RelocationTarget, args: &[Local<Self::Location>], return_types: &[WpType]) -> CallInfo<Self::Location>;    
    fn do_return(&mut self, ty: Option<WpType>, ret_val: Option<Local<Self::Location>>, end_label: Self::Label);
    fn do_emit_label(&mut self, label: Self::Label);
    fn do_load_label(&mut self, label: Self::Label) -> Local<Self::Location>;
    fn do_br_label(&mut self, label: Self::Label, depth: u32);
    fn do_br_cond_label(&mut self, cond: Local<Self::Location>, label: Self::Label, depth: u32);
    fn do_br_not_cond_label(&mut self, cond: Local<Self::Location>, label: Self::Label, depth: u32);
    fn do_br_location(&mut self, loc: Local<Self::Location>, depth: u32);
    fn do_load_from_vmctx(&mut self, sz: Size, offset: u32) -> Local<Self::Location>;
    fn do_deref(&mut self, sz: Size, ptr: Local<Self::Location>) -> Local<Self::Location>;
    fn do_deref_write(&mut self, sz: Size, ptr: Local<Self::Location>, val: Local<Self::Location>);
    fn do_ptr_offset(&mut self, sz: Size, ptr: Local<Self::Location>, offset: i32) -> Local<Self::Location>;
    fn do_vmctx_ptr_offset(&mut self, sz: Size, offset: i32) -> Local<Self::Location>;
    fn do_normalize_local(&mut self, local: Local<Self::Location>) -> Local<Self::Location>;
    fn do_restore_local(&mut self, local: Local<Self::Location>, location: Self::Location) -> Local<Self::Location>;
    fn finalize(self) -> Vec<u8>;

    fn gen_std_trampoline(sig: &FunctionType) -> Vec<u8>;
    fn gen_std_dynamic_import_trampoline(vmoffsets: &VMOffsets, sig: &FunctionType) -> Vec<u8>;
    fn gen_import_call_trampoline(vmoffsets: &VMOffsets, index: FunctionIndex, sig: &FunctionType) -> Vec<u8>;
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
