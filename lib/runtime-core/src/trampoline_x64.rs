//! Trampoline generator for carrying context with function pointer.
//!
//! This makes use of the `mm0` register to pass the context as an implicit "parameter" because `mm0` is
//! not used to pass parameters and is almost never used by modern compilers. It's still better to call
//! `get_context()` as early as possible in the callee function though, as a good practice.
//!
//! Variadic functions are not supported because `rax` is used by the trampoline code.

use crate::loader::CodeMemory;

lazy_static! {
    static ref GET_CONTEXT: extern "C" fn () -> *const CallContext = {
        static CODE: &'static [u8] = &[
            0x48, 0x0f, 0x7e, 0xc0, // movq %mm0, %rax
            0xc3, // retq
        ];
        let mut mem = CodeMemory::new(4096);
        mem[..CODE.len()].copy_from_slice(CODE);
        mem.make_executable();
        let ptr = mem.as_ptr();
        ::std::mem::forget(mem);
        unsafe {
            ::std::mem::transmute(ptr)
        }
    };
}

pub enum CallTarget {}
pub enum CallContext {}
pub enum Trampoline {}

pub struct TrampolineBufferBuilder {
    code: Vec<u8>,
    offsets: Vec<usize>,
}

pub struct TrampolineBuffer {
    code: CodeMemory,
    offsets: Vec<usize>,
}

fn pointer_to_bytes<T>(ptr: &*const T) -> &[u8] {
    unsafe {
        ::std::slice::from_raw_parts(
            ptr as *const *const T as *const u8,
            ::std::mem::size_of::<*const T>(),
        )
    }
}

pub fn get_context() -> *const CallContext {
    GET_CONTEXT()
}

impl TrampolineBufferBuilder {
    pub fn new() -> TrampolineBufferBuilder {
        TrampolineBufferBuilder {
            code: vec![],
            offsets: vec![],
        }
    }

    pub fn add_function(
        &mut self,
        target: *const CallTarget,
        context: *const CallContext,
    ) -> usize {
        let idx = self.offsets.len();
        self.offsets.push(self.code.len());
        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(pointer_to_bytes(&context));
        self.code.extend_from_slice(&[
            0x48, 0x0f, 0x6e, 0xc0, // movq %rax, %mm0
        ]);
        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(pointer_to_bytes(&target));
        self.code.extend_from_slice(&[
            0xff, 0xe0, // jmpq *%rax
        ]);
        idx
    }

    pub fn build(self) -> TrampolineBuffer {
        get_context(); // ensure lazy initialization is completed

        let mut code = CodeMemory::new(self.code.len());
        code[..self.code.len()].copy_from_slice(&self.code);
        code.make_executable();
        TrampolineBuffer {
            code,
            offsets: self.offsets,
        }
    }
}

impl TrampolineBuffer {
    pub fn get_trampoline(&self, idx: usize) -> *const Trampoline {
        &self.code[self.offsets[idx]] as *const u8 as *const Trampoline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_trampoline_call() {
        struct TestContext {
            value: i32,
        }
        extern "C" fn do_add(a: i32, b: f32) -> f32 {
            let ctx = unsafe { &*(get_context() as *const TestContext) };
            a as f32 + b + ctx.value as f32
        }
        let mut builder = TrampolineBufferBuilder::new();
        let ctx = TestContext { value: 3 };
        let idx = builder.add_function(
            do_add as usize as *const _,
            &ctx as *const TestContext as *const _,
        );
        let buf = builder.build();
        let t = buf.get_trampoline(idx);
        let ret =
            unsafe { ::std::mem::transmute::<_, extern "C" fn(i32, f32) -> f32>(t)(1, 2.0) as i32 };
        assert_eq!(ret, 6);
    }
}
