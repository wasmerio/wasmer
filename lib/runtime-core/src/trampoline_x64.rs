//! Trampoline generator for carrying context with function pointer.
//!
//! This makes use of the `mm0` register to pass the context as an implicit "parameter" because `mm0` is
//! not used to pass parameters and is almost never used by modern compilers. It's still better to call
//! `get_context()` as early as possible in the callee function though, as a good practice.
//!
//! Variadic functions are not supported because `rax` is used by the trampoline code.

use crate::loader::CodeMemory;
use crate::vm::Ctx;
use std::fmt;
use std::{mem, slice};

lazy_static! {
    /// Reads the context pointer from `mm0`.
    ///
    /// This function generates code at runtime since `asm!` macro is not yet stable.
    static ref GET_CONTEXT: extern "C" fn () -> *const CallContext = {
        static CODE: &'static [u8] = &[
            0x48, 0x0f, 0x7e, 0xc0, // movq %mm0, %rax
            0xc3, // retq
        ];
        let mut mem = CodeMemory::new(4096);
        mem[..CODE.len()].copy_from_slice(CODE);
        mem.make_executable();
        let ptr = mem.as_ptr();
        mem::forget(mem);
        unsafe {
            mem::transmute(ptr)
        }
    };
}

/// An opaque type for pointers to a callable memory location.
pub enum CallTarget {}

/// An opaque type for context pointers.
pub enum CallContext {}

/// An opaque type for generated trampolines' call entries.
pub enum Trampoline {}

/// Trampoline Buffer Builder.
pub struct TrampolineBufferBuilder {
    code: Vec<u8>,
    offsets: Vec<usize>,
}

/// Trampoline Buffer.
pub struct TrampolineBuffer {
    code: CodeMemory,
    offsets: Vec<usize>,
}

fn value_to_bytes<T: Copy>(ptr: &T) -> &[u8] {
    unsafe { slice::from_raw_parts(ptr as *const T as *const u8, mem::size_of::<T>()) }
}

/// Calls `GET_CONTEXT` and returns the current context.
pub fn get_context() -> *const CallContext {
    GET_CONTEXT()
}

impl TrampolineBufferBuilder {
    /// Creates a new empty `TrampolineBufferBuilder`.
    pub fn new() -> TrampolineBufferBuilder {
        TrampolineBufferBuilder {
            code: vec![],
            offsets: vec![],
        }
    }

    /// Adds a context trampoline.
    ///
    /// This generates a transparent trampoline function that forwards any call to `target` with
    /// unmodified params/returns. When called from the trampoline, `target` will have access to
    /// the `context` specified here through `get_context()`.
    ///
    /// Note that since `rax` is overwritten internally, variadic functions are not supported as `target`.
    pub fn add_context_trampoline(
        &mut self,
        target: *const CallTarget,
        context: *const CallContext,
    ) -> usize {
        let idx = self.offsets.len();
        self.offsets.push(self.code.len());
        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(value_to_bytes(&context));
        self.code.extend_from_slice(&[
            0x48, 0x0f, 0x6e, 0xc0, // movq %rax, %mm0
        ]);
        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(value_to_bytes(&target));
        self.code.extend_from_slice(&[
            0xff, 0xe0, // jmpq *%rax
        ]);
        idx
    }

    /// Adds context RSP state preserving trampoline to the buffer.
    pub fn add_context_rsp_state_preserving_trampoline(
        &mut self,
        target: unsafe extern "C" fn(&mut Ctx, *const CallContext, *const u64),
        context: *const CallContext,
    ) -> usize {
        let idx = self.offsets.len();
        self.offsets.push(self.code.len());

        self.code.extend_from_slice(&[
            0x53, // push %rbx
            0x41, 0x54, // push %r12
            0x41, 0x55, // push %r13
            0x41, 0x56, // push %r14
            0x41, 0x57, // push %r15
        ]);
        self.code.extend_from_slice(&[
            0x48, 0xbe, // movabsq ?, %rsi
        ]);
        self.code.extend_from_slice(value_to_bytes(&context));
        self.code.extend_from_slice(&[
            0x48, 0x89, 0xe2, // mov %rsp, %rdx
        ]);

        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(value_to_bytes(&target));
        self.code.extend_from_slice(&[
            0xff, 0xd0, // callq *%rax
        ]);
        self.code.extend_from_slice(&[
            0x48, 0x81, 0xc4, // add ?, %rsp
        ]);
        self.code.extend_from_slice(value_to_bytes(&40i32)); // 5 * 8
        self.code.extend_from_slice(&[
            0xc3, //retq
        ]);
        idx
    }

    /// Adds a callinfo trampoline.
    ///
    /// This generates a trampoline function that collects `num_params` parameters into an array
    /// and passes the array into `target` as the second argument when called. The first argument
    /// of `target` is the `context` specified here.
    ///
    /// Note that non-integer parameters/variadic functions are not supported.
    pub fn add_callinfo_trampoline(
        &mut self,
        target: unsafe extern "C" fn(*const CallContext, *const u64) -> u64,
        context: *const CallContext,
        num_params: u32,
    ) -> usize {
        let idx = self.offsets.len();
        self.offsets.push(self.code.len());

        let mut stack_offset: u32 = num_params.checked_mul(8).unwrap();
        if stack_offset % 16 == 0 {
            stack_offset += 8;
        }

        self.code.extend_from_slice(&[0x48, 0x81, 0xec]); // sub ?, %rsp
        self.code.extend_from_slice(value_to_bytes(&stack_offset));
        for i in 0..num_params {
            match i {
                0..=5 => {
                    // mov %?, ?(%rsp)
                    let prefix: &[u8] = match i {
                        0 => &[0x48, 0x89, 0xbc, 0x24], // rdi
                        1 => &[0x48, 0x89, 0xb4, 0x24], // rsi
                        2 => &[0x48, 0x89, 0x94, 0x24], // rdx
                        3 => &[0x48, 0x89, 0x8c, 0x24], // rcx
                        4 => &[0x4c, 0x89, 0x84, 0x24], // r8
                        5 => &[0x4c, 0x89, 0x8c, 0x24], // r9
                        _ => unreachable!(),
                    };
                    self.code.extend_from_slice(prefix);
                    self.code.extend_from_slice(value_to_bytes(&(i * 8u32)));
                }
                _ => {
                    self.code.extend_from_slice(&[
                        0x48, 0x8b, 0x84, 0x24, // mov ?(%rsp), %rax
                    ]);
                    self.code.extend_from_slice(value_to_bytes(
                        &((i - 6) * 8u32 + stack_offset + 8/* ret addr */),
                    ));
                    // mov %rax, ?(%rsp)
                    self.code.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
                    self.code.extend_from_slice(value_to_bytes(&(i * 8u32)));
                }
            }
        }
        self.code.extend_from_slice(&[
            0x48, 0xbf, // movabsq ?, %rdi
        ]);
        self.code.extend_from_slice(value_to_bytes(&context));
        self.code.extend_from_slice(&[
            0x48, 0x89, 0xe6, // mov %rsp, %rsi
        ]);

        self.code.extend_from_slice(&[
            0x48, 0xb8, // movabsq ?, %rax
        ]);
        self.code.extend_from_slice(value_to_bytes(&target));
        self.code.extend_from_slice(&[
            0xff, 0xd0, // callq *%rax
        ]);
        self.code.extend_from_slice(&[
            0x48, 0x81, 0xc4, // add ?, %rsp
        ]);
        self.code.extend_from_slice(value_to_bytes(&stack_offset));
        self.code.extend_from_slice(&[
            0xc3, //retq
        ]);
        idx
    }

    /// Consumes the builder and builds the trampoline buffer.
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
    /// Returns the trampoline pointer at index `idx`.
    pub fn get_trampoline(&self, idx: usize) -> *const Trampoline {
        &self.code[self.offsets[idx]] as *const u8 as *const Trampoline
    }
}

impl fmt::Debug for TrampolineBuffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TrampolineBuffer {{}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_context_trampoline() {
        struct TestContext {
            value: i32,
        }
        extern "C" fn do_add(a: i32, b: f32) -> f32 {
            let ctx = unsafe { &*(get_context() as *const TestContext) };
            a as f32 + b + ctx.value as f32
        }
        let mut builder = TrampolineBufferBuilder::new();
        let ctx = TestContext { value: 3 };
        let idx = builder.add_context_trampoline(
            do_add as usize as *const _,
            &ctx as *const TestContext as *const _,
        );
        let buf = builder.build();
        let t = buf.get_trampoline(idx);
        let ret = unsafe { mem::transmute::<_, extern "C" fn(i32, f32) -> f32>(t)(1, 2.0) as i32 };
        assert_eq!(ret, 6);
    }
    #[test]
    fn test_callinfo_trampoline() {
        struct TestContext {
            value: i32,
        }
        unsafe extern "C" fn do_add(ctx: *const CallContext, args: *const u64) -> u64 {
            let ctx = &*(ctx as *const TestContext);
            let args: &[u64] = slice::from_raw_parts(args, 8);
            (args.iter().map(|x| *x as i32).fold(0, |a, b| a + b) + ctx.value) as u64
        }
        let mut builder = TrampolineBufferBuilder::new();
        let ctx = TestContext { value: 100 };
        let idx =
            builder.add_callinfo_trampoline(do_add, &ctx as *const TestContext as *const _, 8);
        let buf = builder.build();
        let t = buf.get_trampoline(idx);
        let ret = unsafe {
            mem::transmute::<_, extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32) -> i32>(t)(
                1, 2, 3, 4, 5, 6, 7, 8,
            ) as i32
        };
        assert_eq!(ret, 136);
    }
}
