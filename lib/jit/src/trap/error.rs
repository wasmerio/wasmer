use super::frame_info::{FrameInfo, GlobalFrameInfo, FRAME_INFO};
use backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use wasmer_runtime::{raise_user_trap, Trap, TrapCode};

/// A struct representing an aborted instruction execution, with a message
/// indicating the cause.
#[derive(Clone)]
pub struct RuntimeError {
    inner: Arc<RuntimeErrorInner>,
}

struct RuntimeErrorInner {
    message: String,
    wasm_trace: Vec<FrameInfo>,
    native_trace: Backtrace,
}

fn _assert_trap_is_sync_and_send(t: &Trap) -> (&dyn Sync, &dyn Send) {
    (t, t)
}

impl RuntimeError {
    /// Creates a new `Trap` with `message`.
    /// # Example
    /// ```
    /// let trap = wasmer_jit::RuntimeError::new("unexpected error");
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new<I: Into<String>>(message: I) -> Self {
        let info = FRAME_INFO.read().unwrap();
        Self::new_with_trace(&info, None, message.into(), Backtrace::new_unresolved())
    }

    /// Create a new RuntimeError from a Trap.
    pub fn from_trap(trap: Trap) -> Self {
        let info = FRAME_INFO.read().unwrap();
        match jit {
            Trap::User(error) => {
                // Since we're the only one using the internals (in
                // theory) we should only see user errors which were originally
                // created from our own `Trap` type (see the trampoline module
                // with functions).
                // Self::new(format!("{}", error))
                *error.downcast().expect("only `Trap` errors are supported")
            }
            Trap::Jit { pc, backtrace } => {
                let code = info
                    .lookup_trap_info(pc)
                    .map_or(TrapCode::StackOverflow, |info| info.trap_code);
                Self::new_wasm(&info, Some(pc), code, backtrace)
            }
            Trap::Wasm {
                trap_code,
                backtrace,
            } => Self::new_wasm(&info, None, trap_code, backtrace),
            Trap::OOM { backtrace } => {
                Self::new_with_trace(&info, None, "out of memory".to_string(), backtrace)
            }
        }
    }

    /// Raises a custom user Error
    pub fn raise(error: Box<dyn Error + Send + Sync>) -> ! {
        unsafe { raise_user_trap(error) }
    }

    fn new_wasm(
        info: &GlobalFrameInfo,
        trap_pc: Option<usize>,
        code: TrapCode,
        backtrace: Backtrace,
    ) -> Self {
        let desc = match code {
            TrapCode::StackOverflow => "call stack exhausted",
            TrapCode::HeapSetterOutOfBounds => "memory out of bounds: data segment does not fit",
            TrapCode::HeapAccessOutOfBounds => "out of bounds memory access",
            TrapCode::TableSetterOutOfBounds => {
                "table out of bounds: elements segment does not fit"
            }
            TrapCode::TableAccessOutOfBounds => "undefined element: out of bounds table access",
            TrapCode::OutOfBounds => "out of bounds",
            TrapCode::IndirectCallToNull => "uninitialized element",
            TrapCode::BadSignature => "indirect call type mismatch",
            TrapCode::IntegerOverflow => "integer overflow",
            TrapCode::IntegerDivisionByZero => "integer divide by zero",
            TrapCode::BadConversionToInteger => "invalid conversion to integer",
            TrapCode::UnreachableCodeReached => "unreachable",
            TrapCode::Interrupt => "interrupt",
            TrapCode::User(_) => unreachable!(),
        };
        let msg = format!("{}", desc);
        Self::new_with_trace(info, trap_pc, msg, backtrace)
    }

    fn new_with_trace(
        info: &GlobalFrameInfo,
        trap_pc: Option<usize>,
        message: String,
        native_trace: Backtrace,
    ) -> Self {
        let mut wasm_trace = Vec::new();
        for frame in native_trace.frames() {
            let pc = frame.ip() as usize;
            if pc == 0 {
                continue;
            }
            // Note that we need to be careful about the pc we pass in here to
            // lookup frame information. This program counter is used to
            // translate back to an original source location in the origin wasm
            // module. If this pc is the exact pc that the trap happened at,
            // then we look up that pc precisely. Otherwise backtrace
            // information typically points at the pc *after* the call
            // instruction (because otherwise it's likely a call instruction on
            // the stack). In that case we want to lookup information for the
            // previous instruction (the call instruction) so we subtract one as
            // the lookup.
            let pc_to_lookup = if Some(pc) == trap_pc { pc } else { pc - 1 };
            if let Some(info) = info.lookup_frame_info(pc_to_lookup) {
                wasm_trace.push(info);
            }
        }
        Self {
            inner: Arc::new(RuntimeErrorInner {
                message,
                wasm_trace,
                native_trace,
            }),
        }
    }

    /// Returns a reference the `message` stored in `Trap`.
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// Returns a list of function frames in WebAssembly code that led to this
    /// trap happening.
    pub fn trace(&self) -> &[FrameInfo] {
        &self.inner.wasm_trace
    }
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeError")
            .field("message", &self.inner.message)
            .field("wasm_trace", &self.inner.wasm_trace)
            .field("native_trace", &self.inner.native_trace)
            .finish()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RuntimeError: {}", self.inner.message)?;
        let trace = self.trace();
        if trace.is_empty() {
            return Ok(());
        }
        for frame in self.trace().iter() {
            let name = frame.module_name();
            let func_index = frame.func_index();
            writeln!(f, "")?;
            write!(f, "    at ")?;
            match frame.func_name() {
                Some(name) => match rustc_demangle::try_demangle(name) {
                    Ok(name) => write!(f, "{}", name)?,
                    Err(_) => write!(f, "{}", name)?,
                },
                None => write!(f, "<unnamed>")?,
            }
            write!(
                f,
                " ({}[{}]:0x{:x})",
                name,
                func_index,
                frame.module_offset()
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

impl From<Trap> for RuntimeError {
    fn from(trap: Trap) -> Self {
        Self::from_trap(trap)
    }
}
