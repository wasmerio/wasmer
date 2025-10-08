mod frame_info;
mod stack;
pub use frame_info::{
    CompiledFunctionFrameInfoVariant, FRAME_INFO, FrameInfosVariant, FunctionExtent,
    GlobalFrameInfoRegistration, register as register_frame_info,
};
pub use stack::get_trace_and_trapcode;
