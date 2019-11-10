//! The loader module functions are used to load an instance.
use crate::{backend::RunnableModule, module::ModuleInfo, types::Type, types::Value, vm::Ctx};
#[cfg(unix)]
use libc::{mmap, mprotect, munmap, MAP_ANON, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

/// The loader trait represents the functions used to load an instance.
pub trait Loader {
    /// The type of `Instance` for the loader.
    type Instance: Instance;
    /// The error type returned by the loader.
    type Error: Debug;

    /// Loads the given module and context into an instance.
    fn load(
        &self,
        rm: &dyn RunnableModule,
        module: &ModuleInfo,
        ctx: &Ctx,
    ) -> Result<Self::Instance, Self::Error>;
}

/// This trait represents an instance used by the loader.
pub trait Instance {
    /// The error type returned by this instance.
    type Error: Debug;
    /// Call a function by id with the given args.
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u128, Self::Error>;
    /// Read memory at the given offset and length.
    fn read_memory(&mut self, _offset: u32, _len: u32) -> Result<Vec<u8>, Self::Error> {
        unimplemented!("Instance::read_memory")
    }
    /// Write memory at the given offset and length.
    fn write_memory(&mut self, _offset: u32, _len: u32, _buf: &[u8]) -> Result<(), Self::Error> {
        unimplemented!("Instance::write_memory")
    }
}

/// A local implementation for `Loader`.
pub struct LocalLoader;

impl Loader for LocalLoader {
    type Instance = LocalInstance;
    type Error = String;

    fn load(
        &self,
        rm: &dyn RunnableModule,
        _module: &ModuleInfo,
        _ctx: &Ctx,
    ) -> Result<Self::Instance, Self::Error> {
        let code = rm.get_code().unwrap();
        let mut code_mem = CodeMemory::new(code.len());
        code_mem[..code.len()].copy_from_slice(code);
        code_mem.make_executable();

        Ok(LocalInstance {
            code: code_mem,
            offsets: rm.get_offsets().unwrap(),
        })
    }
}

/// A local instance.
pub struct LocalInstance {
    code: CodeMemory,
    offsets: Vec<usize>,
}

impl Instance for LocalInstance {
    type Error = String;
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u128, Self::Error> {
        let mut args_u64: Vec<u64> = Vec::new();
        for arg in args {
            if arg.ty() == Type::V128 {
                let bytes = arg.to_u128().to_le_bytes();
                let mut lo = [0u8; 8];
                lo.clone_from_slice(&bytes[0..8]);
                args_u64.push(u64::from_le_bytes(lo));
                let mut hi = [0u8; 8];
                hi.clone_from_slice(&bytes[8..16]);
                args_u64.push(u64::from_le_bytes(hi));
            } else {
                args_u64.push(arg.to_u128() as u64);
            }
        }
        let offset = self.offsets[id];
        let addr: *const u8 = unsafe { self.code.as_ptr().offset(offset as isize) };
        use std::mem::transmute;
        Ok(unsafe {
            match args_u64.len() {
                0 => (transmute::<_, extern "C" fn() -> u128>(addr))(),
                1 => (transmute::<_, extern "C" fn(u64) -> u128>(addr))(args_u64[0]),
                2 => (transmute::<_, extern "C" fn(u64, u64) -> u128>(addr))(
                    args_u64[0],
                    args_u64[1],
                ),
                3 => (transmute::<_, extern "C" fn(u64, u64, u64) -> u128>(addr))(
                    args_u64[0],
                    args_u64[1],
                    args_u64[2],
                ),
                4 => (transmute::<_, extern "C" fn(u64, u64, u64, u64) -> u128>(addr))(
                    args_u64[0],
                    args_u64[1],
                    args_u64[2],
                    args_u64[3],
                ),
                5 => (transmute::<_, extern "C" fn(u64, u64, u64, u64, u64) -> u128>(addr))(
                    args_u64[0],
                    args_u64[1],
                    args_u64[2],
                    args_u64[3],
                    args_u64[4],
                ),
                _ => return Err("too many arguments".into()),
            }
        })
    }
}

/// A pointer to code in memory.
pub struct CodeMemory {
    ptr: *mut u8,
    size: usize,
}

unsafe impl Send for CodeMemory {}
unsafe impl Sync for CodeMemory {}

#[cfg(not(unix))]
impl CodeMemory {
    /// Creates a new code memory with the given size.
    pub fn new(_size: usize) -> CodeMemory {
        unimplemented!("CodeMemory::new");
    }

    /// Makes this code memory executable.
    pub fn make_executable(&self) {
        unimplemented!("CodeMemory::make_executable");
    }

    /// Makes this code memory writable.
    pub fn make_writable(&self) {
        unimplemented!("CodeMemory::make_writable");
    }
}

#[cfg(unix)]
impl CodeMemory {
    /// Creates a new code memory with the given size.
    pub fn new(size: usize) -> CodeMemory {
        if size == 0 {
            return CodeMemory {
                ptr: std::ptr::null_mut(),
                size: 0,
            };
        }

        fn round_up_to_page_size(size: usize) -> usize {
            (size + (4096 - 1)) & !(4096 - 1)
        }
        let size = round_up_to_page_size(size);
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if ptr as isize == -1 {
            panic!("cannot allocate code memory");
        }
        CodeMemory {
            ptr: ptr as _,
            size: size,
        }
    }

    /// Makes this code memory executable.
    pub fn make_executable(&self) {
        if unsafe { mprotect(self.ptr as _, self.size, PROT_READ | PROT_EXEC) } != 0 {
            panic!("cannot set code memory to executable");
        }
    }

    /// Makes this code memory writable.
    pub fn make_writable(&self) {
        if unsafe { mprotect(self.ptr as _, self.size, PROT_READ | PROT_WRITE) } != 0 {
            panic!("cannot set code memory to writable");
        }
    }
}

#[cfg(unix)]
impl Drop for CodeMemory {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr as _, self.size);
        }
    }
}

impl Deref for CodeMemory {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }
}

impl DerefMut for CodeMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}
