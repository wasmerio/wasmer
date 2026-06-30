use std::ffi::CStr;
use std::{
    ffi::CString,
    os::raw::{c_char, c_int},
};

#[repr(C)]
struct LldInvokeResult {
    success: bool,
    messages: *const c_char,
}

unsafe extern "C" {
    fn lld_link(argc: c_int, argv: *const *const c_char) -> LldInvokeResult;
    fn link_free_result(result: *mut LldInvokeResult);
}

pub enum LldError {
    StringConversionError,
}

#[derive(Debug)]
pub struct LldResult {
    success: bool,
    messages: String,
}

impl LldResult {
    pub fn ok(self) -> Result<(), String> {
        if self.success {
            Ok(())
        } else {
            Err(self.messages)
        }
    }

    pub fn debug_print(&self) {
        println!("Result from invocation: {}", self.success);
        println!("Attached message(s): {}", self.messages);
    }
}

pub fn link_native(args: Vec<String>) -> LldResult {
    // Prepare arguments
    let c_args = args
        .iter()
        .map(|arg| CString::new(arg.as_bytes()).unwrap())
        .collect::<Vec<CString>>();
    let args: Vec<*const c_char> = c_args.iter().map(|arg| arg.as_ptr()).collect();

    let mut lld_result = unsafe { lld_link(args.len() as c_int, args.as_ptr()) };

    let messages = if !lld_result.messages.is_null() {
        unsafe {
            CStr::from_ptr(lld_result.messages)
                .to_string_lossy()
                .to_string()
        }
    } else {
        String::new()
    };

    let result = LldResult {
        success: lld_result.success,
        messages,
    };

    unsafe { link_free_result(&mut lld_result as *mut LldInvokeResult) };
    drop(lld_result);

    result
}

#[cfg(test)]
mod tests {
    use super::link_native;

    #[test]
    fn test_via_version() {
        let res = link_native(vec!["--version".to_string()]);
        res.debug_print();
    }
}
