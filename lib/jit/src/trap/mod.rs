mod error;
mod frame_info;
pub use error::RuntimeError;
pub use frame_info::{
    register, ExtraFunctionInfo, ExtraFunctionInfoUnprocessed, FrameInfo,
    GlobalFrameInfoRegistration, FRAME_INFO,
};
