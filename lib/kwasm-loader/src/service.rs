use std::error::Error;
use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;

macro_rules! impl_debug_display {
    ($target:ident) => {
        impl ::std::fmt::Display for $target {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                <Self as ::std::fmt::Debug>::fmt(self, f)
            }
        }
    };
}

#[repr(i32)]
pub enum Command {
    LoadCode = 0x1001,
    RunCode = 0x1002,
    ReadMemory = 0x1003,
    WriteMemory = 0x1004,
}

#[derive(Debug)]
pub enum ServiceError {
    Io(io::Error),
    Code(i32),
    InvalidInput,
    Rejected,
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
    result: *mut RunCodeResult,
}

#[repr(C)]
struct RunCodeResult {
    success: u32,
    retval: u64,
}

#[repr(C)]
struct ReadMemoryRequest {
    out: *mut u8,
    offset: u32,
    len: u32,
}

#[repr(C)]
struct WriteMemoryRequest {
    _in: *const u8,
    offset: u32,
    len: u32,
}

#[repr(C)]
struct ImportRequest {
    name: [u8; 64],
    param_count: u32,
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
    pub imports: &'a [ImportInfo],
    pub dynamic_sigindices: &'a [u32],
    pub table: Option<&'a [TableEntryRequest]>,
}

pub struct ImportInfo {
    pub name: String,
    pub param_count: usize,
}

pub struct RunProfile<'a> {
    pub entry_offset: u32,
    pub params: &'a [u64],
}

pub struct ServiceContext {
    dev: File,
}

impl ServiceContext {
    pub fn new(load: LoadProfile) -> ServiceResult<ServiceContext> {
        let dev = File::open("/dev/wasmctl")?;
        let imports: Vec<ImportRequest> = load
            .imports
            .iter()
            .map(|x| {
                let mut req = ImportRequest {
                    name: [0u8; 64],
                    param_count: x.param_count as u32,
                };
                let name = x.name.as_bytes();
                let mut count = req.name.len() - 1;
                if name.len() < count {
                    count = name.len();
                }
                req.name[..count].copy_from_slice(&name[..count]);
                req
            })
            .collect();
        let req = LoadCodeRequest {
            code: load.code.as_ptr(),
            code_len: load.code.len() as u32,
            memory: load
                .memory
                .map(|x| x.as_ptr())
                .unwrap_or(::std::ptr::null()),
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
                &req as *const _ as ::libc::c_ulong,
            )
        };
        if ret != 0 {
            Err(ServiceError::Code(ret))
        } else {
            Ok(ServiceContext { dev: dev })
        }
    }

    pub fn run_code(&mut self, run: RunProfile) -> ServiceResult<u64> {
        let mut result: RunCodeResult = unsafe { ::std::mem::zeroed() };
        let mut req = RunCodeRequest {
            entry_offset: run.entry_offset,
            params: run.params.as_ptr(),
            param_count: run.params.len() as u32,
            result: &mut result,
        };
        let fd = self.dev.as_raw_fd();
        let err = unsafe {
            ::libc::ioctl(
                fd,
                Command::RunCode as i32 as ::libc::c_ulong,
                &mut req as *mut _ as ::libc::c_ulong,
            )
        };
        if err < 0 {
            Err(ServiceError::Code(err))
        } else if result.success == 0 {
            println!("Rejected {} {}", result.success, result.retval);
            Err(ServiceError::Rejected)
        } else {
            Ok(result.retval)
        }
    }

    pub fn read_memory(&mut self, offset: u32, len: u32) -> ServiceResult<Vec<u8>> {
        let fd = self.dev.as_raw_fd();
        let mut ret = Vec::with_capacity(len as usize);
        unsafe {
            ret.set_len(len as usize);
        }
        let req = ReadMemoryRequest {
            out: ret.as_mut_ptr(),
            offset: offset,
            len: len,
        };
        let err = unsafe {
            ::libc::ioctl(
                fd,
                Command::ReadMemory as i32 as ::libc::c_ulong,
                &req as *const _ as ::libc::c_ulong,
            )
        };
        if err < 0 {
            Err(ServiceError::Code(err))
        } else {
            Ok(ret)
        }
    }

    pub fn write_memory(&mut self, offset: u32, len: u32, buf: &[u8]) -> ServiceResult<()> {
        let fd = self.dev.as_raw_fd();
        let req = WriteMemoryRequest {
            _in: buf.as_ptr(),
            offset: offset,
            len: len,
        };
        let err = unsafe {
            ::libc::ioctl(
                fd,
                Command::WriteMemory as i32 as ::libc::c_ulong,
                &req as *const _ as ::libc::c_ulong,
            )
        };
        if err < 0 {
            Err(ServiceError::Code(err))
        } else {
            Ok(())
        }
    }
}
