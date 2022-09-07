use wasmer_derive::ValueType;
use wasmer_types::MemorySize;
use wasmer_wasi_types_generated::wasi::{
    BusDataFormat, BusEventType, 
    BusEventExit, BusEventFault,
    BusEventClose, Cid, OptionCid,
};

// Not sure how to port these types to .wit with generics ...

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_call_t<M: MemorySize> {
    pub parent: OptionCid,
    pub cid: Cid,
    pub format: BusDataFormat,
    pub topic_ptr: M::Offset,
    pub topic_len: M::Offset,
    pub buf_ptr: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_busevent_u<M: MemorySize> {
    pub noop: u8,
    pub exit: BusEventExit,
    pub call: __wasi_busevent_call_t<M>,
    pub result: __wasi_busevent_result_t<M>,
    pub fault: BusEventFault,
    pub close: BusEventClose,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_busevent_result_t<M: MemorySize> {
    pub format: BusDataFormat,
    pub cid: Cid,
    pub buf_ptr: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_busevent_t<M: MemorySize> {
    pub tag: BusEventType,
    pub u: __wasi_busevent_u<M>,
}
