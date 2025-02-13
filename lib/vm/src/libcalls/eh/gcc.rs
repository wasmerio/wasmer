use libunwind as uw;

use super::dwarf::eh::{self, EHAction, EHContext};

// In case where multiple copies of std exist in a single process,
// we use address of this static variable to distinguish an exception raised by
// this copy and some other copy (which needs to be treated as foreign exception).
static CANARY: u8 = 0;
const WASMER_EXCEPTION_CLASS: uw::_Unwind_Exception_Class = u64::from_ne_bytes(*b"WMERWASM");

#[repr(C)]
pub struct UwExceptionWrapper {
    pub _uwe: uw::_Unwind_Exception,
    pub canary: *const u8,
    pub cause: Box<dyn std::any::Any + Send>,
}

impl UwExceptionWrapper {
    pub fn new(tag: u64, data_ptr: usize, data_size: u64) -> Self {
        Self {
            _uwe: uw::_Unwind_Exception {
                exception_class: WASMER_EXCEPTION_CLASS,
                exception_cleanup: None,
                private_1: core::ptr::null::<u8>() as usize as _,
                private_2: 0,
            },
            canary: &CANARY,
            cause: Box::new(WasmerException {
                tag,
                data_ptr,
                data_size,
            }),
        }
    }
}

#[repr(C)]
#[derive(Debug, thiserror::Error, Clone)]
#[error("Uncaught exception in wasm code!")]
pub struct WasmerException {
    pub tag: u64,
    pub data_ptr: usize,
    pub data_size: u64,
}

impl WasmerException {
    pub fn new(tag: u64, data_ptr: usize, data_size: u64) -> Self {
        Self {
            tag,
            data_ptr,
            data_size,
        }
    }
}

#[cfg(target_arch = "x86_64")]
const UNWIND_DATA_REG: (i32, i32) = (0, 1); // RAX, RDX

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const UNWIND_DATA_REG: (i32, i32) = (0, 1); // R0, R1 / X0, X1

#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
const UNWIND_DATA_REG: (i32, i32) = (10, 11); // x10, x11

#[cfg(target_arch = "loongarch64")]
const UNWIND_DATA_REG: (i32, i32) = (4, 5); // a0, a1

#[no_mangle]
/// The implementation of Wasmer's personality function.
///
/// # Safety
///
/// Performs libunwind unwinding magic.
pub unsafe extern "C" fn wasmer_eh_personality(
    version: std::ffi::c_int,
    actions: uw::_Unwind_Action,
    exception_class: uw::_Unwind_Exception_Class,
    exception_object: *mut uw::_Unwind_Exception,
    context: *mut uw::_Unwind_Context,
) -> uw::_Unwind_Reason_Code {
    unsafe {
        if version != 1 {
            return uw::_Unwind_Reason_Code__URC_FATAL_PHASE1_ERROR;
        }

        let uw_exc = std::mem::transmute::<*mut uw::_Unwind_Exception, *mut UwExceptionWrapper>(
            exception_object,
        );

        if exception_class != WASMER_EXCEPTION_CLASS {
            return uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND;
        }

        let wasmer_exc = (*uw_exc).cause.downcast_ref::<WasmerException>();
        let wasmer_exc = match wasmer_exc {
            Some(e) => e,
            None => {
                return uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND;
            }
        };

        let eh_action = match find_eh_action(context, wasmer_exc.tag) {
            Ok(action) => action,
            Err(_) => {
                return uw::_Unwind_Reason_Code__URC_FATAL_PHASE1_ERROR;
            }
        };

        if actions as i32 & uw::_Unwind_Action__UA_SEARCH_PHASE as i32 != 0 {
            match eh_action {
                EHAction::None | EHAction::Cleanup(_) => {
                    uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND
                }
                EHAction::Catch { .. } | EHAction::Filter { .. } => {
                    uw::_Unwind_Reason_Code__URC_HANDLER_FOUND
                }
                EHAction::Terminate => uw::_Unwind_Reason_Code__URC_FATAL_PHASE1_ERROR,
            }
        } else {
            match eh_action {
                EHAction::None => uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND,
                // Forced unwinding hits a terminate action.
                EHAction::Filter { .. }
                    if actions as i32 & uw::_Unwind_Action__UA_FORCE_UNWIND as i32 != 0 =>
                {
                    uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND
                }
                EHAction::Cleanup(lpad) => {
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.0, uw_exc as _);
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.1, 0);
                    uw::_Unwind_SetIP(context, lpad as usize as _);
                    uw::_Unwind_Reason_Code__URC_INSTALL_CONTEXT
                }
                EHAction::Catch { lpad, tag } | EHAction::Filter { lpad, tag } => {
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.0, uw_exc as _);
                    #[allow(trivial_numeric_casts)]
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.1, tag as _);
                    uw::_Unwind_SetIP(context, lpad as usize as _);
                    uw::_Unwind_Reason_Code__URC_INSTALL_CONTEXT
                }
                EHAction::Terminate => uw::_Unwind_Reason_Code__URC_FATAL_PHASE2_ERROR,
            }
        }
    }
}

unsafe fn find_eh_action(context: *mut uw::_Unwind_Context, tag: u64) -> Result<EHAction, ()> {
    unsafe {
        let lsda = uw::_Unwind_GetLanguageSpecificData(context) as *const u8;
        let mut ip_before_instr: std::ffi::c_int = 0;
        let ip = uw::_Unwind_GetIPInfo(context, &mut ip_before_instr);
        let eh_context = EHContext {
            // The return address points 1 byte past the call instruction,
            // which could be in the next IP range in LSDA range table.
            //
            // `ip = -1` has special meaning, so use wrapping sub to allow for that
            ip: if ip_before_instr != 0 {
                ip as _
            } else {
                ip.wrapping_sub(1) as _
            },
            func_start: uw::_Unwind_GetRegionStart(context) as *const _,
            get_text_start: &|| uw::_Unwind_GetTextRelBase(context) as *const _,
            get_data_start: &|| uw::_Unwind_GetDataRelBase(context) as *const _,
            tag,
        };
        eh::find_eh_action(lsda, &eh_context)
    }
}

pub unsafe fn throw(tag: u64, data_ptr: usize, data_size: u64) -> ! {
    let exception = Box::new(UwExceptionWrapper::new(tag, data_ptr, data_size));
    let exception_param = Box::into_raw(exception) as *mut libunwind::_Unwind_Exception;

    match uw::_Unwind_RaiseException(exception_param) {
        libunwind::_Unwind_Reason_Code__URC_END_OF_STACK => {
            crate::raise_lib_trap(crate::Trap::lib(wasmer_types::TrapCode::UncaughtException))
        }
        _ => {
            unreachable!()
        }
    }
}

pub unsafe fn rethrow(exc: *mut UwExceptionWrapper) -> ! {
    if exc.is_null() {
        panic!();
    }

    match uw::_Unwind_Resume_or_Rethrow(std::mem::transmute::<
        *mut UwExceptionWrapper,
        *mut libunwind::_Unwind_Exception,
    >(exc))
    {
        libunwind::_Unwind_Reason_Code__URC_END_OF_STACK => {
            crate::raise_lib_trap(crate::Trap::lib(wasmer_types::TrapCode::UncaughtException))
        }
        _ => unreachable!(),
    }
}
