use crate::pool::{AllocErr, AllocId, AllocMetadata, ItemAlloc, PagePool};
use std::{
    alloc::{alloc, Layout},
    mem::{align_of, size_of},
    slice,
};
use wasmer_runtime_core::types::FuncIndex;

struct CodeAlloc<'a> {
    call_offsets: &'a [CallOffset],
    code_size: u32,
}

unsafe impl<'a> ItemAlloc for CodeAlloc<'a> {
    type Output = Code;

    fn metadata(&self) -> AllocMetadata {
        AllocMetadata {
            size: size_of::<Code>()
                + (self.call_offsets.len() * size_of::<CallOffset>())
                + self.code_size as usize,
            executable: true,
        }
    }

    unsafe fn in_place(self, header: *mut Code) {
        (*header).code_size = self.code_size;
        (*header).call_offsets_len = self.call_offsets.len() as u32;
        self.call_offsets
            .as_ptr()
            .copy_to((*header).call_offsets.as_mut_ptr(), self.call_offsets.len());
    }
}

#[repr(C)]
pub struct CallOffset {
    func_index: FuncIndex,
    offset: u32,
}

#[repr(C)]
pub struct Code {
    code_size: u32,
    call_offsets_len: u32,
    call_offsets: [CallOffset; 0],
}

impl Code {
    pub fn new(
        pool: &PagePool,
        code_size: u32,
        call_offsets: &[CallOffset],
    ) -> Result<AllocId<Code>, AllocErr> {
        let code_alloc = CodeAlloc {
            call_offsets,
            code_size,
        };

        pool.alloc(code_alloc)
    }

    pub fn call_offsets(&self) -> &[CallOffset] {
        let call_offsets_len = self.call_offsets_len as usize;
        unsafe { slice::from_raw_parts(self.call_offsets.as_ptr(), call_offsets_len) }
    }

    pub fn code_ptr(&self) -> *const u8 {
        let call_offsets_len = self.call_offsets_len as usize;
        unsafe { (&self.call_offsets as *const CallOffset).add(call_offsets_len) as *const u8 }
    }

    pub fn code_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.code_ptr() as *mut u8, self.code_size as usize) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc() {
        let pool = PagePool::new();
        let _code = Code::new(&pool, 16, &[]).unwrap();
    }

    #[test]
    fn test_exec() {
        fn assemble_jmp(address: u64) -> [u8; 16] {
            let mut buf = [0; 16];

            buf[..2].copy_from_slice(&[0x48, 0xb8]);
            buf[2..10].copy_from_slice(&address.to_le_bytes());
            buf[10..12].copy_from_slice(&[0xff, 0xe0]);

            buf
        }

        unsafe fn callable() -> usize {
            42
        }

        let pool = PagePool::new();
        let mut code_id = Code::new(&pool, 16, &[]).unwrap();

        let mut code = pool.get_mut(&mut code_id);

        code.code_mut()
            .copy_from_slice(&assemble_jmp(callable as u64));

        let result = unsafe {
            let func_ptr: unsafe fn() -> usize = std::mem::transmute(code.code_ptr());
            func_ptr()
        };

        assert_eq!(result, 42);
    }
}
