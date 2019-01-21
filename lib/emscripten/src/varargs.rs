use std::mem;
use wasmer_runtime::{vm::Ctx, types::MemoryIndex, structures::TypedIndex};

#[repr(transparent)]
pub struct VarArgs {
    pub pointer: u32, // assuming 32bit wasm
}

impl VarArgs {
    pub fn get<T: Sized>(&mut self, vmctx: &mut Ctx) -> T {
        let ptr = vmctx.memory(MemoryIndex::new(0))[self.pointer as usize];
        self.pointer += mem::size_of::<T>() as u32;
        unsafe { (ptr as *const T).read() }
    }
}
