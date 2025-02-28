#[cfg(feature = "sys")]
pub mod sys;
#[cfg(feature = "sys")]
pub use sys::*;

use super::wasmer_engine_t;

#[cfg(feature = "jsc")]
pub mod jsc;

#[cfg(feature = "v8")]
pub mod v8;

#[cfg(feature = "wasmi")]
pub mod wasmi;

#[cfg(feature = "wamr")]
pub mod wamr;

#[repr(C)]
#[derive(Debug)]
pub(crate) enum wasmer_engine_config_t {
    #[cfg(feature = "sys")]
    Sys(sys::wasmer_sys_engine_config_t),

    #[cfg(feature = "jsc")]
    Jsc(jsc::wasmer_jsc_engine_config_t),

    #[cfg(feature = "v8")]
    V8(v8::wasmer_v8_engine_config_t),

    #[cfg(feature = "wasmi")]
    Wasmi(wasmi::wasmer_wasmi_engine_config_t),

    #[cfg(feature = "wamr")]
    Wamr(wamr::wasmer_wamr_engine_config_t),
}

impl Default for wasmer_engine_config_t {
    fn default() -> Self {
        match wasmer_engine_t::default() {
            #[cfg(feature = "sys")]
            wasmer_engine_t::UNIVERSAL => Self::Sys(sys::wasmer_sys_engine_config_t::default()),
            #[cfg(feature = "v8")]
            wasmer_engine_t::V8 => Self::V8(v8::wasmer_v8_engine_config_t::default()),
            #[cfg(feature = "wasmi")]
            wasmer_engine_t::WASMI => Self::Wasmi(wasmi::wasmer_wasmi_engine_config_t::default()),
            #[cfg(feature = "wamr")]
            wasmer_engine_t::WAMR => Self::Wamr(wamr::wasmer_wamr_engine_config_t::default()),
            #[cfg(feature = "jsc")]
            wasmer_engine_t::JSC => Self::Jsc(jsc::wasmer_jsc_engine_config_t::default()),
        }
    }
}
