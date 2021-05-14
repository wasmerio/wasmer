use crate::common_decl::*;
// use smallvec::smallvec;
use smallvec::SmallVec;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer::Value;
use wasmer_types::{FunctionType, FunctionIndex};
use wasmer_vm::VMOffsets;

use wasmer_compiler::{Relocation, RelocationTarget};

// const NATIVE_PAGE_SIZE: usize = 4096;

use std::fmt::Debug;

// pub struct Machine {
//     used_gprs: HashSet<GPR>,
//     used_xmms: HashSet<XMM>,
//     stack_offset: MachineStackOffset,
//     save_area_offset: Option<MachineStackOffset>,
//     pub state: MachineState,
//     pub(crate) track_state: bool,
// }

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
}

pub struct CallInfo<T: Copy> {
    pub returns: SmallVec<[Local<T>; 1]>,
    pub before_call: usize,
    pub after_call: usize,
}

pub trait Machine {
    type Location: MaybeImmediate + Copy + Debug;
    type Label: Copy;

    fn new_state() -> MachineState;
    fn get_state(&mut self) -> &mut MachineState;
    fn get_assembly_offset(&mut self) -> usize;
    fn new_label(&mut self) -> Self::Label;
    fn imm32(&mut self, n: u32) -> Local<Self::Location>;
    fn release_location(&mut self, loc: Local<Self::Location>);
    fn init(&mut self, n_locals: usize, n_params: usize) -> Vec<Local<Self::Location>>;
    fn emit_end(&mut self, end_label: Self::Label) -> Vec<Relocation>;
    fn emit_add_i32(&mut self, sz: Size,
        src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;
    fn emit_sub_i32(&mut self, sz: Size,
        src1: Local<Self::Location>, src2: Local<Self::Location>) -> Local<Self::Location>;    
    fn emit_call(&mut self, reloc_target: RelocationTarget,
        params: &[Local<Self::Location>], return_types: &[WpType]) -> CallInfo<Self::Location>;    
    fn emit_return(&mut self, ty: WpType, loc: Local<Self::Location>);
    fn finalize(self) -> Vec<u8>;

    fn gen_std_trampoline(
        sig: &FunctionType) -> Vec<u8>;
    fn gen_std_dynamic_import_trampoline(
        vmoffsets: &VMOffsets,
        sig: &FunctionType) -> Vec<u8>;
    fn gen_import_call_trampoline(
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType) -> Vec<u8>;
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
