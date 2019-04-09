use std::mem;
use wasmer_runtime_core::{
    types::{Type, WasmExternType},
    vm::Ctx,
};

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct VarArgs {
    pub pointer: u32, // assuming 32bit wasm
}

impl VarArgs {
    pub fn get<T: Sized>(&mut self, ctx: &mut Ctx) -> T {
        let ptr = emscripten_memory_pointer!(ctx.memory(0), self.pointer);
        self.pointer += mem::size_of::<T>() as u32;
        unsafe { (ptr as *const T).read() }
    }
}

unsafe impl WasmExternType for VarArgs {
    const TYPE: Type = Type::I32;

    fn to_bits(self) -> u64 {
        self.pointer as u64
    }
    fn from_bits(n: u64) -> Self {
        Self { pointer: n as u32 }
    }
}
