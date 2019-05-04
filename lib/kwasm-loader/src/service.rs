use std::fs::File;
use std::io;
use std::error::Error;
use std::os::unix::io::AsRawFd;

macro_rules! impl_debug_display {
    ($target:ident) => {
        impl ::std::fmt::Display for $target {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                <Self as ::std::fmt::Debug>::fmt(self, f)
            }
        }
    }
}

#[repr(i32)]
pub enum Command {
    RunCode = 0x1001,
}

#[derive(Debug)]
pub enum ServiceError {
    Io(io::Error),
    InvalidInput,
    Rejected
}

pub type ServiceResult<T> = Result<T, ServiceError>;

impl_debug_display!(ServiceError);

impl Error for ServiceError {
    fn description(&self) -> &str {
        "ServiceError"
    }
}

impl From<io::Error> for ServiceError {
    fn from(other: io::Error) -> ServiceError {
        ServiceError::Io(other)
    }
}

#[repr(C)]
struct RunCodeRequest {
    code: *const u8,
    code_len: u32,
    memory: *const u8,
    memory_len: u32,
    memory_max: u32,
    table: *const u32,
    table_count: u32,
    globals: *const u64,
    global_count: u32,

    imported_funcs: *const ImportRequest,
    imported_func_count: u32,

    entry_offset: u32,
    params: *const u64,
    param_count: u32,
}

#[repr(C)]
struct ImportRequest {
    name: [u8; 64],
}

pub struct RunProfile<'a> {
    pub code: &'a [u8],
    pub memory: Option<&'a [u8]>,
    pub memory_max: usize,
    pub globals: &'a [u64],
    pub params: &'a [u64],
    pub entry_offset: u32,
    pub imports: &'a [String]
}

pub struct ServiceContext {
    dev: File
}

impl ServiceContext {
    pub fn connect() -> ServiceResult<ServiceContext> {
        Ok(ServiceContext {
            dev: File::open("/dev/wasmctl")?
        })
    }

    pub fn run_code(&mut self, run: RunProfile) -> ServiceResult<i32> {
        let imports: Vec<ImportRequest> = run.imports.iter().map(|x| {
            let mut req: ImportRequest = unsafe { ::std::mem::zeroed() };
            let x = x.as_bytes();
            let mut count = req.name.len() - 1;
            if x.len() < count {
                count = x.len();
            }
            req.name[..count].copy_from_slice(&x[..count]);
            req
        }).collect();
        let req = RunCodeRequest {
            code: run.code.as_ptr(),
            code_len: run.code.len() as u32,
            memory: run.memory.map(|x| x.as_ptr()).unwrap_or(::std::ptr::null()),
            memory_len: run.memory.map(|x| x.len() as u32).unwrap_or(0),
            memory_max: run.memory_max as u32,
            table: ::std::ptr::null(),
            table_count: 0,
            globals: run.globals.as_ptr(),
            global_count: run.globals.len() as u32,
            imported_funcs: imports.as_ptr(),
            imported_func_count: imports.len() as u32,
            params: run.params.as_ptr(),
            param_count: run.params.len() as u32,
            entry_offset: run.entry_offset,
        };
        let fd = self.dev.as_raw_fd();
        let ret = unsafe {
            ::libc::ioctl(
                fd,
                Command::RunCode as i32 as ::libc::c_ulong,
                &req as *const _ as ::libc::c_ulong
            )
        };
        Ok(ret)
    }
}
