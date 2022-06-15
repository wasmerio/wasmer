use super::*;
use wasmer_derive::ValueType;
use wasmer_types::MemorySize;

pub type __wasi_busdataformat_t = u8;
pub const __WASI_BUS_DATA_FORMAT_RAW: __wasi_busdataformat_t = 0;
pub const __WASI_BUS_DATA_FORMAT_BINCODE: __wasi_busdataformat_t = 1;
pub const __WASI_BUS_DATA_FORMAT_MESSAGE_PACK: __wasi_busdataformat_t = 2;
pub const __WASI_BUS_DATA_FORMAT_JSON: __wasi_busdataformat_t = 3;
pub const __WASI_BUS_DATA_FORMAT_YAML: __wasi_busdataformat_t = 4;
pub const __WASI_BUS_DATA_FORMAT_XML: __wasi_busdataformat_t = 5;
pub const __WASI_BUS_DATA_FORMAT_RKYV: __wasi_busdataformat_t = 6;

pub type __wasi_buseventtype_t = u8;
pub const __WASI_BUS_EVENT_TYPE_NOOP: __wasi_buseventtype_t = 0;
pub const __WASI_BUS_EVENT_TYPE_EXIT: __wasi_buseventtype_t = 1;
pub const __WASI_BUS_EVENT_TYPE_CALL: __wasi_buseventtype_t = 2;
pub const __WASI_BUS_EVENT_TYPE_RESULT: __wasi_buseventtype_t = 3;
pub const __WASI_BUS_EVENT_TYPE_FAULT: __wasi_buseventtype_t = 4;
pub const __WASI_BUS_EVENT_TYPE_CLOSE: __wasi_buseventtype_t = 5;

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
pub struct __wasi_option_fd_t {
    pub tag: __wasi_option_t,
    pub fd: __wasi_fd_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_cid_t {
    pub tag: __wasi_option_t,
    pub cid: __wasi_cid_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_bus_handles_t {
    pub bid: __wasi_bid_t,
    pub stdin: __wasi_option_fd_t,
    pub stdout: __wasi_option_fd_t,
    pub stderr: __wasi_option_fd_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_exit_t {
    pub bid: __wasi_bid_t,
    pub rval: __wasi_exitcode_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_call_t<M: MemorySize> {
    pub parent: __wasi_option_cid_t,
    pub cid: __wasi_cid_t,
    pub format: __wasi_busdataformat_t,
    pub topic_ptr: M::Offset,
    pub topic_len: M::Offset,
    pub buf_ptr: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_result_t<M: MemorySize> {
    pub format: __wasi_busdataformat_t,
    pub cid: __wasi_cid_t,
    pub buf_ptr: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_fault_t {
    pub cid: __wasi_cid_t,
    pub err: __bus_errno_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_close_t {
    pub cid: __wasi_cid_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_busevent_u<M: MemorySize> {
    pub noop: u8,
    pub exit: __wasi_busevent_exit_t,
    pub call: __wasi_busevent_call_t<M>,
    pub result: __wasi_busevent_result_t<M>,
    pub fault: __wasi_busevent_fault_t,
    pub close: __wasi_busevent_close_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_busevent_t<M: MemorySize> {
    pub tag: __wasi_buseventtype_t,
    pub u: __wasi_busevent_u<M>,
}
