mod error;
mod frame_info;
pub use error::RuntimeError;
pub use frame_info::{
    register, ExtraFunctionInfo, UnprocessedFunctionFrameInfo, FrameInfo,
    GlobalFrameInfoRegistration, FRAME_INFO,
};
