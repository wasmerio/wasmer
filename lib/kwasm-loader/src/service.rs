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
    LoadCode = 0x1001,
    RunCode = 0x1002,
}

#[derive(Debug)]
pub enum ServiceError {
    Io(io::Error),
    Code(i32),
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
struct LoadCodeRequest {
    code: *const u8,
    code_len: u32,
    memory: *const u8,
    memory_len: u32,
    memory_max: u32,
    table: *const TableEntryRequest,
    table_count: u32,
    globals: *const u64,
    global_count: u32,

    imported_funcs: *const ImportRequest,
    imported_func_count: u32,

    dynamic_sigindices: *const u32,
    dynamic_sigindice_count: u32,
}

#[repr(C)]
struct RunCodeRequest {
    entry_offset: u32,
    params: *const u64,
    param_count: u32,
}

#[repr(C)]
struct ImportRequest {
    name: [u8; 64],
}

#[repr(C)]
pub struct TableEntryRequest {
    pub offset: usize,
    pub sig_id: u32,
}

pub struct LoadProfile<'a> {
    pub code: &'a [u8],
    pub memory: Option<&'a [u8]>,
    pub memory_max: usize,
    pub globals: &'a [u64],
    pub imports: &'a [String],
    pub dynamic_sigindices: &'a [u32],
    pub table: Option<&'a [TableEntryRequest]>,
}

pub struct RunProfile<'a> {
    pub entry_offset: u32,
    pub params: &'a [u64],
}

pub struct ServiceContext {
    dev: File
}

impl ServiceContext {
    pub fn new(load: LoadProfile) -> ServiceResult<ServiceContext> {
        let dev = File::open("/dev/wasmctl")?;
        let imports: Vec<ImportRequest> = load.imports.iter().map(|x| {
            let mut req: ImportRequest = unsafe { ::std::mem::zeroed() };
            let x = x.as_bytes();
            let mut count = req.name.len() - 1;
            if x.len() < count {
                count = x.len();
            }
            req.name[..count].copy_from_slice(&x[..count]);
            req
        }).collect();
        let req = LoadCodeRequest {
            code: load.code.as_ptr(),
            code_len: load.code.len() as u32,
            memory: load.memory.map(|x| x.as_ptr()).unwrap_or(::std::ptr::null()),
            memory_len: load.memory.map(|x| x.len() as u32).unwrap_or(0),
            memory_max: load.memory_max as u32,
            table: load.table.map(|x| x.as_ptr()).unwrap_or(::std::ptr::null()),
            table_count: load.table.map(|x| x.len() as u32).unwrap_or(0),
            globals: load.globals.as_ptr(),
            global_count: load.globals.len() as u32,
            imported_funcs: imports.as_ptr(),
            imported_func_count: imports.len() as u32,
            dynamic_sigindices: load.dynamic_sigindices.as_ptr(),
            dynamic_sigindice_count: load.dynamic_sigindices.len() as u32,
        };
        let fd = dev.as_raw_fd();
        let ret = unsafe {
            ::libc::ioctl(
                fd,
                Command::LoadCode as i32 as ::libc::c_ulong,
                &req as *const _ as ::libc::c_ulong
            )
        };
        if ret != 0 {
            Err(ServiceError::Code(ret))
        } else {
            Ok(ServiceContext {
                dev: dev,
            })
        }
    }

    pub fn run_code(&mut self, run: RunProfile) -> ServiceResult<i32> {
        let req = RunCodeRequest {
            entry_offset: run.entry_offset,
            params: run.params.as_ptr(),
            param_count: run.params.len() as u32,
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
