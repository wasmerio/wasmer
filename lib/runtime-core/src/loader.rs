use crate::{backend::RunnableModule, module::ModuleInfo, types::Value, vm::Ctx};
#[cfg(unix)]
use libc::{mmap, mprotect, munmap, MAP_ANON, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

pub trait Loader {
    type Instance: Instance;
    type Error: Debug;

    fn load(
        &self,
        rm: &dyn RunnableModule,
        module: &ModuleInfo,
        ctx: &Ctx,
    ) -> Result<Self::Instance, Self::Error>;
}

pub trait Instance {
    type Error: Debug;
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u64, Self::Error>;
    fn read_memory(&mut self, _offset: u32, _len: u32) -> Result<Vec<u8>, Self::Error> {
        unimplemented!()
    }

    fn write_memory(&mut self, _offset: u32, _len: u32, _buf: &[u8]) -> Result<(), Self::Error> {
        unimplemented!()
    }
}

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

pub struct LocalInstance {
    code: CodeMemory,
    offsets: Vec<usize>,
}

impl Instance for LocalInstance {
    type Error = String;
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u64, Self::Error> {
        let offset = self.offsets[id];
        let addr: *const u8 = unsafe { self.code.as_ptr().offset(offset as isize) };
        use std::mem::transmute;
        Ok(unsafe {
            match args.len() {
                0 => (transmute::<_, extern "C" fn() -> u64>(addr))(),
                1 => (transmute::<_, extern "C" fn(u64) -> u64>(addr))(args[0].to_u64()),
                2 => (transmute::<_, extern "C" fn(u64, u64) -> u64>(addr))(
                    args[0].to_u64(),
                    args[1].to_u64(),
                ),
                3 => (transmute::<_, extern "C" fn(u64, u64, u64) -> u64>(addr))(
                    args[0].to_u64(),
                    args[1].to_u64(),
                    args[2].to_u64(),
                ),
                4 => (transmute::<_, extern "C" fn(u64, u64, u64, u64) -> u64>(addr))(
                    args[0].to_u64(),
                    args[1].to_u64(),
                    args[2].to_u64(),
                    args[3].to_u64(),
                ),
                5 => (transmute::<_, extern "C" fn(u64, u64, u64, u64, u64) -> u64>(addr))(
                    args[0].to_u64(),
                    args[1].to_u64(),
                    args[2].to_u64(),
                    args[3].to_u64(),
                    args[4].to_u64(),
                ),
                _ => return Err("too many arguments".into()),
            }
        })
    }
}

pub struct CodeMemory {
    ptr: *mut u8,
    size: usize,
}

#[cfg(not(unix))]
impl CodeMemory {
    pub fn new(_size: usize) -> CodeMemory {
        unimplemented!();
    }

    pub fn make_executable(&mut self) {
        unimplemented!();
    }
}

#[cfg(unix)]
impl CodeMemory {
    pub fn new(size: usize) -> CodeMemory {
        fn round_up_to_page_size(size: usize) -> usize {
            (size + (4096 - 1)) & !(4096 - 1)
        }
        let size = round_up_to_page_size(size);
        let ptr = unsafe {
            mmap(
                ::std::ptr::null_mut(),
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

    pub fn make_executable(&mut self) {
        if unsafe { mprotect(self.ptr as _, self.size, PROT_READ | PROT_EXEC) } != 0 {
            panic!("cannot set code memory to executable");
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
        unsafe { ::std::slice::from_raw_parts(self.ptr, self.size) }
    }
}

impl DerefMut for CodeMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { ::std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}
