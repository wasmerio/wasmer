//! Implementation of personality function and unwinding support for Wasmer.
//!
//! On a native platform, when an exception is thrown, the type info of the
//! exception is known, and can be matched against the LSDA table within the
//! personality function (e.g. __gxx_personality_v0 for Itanium ABI).
//!
//! However, in WASM, the exception "type" can change between compilation
//! and instantiation because tags can be imported from other modules. Also,
//! a single module can be instantiated many times, but all instances share
//! the same code, differing only in their VMContext data. This means that,
//! to be able to match the thrown exception against the expected tag in
//! catch clauses, we need to go through the VMContext of the specific instance
//! to which the stack frame belongs; nothing else can tell us exactly which
//! instance we're currently looking at, including the IP which will be the
//! same for all instances of the same module.
//!
//! To achieve this, we use a two-stage personality function. The first stage
//! is the normal personality function which is called by libunwind; this
//! function always catches the exception as long as it's a Wasmer exception,
//! without looking at the specific tags. Afterwards, control is transferred
//! to the module's landing pad, which can load its VMContext and pass it to
//! the second stage of the personality function. Afterwards, the second stage
//! can take the "local tag number" (the tag index as seen from the WASM
//! module's point of view) from the LSDA and translate it to the unique tag
//! within the Store, and match that against the thrown exception's tag.
//!
//! The throw function also uses the VMContext of its own instance to get the
//! unique tag from the Store, and uses that as the final exception tag.
//!
//! It's important to note that we can't count on libunwind behaving properly
//! if we make calls from the second stage of the personality function; this is
//! why the first stage has to extract all the data necessary for the second
//! stage and place it in the exception object. The second stage will clear
//! out the data before returning, so further stack frames will not get stale
//! data by mistake.

use libunwind as uw;
use wasmer_types::TagIndex;

use crate::VMContext;

use super::dwarf::eh::{self, EHAction, EHContext};

// In case where multiple copies of std exist in a single process,
// we use address of this static variable to distinguish an exception raised by
// this copy and some other copy (which needs to be treated as foreign exception).
static CANARY: u8 = 0;
const WASMER_EXCEPTION_CLASS: uw::_Unwind_Exception_Class = u64::from_ne_bytes(*b"WMERWASM");

const CATCH_ALL_TAG_VALUE: i32 = i32::MAX;
// This constant is not reflected in the generated code, but the switch block
// has a default action of rethrowing the exception, which this value should
// trigger.
const NO_MATCH_FOUND_TAG_VALUE: i32 = i32::MAX - 1;

#[repr(C)]
pub struct UwExceptionWrapper {
    pub _uwe: uw::_Unwind_Exception,
    pub canary: *const u8,
    pub cause: Box<dyn std::any::Any + Send>,

    // First stage -> second stage communication
    pub current_frame_info: Option<Box<CurrentFrameInfo>>,
}

#[repr(C)]
pub struct CurrentFrameInfo {
    pub exception_tag: u32,
    pub catch_tags: Vec<u32>,
    pub has_catch_all: bool,
}

impl UwExceptionWrapper {
    pub fn new(tag: u32, data_ptr: usize, data_size: u64) -> Self {
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
            current_frame_info: None,
        }
    }
}

#[repr(C)]
#[derive(Debug, thiserror::Error, Clone)]
#[error("Uncaught exception in wasm code!")]
pub struct WasmerException {
    // This is the store-unique tag index.
    pub tag: u32,
    pub data_ptr: usize,
    pub data_size: u64,
}

impl WasmerException {
    pub fn new(tag: u32, data_ptr: usize, data_size: u64) -> Self {
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

#[unsafe(no_mangle)]
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

        let eh_action = match find_eh_action(context) {
            Ok(action) => action,
            Err(_) => {
                return uw::_Unwind_Reason_Code__URC_FATAL_PHASE1_ERROR;
            }
        };

        if actions as i32 & uw::_Unwind_Action__UA_SEARCH_PHASE as i32 != 0 {
            match eh_action {
                EHAction::None => uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND,
                EHAction::CatchAll { .. }
                | EHAction::CatchSpecific { .. }
                | EHAction::CatchSpecificOrAll { .. } => uw::_Unwind_Reason_Code__URC_HANDLER_FOUND,
                EHAction::Terminate => uw::_Unwind_Reason_Code__URC_FATAL_PHASE1_ERROR,
            }
        } else {
            // For the catch-specific vs catch-specific-or-all case below, checked before
            // we move eh_action out in the match
            let has_catch_all = matches!(eh_action, EHAction::CatchSpecificOrAll { .. });

            match eh_action {
                EHAction::None => uw::_Unwind_Reason_Code__URC_CONTINUE_UNWIND,
                EHAction::CatchAll { lpad } => {
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.0, uw_exc as _);
                    // Zero means immediate catch-all
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.1, 0);
                    uw::_Unwind_SetIP(context, lpad as usize as _);
                    uw::_Unwind_Reason_Code__URC_INSTALL_CONTEXT
                }
                EHAction::CatchSpecific { lpad, tags }
                | EHAction::CatchSpecificOrAll { lpad, tags } => {
                    (*uw_exc).current_frame_info = Some(Box::new(CurrentFrameInfo {
                        exception_tag: wasmer_exc.tag,
                        catch_tags: tags,
                        has_catch_all,
                    }));
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.0, uw_exc as _);
                    // One means enter phase 2
                    uw::_Unwind_SetGR(context, UNWIND_DATA_REG.1, 1);
                    uw::_Unwind_SetIP(context, lpad as usize as _);
                    uw::_Unwind_Reason_Code__URC_INSTALL_CONTEXT
                }
                EHAction::Terminate => uw::_Unwind_Reason_Code__URC_FATAL_PHASE2_ERROR,
            }
        }
    }
}

#[unsafe(no_mangle)]
/// The second stage of the  personality function. See module level documentation
/// for an explanation of the exact procedure used during unwinding.
///
/// # Safety
///
/// Does pointer accesses, which must be valid.
pub unsafe extern "C" fn wasmer_eh_personality2(
    vmctx: *mut VMContext,
    exception_object: *mut UwExceptionWrapper,
) -> i32 {
    unsafe {
        let Some(current_frame_info) = (*exception_object).current_frame_info.take() else {
            // This should never happen
            unreachable!("wasmer_eh_personality2 called without current_frame_info");
        };

        let instance = (*vmctx).instance();
        for tag in current_frame_info.catch_tags {
            let unique_tag = instance.shared_tag_ptr(TagIndex::from_u32(tag)).index();
            if unique_tag == current_frame_info.exception_tag {
                return tag as i32;
            }
        }

        if current_frame_info.has_catch_all {
            CATCH_ALL_TAG_VALUE
        } else {
            NO_MATCH_FOUND_TAG_VALUE
        }
    }
}

unsafe fn find_eh_action(context: *mut uw::_Unwind_Context) -> Result<EHAction, ()> {
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
        };
        eh::find_eh_action(lsda, &eh_context)
    }
}

pub unsafe fn throw(tag: u32, vmctx: *mut VMContext, data_ptr: usize, data_size: u64) -> ! {
    unsafe {
        // Look up the unique tag from the VMContext.
        let unique_tag = (*vmctx)
            .instance()
            .shared_tag_ptr(TagIndex::from_u32(tag))
            .index();

        let exception = Box::new(UwExceptionWrapper::new(unique_tag, data_ptr, data_size));
        let exception_param = Box::into_raw(exception) as *mut libunwind::_Unwind_Exception;

        match uw::_Unwind_RaiseException(exception_param) {
            libunwind::_Unwind_Reason_Code__URC_END_OF_STACK => {
                crate::raise_lib_trap(crate::Trap::lib(wasmer_types::TrapCode::UncaughtException));
                unreachable!();
            }
            _ => {
                unreachable!()
            }
        }
    }
}

pub unsafe fn rethrow(exc: *mut UwExceptionWrapper) -> ! {
    unsafe {
        if exc.is_null() {
            panic!();
        }

        match uw::_Unwind_Resume_or_Rethrow(std::mem::transmute::<
            *mut UwExceptionWrapper,
            *mut libunwind::_Unwind_Exception,
        >(exc))
        {
            libunwind::_Unwind_Reason_Code__URC_END_OF_STACK => {
                crate::raise_lib_trap(crate::Trap::lib(wasmer_types::TrapCode::UncaughtException));
                unreachable!()
            }
            _ => unreachable!(),
        }
    }
}
