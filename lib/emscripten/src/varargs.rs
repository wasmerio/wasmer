use crate::EmEnv;
use std::mem;
use wasmer::FromToNativeWasmType;
// use std::ffi::CStr;
use std::os::raw::c_char;
use wasmer::FunctionEnvMut;

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct VarArgs {
    pub pointer: u32, // assuming 32bit wasm
}

impl VarArgs {
    pub fn get<T: Sized>(&mut self, ctx: &FunctionEnvMut<EmEnv>) -> T {
        let memory = ctx.data().memory(0);
        let ptr = emscripten_memory_pointer!(memory.view(&ctx), self.pointer);
        self.pointer += mem::size_of::<T>() as u32;
        unsafe { (ptr as *const T).read() }
    }

    // pub fn getStr<'a>(&mut self, ctx: &mut Ctx) -> &'a CStr {
    pub fn get_str(&mut self, ctx: &FunctionEnvMut<EmEnv>) -> *const c_char {
        let ptr_addr: u32 = self.get(ctx);
        let memory = ctx.data().memory(0);
        emscripten_memory_pointer!(memory.view(&ctx), ptr_addr) as *const c_char
    }
}

unsafe impl FromToNativeWasmType for VarArgs {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.pointer as _
    }
    fn from_native(n: Self::Native) -> Self {
        Self { pointer: n as u32 }
    }
}
