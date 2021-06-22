use super::frame_info::{FrameInfo, GlobalFrameInfo, FRAME_INFO};
use backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use wasm_bindgen::Exception;

/// A struct representing an aborted instruction execution, with a message
/// indicating the cause.
#[derive(Clone)]
pub struct RuntimeError {
    inner: Arc<RuntimeErrorInner>,
}

/// The source of the `RuntimeError`.
#[derive(Debug)]
enum RuntimeErrorSource {
    Generic(String),
    User(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for RuntimeErrorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generic(s) => write!(f, "{}", s),
            Self::User(s) => write!(f, "{}", s),
            Self::OOM => write!(f, "Wasmer VM out of memory"),
            Self::Trap(s) => write!(f, "{}", s.message()),
        }
    }
}

struct RuntimeErrorInner {
    /// The source error (this can be a custom user `Error` or a [`TrapCode`])
    source: RuntimeErrorSource,
}

fn _assert_trap_is_sync_and_send(t: &Trap) -> (&dyn Sync, &dyn Send) {
    (t, t)
}

impl RuntimeError {
    /// Creates a new generic `RuntimeError` with the given `message`.
    ///
    /// # Example
    /// ```
    /// let trap = wasmer_engine::RuntimeError::new("unexpected error");
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new<I: Into<String>>(message: I) -> Self {
        let info = FRAME_INFO.read().unwrap();
        let msg = message.into();
        Self::new_with_trace(
            &info,
            None,
            RuntimeErrorSource::Generic(msg),
            Backtrace::new_unresolved(),
        )
    }

    /// Raises a custom user Error
    pub fn raise(error: Box<dyn Error + Send + Sync>) -> ! {
        wasm_bindgen::throw_val()
    }

    fn new_with_trace(
        info: &GlobalFrameInfo,
        trap_pc: Option<usize>,
        source: RuntimeErrorSource,
        native_trace: Backtrace,
    ) -> Self {
        let frames: Vec<usize> = native_trace
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
            .collect();

        // Let's construct the trace
        let wasm_trace = frames
            .into_iter()
            .filter_map(|pc| info.lookup_frame_info(pc))
            .collect::<Vec<_>>();

        Self {
            inner: Arc::new(RuntimeErrorInner {
                source,
                wasm_trace,
                native_trace,
            }),
        }
    }

    /// Returns a reference the `message` stored in `Trap`.
    pub fn message(&self) -> String {
        format!("{}", self.inner.source)
    }

    /// Returns a list of function frames in WebAssembly code that led to this
    /// trap happening.
    pub fn trace(&self) -> &[FrameInfo] {
        &self.inner.wasm_trace
    }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self.inner.source {
            // We only try to downcast user errors
            RuntimeErrorSource::User(err)) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            Ok(inner) => Err(Self {
                inner: Arc::new(inner),
            }),
            Err(inner) => Err(Self { inner }),
        }
    }

    /// Returns trap code, if it's a Trap
    pub fn to_trap(self) -> Option<TrapCode> {
        if let RuntimeErrorSource::Trap(trap_code) = self.inner.source {
            Some(trap_code)
        } else {
            None
        }
    }

    /// Returns true if the `RuntimeError` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match &self.inner.source {
            RuntimeErrorSource::User(err) => err.is::<T>(),
            _ => false,
        }
    }
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeError")
            .field("source", &self.inner.source)
            .field("wasm_trace", &self.inner.wasm_trace)
            .field("native_trace", &self.inner.native_trace)
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
            RuntimeErrorSource::User(err) => Some(&**err),
            RuntimeErrorSource::Trap(err) => Some(err),
            _ => None,
        }
    }
}

impl From<Trap> for RuntimeError {
    fn from(trap: Trap) -> Self {
        Self::from_trap(trap)
    }
}
