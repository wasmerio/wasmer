use super::*;
use wasmer_derive::ValueType;
use wasmer_types::MemorySize;

pub type __wasi_busdatatype_t = u8;
pub const __WASI_BUS_DATA_TYPE_CALL: __wasi_busdatatype_t = 0;
pub const __WASI_BUS_DATA_TYPE_CALLBACK: __wasi_busdatatype_t = 1;
pub const __WASI_BUS_DATA_TYPE_REPLY: __wasi_busdatatype_t = 2;

pub type __wasi_busdataformat_t = u8;
pub const __WASI_BUS_DATA_FORMAT_RAW: __wasi_busdataformat_t = 0;
pub const __WASI_BUS_DATA_FORMAT_BINCODE: __wasi_busdataformat_t = 1;
pub const __WASI_BUS_DATA_FORMAT_MESSAGE_PACK: __wasi_busdataformat_t = 2;
pub const __WASI_BUS_DATA_FORMAT_JSON: __wasi_busdataformat_t = 3;
pub const __WASI_BUS_DATA_FORMAT_YAML: __wasi_busdataformat_t = 4;
pub const __WASI_BUS_DATA_FORMAT_XML: __wasi_busdataformat_t = 5;

pub type __wasi_buseventtype_t = u8;
pub const __WASI_BUS_EVENT_TYPE_EXIT: __wasi_buseventtype_t = 0;
pub const __WASI_BUS_EVENT_TYPE_DATA: __wasi_buseventtype_t = 1;
pub const __WASI_BUS_EVENT_TYPE_FAULT: __wasi_buseventtype_t = 2;

pub type __wasi_bid_t = u32;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_bid_t {
    pub tag: __wasi_option_t,
    pub bid: __wasi_bid_t,
}

pub type __wasi_cid_t = u8;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_cid_t {
    pub tag: __wasi_option_t,
    pub cid: __wasi_cid_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_bus_handles_t {
    pub handle: __wasi_bid_t,
    pub stdin: __wasi_fd_t,
    pub stdout: __wasi_fd_t,
    pub stderr: __wasi_fd_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_exit_t {
    pub rval: __wasi_exitcode_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_data_t<M: MemorySize> {
    pub ty: __wasi_busdatatype_t,
    pub format: __wasi_busdataformat_t,
    pub cid: __wasi_cid_t,
    pub topic_len: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_fault_t {
    pub cid: __wasi_cid_t,
    pub err: __bus_errno_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_busevent_u<M: MemorySize> {
    pub exit: __wasi_busevent_exit_t,
    pub data: __wasi_busevent_data_t<M>,
    pub fault: __wasi_busevent_fault_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_busevent_t<M: MemorySize> {
    pub tag: __wasi_buseventtype_t,
    pub u: __wasi_busevent_u<M>,
}
