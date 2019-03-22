use crate::alloc_pool::{AllocId, AllocMetadata, AllocPool};
use std::arch::x86_64::cmpxchg16b;
use std::sync::atomic::Ordering;
use wasmer_runtime_core::{
    backend::sys::{Memory, Protect},
    module::ModuleInfo,
    structures::TypedIndex,
    types::LocalFuncIndex,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const CACHE_LINE_SIZE: usize = 64;

/// This assembles a relative jmp instruction with a relative
/// offset that is in-respect to the beginning of the returned buffer.
/// To prevent false-sharing, we need to align this to cache-lines (64 bytes).
/// This generates:
////    movabs rax, <absolute function address>
////    jmp rax
fn assemble_jmp(address: u64) -> [u8; 16] {
    let mut buf = [0; 16];

    buf[..2].copy_from_slice(&[0x48, 0xb8]);
    buf[2..10].copy_from_slice(&address.to_le_bytes());
    buf[10..12].copy_from_slice(&[0xff, 0xe0]);

    buf
}

/// This atomically updates a (possibly live) thunk to jump to a new
/// address. This function unfortunately requires nightly rust.
unsafe fn update_thunk(thunk: *mut u8, new_address: u64) {
    debug_assert!(thunk as usize % 16 == 0);

    let cast_thunk = thunk as *mut u128;
    let mut old_value = *cast_thunk;
    let new_value = u128::from_le_bytes(assemble_jmp(new_address));
    while {
        let read_value = cmpxchg16b(
            cast_thunk,
            old_value,
            new_value,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        let temp_old_value = old_value;
        old_value = read_value;
        temp_old_value != read_value
    } {}
}

pub struct ThunkTable {
    mem: Memory,
}

impl ThunkTable {
    /// This creates an empty `ThunkTable`.
    pub fn new(num_local_functions: usize, filler: impl Fn(LocalFuncIndex) -> u64) -> Self {
        let thunk_table_size = CACHE_LINE_SIZE * num_local_functions;
        let mem = Memory::with_size_protect(thunk_table_size, Protect::ReadWriteExec).unwrap();

        let table = Self { mem };

        for index in 0..num_local_functions {
            let func_index = LocalFuncIndex::new(index);
            let ptr = table.thunk_ptr(func_index) as *mut [u8; 16];
            let address = filler(func_index);
            unsafe { ptr.write(assemble_jmp(address)) };
        }

        table
    }

    /// This atomically updates a (possibly live) thunk to jump to a new
    /// address.
    pub unsafe fn update_thunk(&self, index: LocalFuncIndex, new_address: u64) {
        let thunk_ptr = (self.mem.as_ptr() as *mut [u8; 64]).add(index.index());

        update_thunk(thunk_ptr as *mut u8, new_address);
    }

    pub fn thunk_ptr(&self, index: LocalFuncIndex) -> *const [u8; 64] {
        unsafe { (self.mem.as_ptr() as *mut [u8; 64]).add(index.index()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn valid_thunk_table() {
        let thunk_table = ThunkTable::new(2, |_| 0);
        unsafe {
            thunk_table.update_thunk(LocalFuncIndex::new(1), 0xcafebabecafebabe);
        }

        let buffer = unsafe { thunk_table.thunk_ptr(LocalFuncIndex::new(1)).read() };

        assert_eq!(
            &buffer as &[u8],
            &[
                0x48u8, 0xb8, 0xbe, 0xba, 0xfe, 0xca, 0xbe, 0xba, 0xfe, 0xca, 0xff, 0xe0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ] as &[u8],
        );
    }

    #[test]
    fn callable_thunk_table() {
        extern "C" fn identity(a: i32) -> i32 {
            a
        }

        let thunk_table = ThunkTable::new(1, |_| 0);

        let f: extern "C" fn(i32) -> i32 = unsafe {
            thunk_table.update_thunk(LocalFuncIndex::new(0), identity as u64);
            mem::transmute(thunk_table.thunk_ptr(LocalFuncIndex::new(0)))
        };

        assert_eq!(f(42), 42);
    }

    #[test]
    fn init_callable_thunk_table() {
        extern "C" fn identity(a: i32) -> i32 {
            a
        }

        let thunk_table = ThunkTable::new(1, |_| identity as u64);

        let f: extern "C" fn(i32) -> i32 =
            unsafe { mem::transmute(thunk_table.thunk_ptr(LocalFuncIndex::new(0))) };

        assert_eq!(f(42), 42);
    }
}
