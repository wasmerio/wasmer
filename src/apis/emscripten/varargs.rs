use crate::webassembly::Instance;
use std::mem;

#[repr(transparent)]
pub struct VarArgs {
    pub pointer: u32, // assuming 32bit wasm
}

impl VarArgs {
    pub fn get<T: Sized>(&mut self, instance: &mut Instance) -> T {
        let ptr = instance.memory_offset_addr(0, self.pointer as usize);
        self.pointer += mem::size_of::<T>() as u32;
        unsafe { (ptr as *const T).read() }
    }
}
