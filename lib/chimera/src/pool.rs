//! This module reserves virtual address space and hands out pages from it.

use std::{
    marker::PhantomData,
    any::TypeId,
    mem::{align_of, size_of, transmute},
};
use wasmer_runtime_core::backend::sys::{Memory, Protect};
use parking_lot::Mutex;

#[cfg(target_arch = "x86_64")]
const POOL_SIZE: usize = 1 << 31; // 2 GB
#[cfg(target_arch = "aarch64")]
const POOL_SIZE: usize = 1 << 27; // 128 MB

pub struct AllocMetadata {
    size: usize,
    executable: bool,
}

pub unsafe trait ItemAlloc {
    type Output;

    fn metadata(&self) -> AllocMetadata {
        AllocMetadata {
            size: size_of::<Self::Output>(),
            executable: false,
        }
    }

    unsafe fn in_place(self, output: *mut Self::Output);
}

pub struct AllocId<T>(u32, PhantomData<T>);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AllocErr {
    OutOfSpace,
    CantProtect,
}

struct PagePoolInner {
    memory: Memory,
    offset_map: Vec<(usize, TypeId)>,
    bump_page: usize,
}

pub struct PagePool {
    inner: Mutex<PagePoolInner>,
    memory_start: *mut u8,
}

impl PagePool {
    pub fn new() -> Self {
        let memory = Memory::with_size(POOL_SIZE).unwrap();
        let memory_start = memory.as_ptr();

        PagePool {
            inner: Mutex::new(PagePoolInner {
                memory,
                offset_map: Vec::new(),
                bump_page: 0,
            }),
            memory_start,
        }
    }

    pub fn alloc<A: ItemAlloc>(&self, item_alloc: A) -> Result<AllocId<A::Output>, AllocErr> {
        assert!(align_of::<A::Output>() <= 4096);

        let AllocMetadata { size, executable } = item_alloc.metadata();

        let total_size = round_up_to_page_size(size, 4096);

        let mut inner = self.inner.lock();
        
        let current_bump_offset = inner.bump_page;
        inner.bump_page += total_size / 4096;

        let offset = current_bump_offset.wrapping_mul(4096) as usize;
        let total_size = round_up_to_page_size(size, 4096);
        if offset + total_size >= POOL_SIZE {
            return Err(AllocErr::OutOfSpace);
        }

        unsafe {
            inner.memory
                .protect(
                    offset..size,
                    match executable {
                        false => Protect::ReadWrite,
                        true => Protect::ReadWriteExec,
                    },
                )
                .map_err(|_| AllocErr::CantProtect)?;

            let ptr = inner.memory.as_ptr().add(offset);
            item_alloc.in_place(ptr as *mut A::Output);
        }

        Ok(AllocId(offset as u32, PhantomData))
    }

    pub unsafe fn get<T>(&self, index: &AllocId<T>) -> &T {
        &*(self.memory_start.add(index.0 as usize) as *const T)
    }

    pub unsafe fn get_mut<T>(&self, index: &mut AllocId<T>) -> &mut T {
        &mut *(self.memory_start.add(index.0 as usize) as *mut T)
    }
}

/// Round `size` up to the nearest multiple of `page_size`.
fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
    (size + (page_size - 1)) & !(page_size - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation() {
        let _pool = PagePool::new();
    }

    #[test]
    fn allocation() {
        struct Foobar {
            a: usize,
            b: usize,
        }

        struct FoobarAlloc;

        unsafe impl ItemAlloc for FoobarAlloc {
            type Output = Foobar;
            unsafe fn in_place(self, output: *mut Self::Output) {
                (*output).a = 42;
                (*output).b = 52;
            }
        }

        let pool = PagePool::new();
        let foobar_id = pool.alloc(FoobarAlloc).unwrap();

        let foobar = unsafe { pool.get(&foobar_id) };

        assert_eq!(foobar.a, 42);
        assert_eq!(foobar.b, 52);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_executable() {
        unsafe fn callable() -> usize {
            42
        }

        struct Callable {
            buf: [u8; 16],
        }

        struct CallableAlloc;

        unsafe impl ItemAlloc for CallableAlloc {
            type Output = Callable;
            fn metadata(&self) -> AllocMetadata {
                AllocMetadata {
                    size: 16,
                    executable: true,
                }
            }
            
            unsafe fn in_place(self, output: *mut Callable) {
                fn assemble_jmp(address: u64) -> [u8; 16] {
                    let mut buf = [0; 16];

                    buf[..2].copy_from_slice(&[0x48, 0xb8]);
                    buf[2..10].copy_from_slice(&address.to_le_bytes());
                    buf[10..12].copy_from_slice(&[0xff, 0xe0]);

                    buf
                }

                (*output).buf = assemble_jmp(callable as u64);
            }
        }

        let pool = PagePool::new();
        let callable_id = pool.alloc(CallableAlloc).unwrap();
        let result = unsafe {
            let callable_ref = pool.get(&callable_id);
            let func_ptr: unsafe fn() -> usize = transmute(callable_ref.buf.as_ptr());
            func_ptr()
        };

        assert_eq!(result, 42);
    }
}
