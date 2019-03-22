use crate::alloc_pool::{AllocErr, AllocId, AllocMetadata, AllocPool, ItemAlloc};
use std::{
    alloc::{alloc, Layout},
    any::Any,
    mem::{align_of, size_of},
    ptr::NonNull,
    slice,
};
use wasmer_runtime_core::types::LocalFuncIndex;

pub trait KeepAlive: Send + 'static {}

impl<T> KeepAlive for T where T: Send + 'static {}

struct CodeAlloc {
    metadata: Metadata,
    keep_alive: Box<dyn KeepAlive>,
}

unsafe impl ItemAlloc for CodeAlloc {
    type Output = Code;

    fn metadata(&self) -> AllocMetadata {
        AllocMetadata {
            size: size_of::<Code>() + self.metadata.code_size as usize,
            executable: true,
        }
    }

    unsafe fn in_place(self, header: *mut Code) {
        (&mut (*header).keep_alive as *mut Box<dyn KeepAlive>).write(self.keep_alive);
        (&mut (*header).call_offsets as *mut Box<[CallOffset]>).write(Box::new([]));
        (*header).metadata = self.metadata;
    }
}

#[repr(C)]
pub struct CallOffset {
    pub func_index: LocalFuncIndex,
    pub offset: u32,
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Metadata {
    pub func_index: LocalFuncIndex,
    pub code_size: u32,
}

#[repr(C)]
pub struct Code {
    pub keep_alive: Box<dyn KeepAlive>,
    pub call_offsets: Box<[CallOffset]>,
    pub metadata: Metadata,
    code: [u8; 0],
}

impl Code {
    pub fn new(
        pool: &AllocPool,
        keep_alive: impl KeepAlive,
        metadata: Metadata,
    ) -> Result<AllocId<Code>, AllocErr> {
        let code_alloc = CodeAlloc {
            keep_alive: Box::new(keep_alive),
            metadata,
        };

        pool.alloc(code_alloc)
    }

    pub fn code_ptr(&self) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(self.code.as_ptr() as *mut u8) }
    }

    pub fn code_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self.code.as_mut_ptr(), self.metadata.code_size as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer_runtime_core::structures::TypedIndex;

    #[test]
    fn test_alloc() {
        let pool = AllocPool::new();
        let _code = Code::new(
            &pool,
            (),
            Metadata {
                func_index: LocalFuncIndex::new(0),
                code_size: 16,
            },
        )
        .unwrap();
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

        let pool = AllocPool::new();
        let mut code_id = Code::new(
            &pool,
            (),
            Metadata {
                func_index: LocalFuncIndex::new(0),
                code_size: 16,
            },
        )
        .unwrap();

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
