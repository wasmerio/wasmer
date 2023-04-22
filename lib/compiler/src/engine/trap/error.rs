use super::frame_info::{GlobalFrameInfo, FRAME_INFO};
use backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use wasmer_types::{FrameInfo, TrapCode};
use wasmer_vm::Trap;

/// A struct representing an aborted instruction execution, with a message
/// indicating the cause.
#[derive(Clone)]
pub struct RuntimeError {
    inner: Arc<RuntimeErrorInner>,
}

#[derive(Debug)]
struct RuntimeStringError {
    details: String,
}

impl RuntimeStringError {
    fn new(msg: String) -> RuntimeStringError {
        RuntimeStringError { details: msg }
    }
}

impl fmt::Display for RuntimeStringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for RuntimeStringError {
    fn description(&self) -> &str {
        &self.details
    }
}

struct RuntimeErrorInner {
    /// The source error
    source: Trap,
    /// The trap code (if any)
    trap_code: Option<TrapCode>,
    /// The reconstructed Wasm trace (from the native trace and the `GlobalFrameInfo`).
    wasm_trace: Vec<FrameInfo>,
}

fn _assert_trap_is_sync_and_send(t: &Trap) -> (&dyn Sync, &dyn Send) {
    (t, t)
}

impl RuntimeError {
    /// Creates a new generic `RuntimeError` with the given `message`.
    ///
    /// # Example
    /// ```
    /// let trap = wasmer_compiler::RuntimeError::new("unexpected error");
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new<I: Into<String>>(message: I) -> Self {
        let msg = message.into();
        let source = RuntimeStringError::new(msg);
        Self::user(Box::new(source))
    }

    /// Creates `RuntimeError` from an error and a WasmTrace
    ///
    /// # Example
    /// ```ignore
    /// let wasm_trace = vec![wasmer_types::FrameInfo::new(
    ///   "my_module".to_string(),
    ///   0,
    ///   Some("my_function".to_string()),
    ///   0.into(),
    ///   2.into()
    /// )];
    /// let trap = wasmer_compiler::RuntimeError::new(my_error, wasm_trace);
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new_from_source(
        source: Trap,
        wasm_trace: Vec<FrameInfo>,
        trap_code: Option<TrapCode>,
    ) -> Self {
        // println!("CREATING ERROR FROM TRAP {}", source);
        Self {
            inner: Arc::new(RuntimeErrorInner {
                source,
                wasm_trace,
                trap_code,
            }),
        }
    }

    /// Creates a custom user Error.
    ///
    /// This error object can be passed through Wasm frames and later retrieved
    /// using the `downcast` method.
    pub fn user(error: Box<dyn Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            Ok(err) => *err,
            Err(error) => error.into(),
        }
    }

    /// Returns a reference the `message` stored in `Trap`.
    pub fn message(&self) -> String {
        if let Some(trap_code) = self.inner.trap_code {
            return trap_code.message().to_string();
        } else {
            return self.inner.source.to_string();
        }
    }

    /// Returns a list of function frames in WebAssembly code that led to this
    /// trap happening.
    pub fn trace(&self) -> &[FrameInfo] {
        &self.inner.wasm_trace
    }

    /// Returns trap code, if it's a Trap
    pub fn to_trap(self) -> Option<TrapCode> {
        self.inner.trap_code
    }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match Arc::try_unwrap(self.inner) {
            Ok(inner) if inner.source.is::<T>() => Ok(inner.source.downcast::<T>().unwrap()),
            Ok(inner) => Err(Self {
                inner: Arc::new(inner),
            }),
            Err(inner) => Err(Self { inner }),
        }
    }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        self.inner.as_ref().source.downcast_ref::<T>()
    }

    /// Returns true if the `RuntimeError` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        self.inner.source.is::<T>()
    }
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeError")
            .field("source", &self.inner.source)
            .field("wasm_trace", &self.inner.wasm_trace)
            .finish()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RuntimeError: {}", self.message())?;
        let trace = self.trace();
        if trace.is_empty() {
            return Ok(());
        }
        for frame in self.trace().iter() {
            let name = frame.module_name();
            let func_index = frame.func_index();
            writeln!(f)?;
            write!(f, "    at ")?;
            match frame.function_name() {
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

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner.source {
            Trap::User(err) => Some(&**err),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl From<Box<dyn Error + Send + Sync>> for RuntimeError {
    fn from(error: Box<dyn Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            // The error is already a RuntimeError, we return it directly
            Ok(runtime_error) => *runtime_error,
            Err(error) => Trap::user(error).into(),
        }
    }
}

#[cfg(feature = "core")]
impl From<Box<dyn CoreError + Send + Sync>> for RuntimeError {
    fn from(error: Box<dyn Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            // The error is already a RuntimeError, we return it directly
            Ok(runtime_error) => *runtime_error,
            Err(error) => Trap::user(error).into(),
        }
    }
}

// ------

impl From<Trap> for RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<RuntimeError>() {
            return trap.downcast::<RuntimeError>().unwrap();
        }
        let info = FRAME_INFO.read().unwrap();
        let (wasm_trace, trap_code) = match &trap {
            // A user error
            Trap::User(_err) => (wasm_trace(&info, None, &Backtrace::new_unresolved()), None),
            // A trap caused by the VM being Out of Memory
            Trap::OOM { backtrace } => (wasm_trace(&info, None, backtrace), None),
            // A trap caused by an error on the generated machine code for a Wasm function
            Trap::Wasm {
                pc,
                signal_trap,
                backtrace,
            } => {
                let trap_code = info
                    .lookup_trap_info(*pc)
                    .map_or(signal_trap.unwrap_or(TrapCode::StackOverflow), |info| {
                        info.trap_code
                    });

                (wasm_trace(&info, Some(*pc), backtrace), Some(trap_code))
            }
            // A trap triggered manually from the Wasmer runtime
            Trap::Lib {
                trap_code,
                backtrace,
            } => (wasm_trace(&info, None, backtrace), Some(trap_code.clone())),
        };

        RuntimeError::new_from_source(trap, wasm_trace, trap_code)
    }
}

fn wasm_trace(
    info: &GlobalFrameInfo,
    trap_pc: Option<usize>,
    backtrace: &Backtrace,
) -> Vec<FrameInfo> {
    // Let's construct the trace
    backtrace
        .frames()
        .iter()
        .filter_map(|frame| {
            let pc = frame.ip() as usize;
            if pc == 0 {
                None
            } else {
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
                Some(pc_to_lookup)
            }
        })
        .filter_map(|pc| info.lookup_frame_info(pc))
        .collect::<Vec<_>>()
}
