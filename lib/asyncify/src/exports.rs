//! The asyncify exports of an instance and the data buffer they work on.

use std::{fmt, ops::Range};

use wasmer::{
    AsStoreMut, AsStoreRef, ExportError, Exports, Instance, Memory, MemoryAccessError,
    RuntimeError, TypedFunction,
};

/// Where an unwind saves the guest stack.
///
/// Asyncify keeps a `{ current, end }` header at `header`; the saved stack
/// goes in the buffer that follows it, up to `end`. Both header fields are as
/// wide as the module's pointers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncifyData {
    /// Address of the header in linear memory.
    pub header: u64,
    /// End of the buffer, exclusive.
    pub end: u64,
}

/// The value of asyncify's internal state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncifyState {
    /// Running normally.
    Normal,
    /// Unwinding the stack to the host.
    Unwinding,
    /// Rewinding the stack back to where it unwound.
    Rewinding,
}

/// Why an asyncify operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum AsyncifyError {
    /// The header or buffer is outside the memory.
    Memory(MemoryAccessError),
    /// The buffer is too small for the header and saved stack, or an address
    /// does not fit the module's pointer width.
    InvalidData,
    /// Calling an asyncify export trapped.
    Call(RuntimeError),
    /// `asyncify_get_state` returned a value asyncify does not define.
    UnknownState(i32),
}

impl fmt::Display for AsyncifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(err) => write!(f, "asyncify data is not accessible: {err}"),
            Self::InvalidData => f.write_str("asyncify data does not fit its buffer"),
            Self::Call(err) => write!(f, "asyncify export failed: {err}"),
            Self::UnknownState(state) => write!(f, "unknown asyncify state {state}"),
        }
    }
}

impl std::error::Error for AsyncifyError {}

impl From<MemoryAccessError> for AsyncifyError {
    fn from(err: MemoryAccessError) -> Self {
        Self::Memory(err)
    }
}

impl From<RuntimeError> for AsyncifyError {
    fn from(err: RuntimeError) -> Self {
        Self::Call(err)
    }
}

/// The asyncify exports of an instance, and the memory their data lives in.
#[derive(Clone, Debug)]
pub struct Asyncify {
    start_unwind: DataPtrFunction,
    stop_unwind: TypedFunction<(), ()>,
    start_rewind: DataPtrFunction,
    stop_rewind: TypedFunction<(), ()>,
    get_state: TypedFunction<(), i32>,
    memory: Memory,
}

impl Asyncify {
    /// Resolves the asyncify exports of `instance`, whose data lives in
    /// `memory`.
    pub fn new(
        store: &impl AsStoreRef,
        instance: &Instance,
        memory: Memory,
    ) -> Result<Self, ExportError> {
        let exports = &instance.exports;
        let start_unwind = DataPtrFunction::get(exports, store, "asyncify_start_unwind")?;
        let start_rewind = DataPtrFunction::get(exports, store, "asyncify_start_rewind")?;
        if start_unwind.is_64bit() != start_rewind.is_64bit() {
            return Err(ExportError::IncompatibleType);
        }
        Ok(Self {
            start_unwind,
            stop_unwind: exports.get_typed_function(store, "asyncify_stop_unwind")?,
            start_rewind,
            stop_rewind: exports.get_typed_function(store, "asyncify_stop_rewind")?,
            get_state: exports.get_typed_function(store, "asyncify_get_state")?,
            memory,
        })
    }

    /// Like [`Asyncify::new`], with the memory `instance` exports as `memory`.
    pub fn from_instance(
        store: &impl AsStoreRef,
        instance: &Instance,
    ) -> Result<Self, ExportError> {
        let memory = instance.exports.get_memory("memory")?.clone();
        Self::new(store, instance, memory)
    }

    /// Size of the `{ current, end }` header in front of the buffer.
    pub fn header_size(&self) -> u64 {
        if self.start_unwind.is_64bit() { 16 } else { 8 }
    }

    /// Returns asyncify's current state.
    pub fn state(&self, store: &mut impl AsStoreMut) -> Result<AsyncifyState, AsyncifyError> {
        match self.get_state.call(store)? {
            0 => Ok(AsyncifyState::Normal),
            1 => Ok(AsyncifyState::Unwinding),
            2 => Ok(AsyncifyState::Rewinding),
            state => Err(AsyncifyError::UnknownState(state)),
        }
    }

    /// Starts unwinding the guest stack into `data`'s buffer.
    ///
    /// The host function that calls this must return right away so that the
    /// guest can unwind.
    pub fn start_unwind(
        &self,
        store: &mut impl AsStoreMut,
        data: AsyncifyData,
    ) -> Result<(), AsyncifyError> {
        let buffer = self.buffer_start(data)?;
        self.write_header(store, data, buffer)?;
        self.start_unwind.call(store, data.header)
    }

    /// Finishes an unwind once the guest stack has unwound to the host.
    ///
    /// Returns where in memory the saved stack is.
    pub fn stop_unwind(
        &self,
        store: &mut impl AsStoreMut,
        data: AsyncifyData,
    ) -> Result<Range<u64>, AsyncifyError> {
        let buffer = self.buffer_start(data)?;
        let current = self.read_current(store, data)?;
        self.stop_unwind.call(store)?;
        Ok(buffer..current)
    }

    /// Starts rewinding into the stack saved in the first `saved_len` bytes of
    /// `data`'s buffer.
    ///
    /// Calling the function that unwound again then rewinds into it.
    pub fn start_rewind(
        &self,
        store: &mut impl AsStoreMut,
        data: AsyncifyData,
        saved_len: u64,
    ) -> Result<(), AsyncifyError> {
        let current = self
            .buffer_start(data)?
            .checked_add(saved_len)
            .filter(|current| *current <= data.end)
            .ok_or(AsyncifyError::InvalidData)?;
        self.write_header(store, data, current)?;
        self.start_rewind.call(store, data.header)
    }

    /// Finishes a rewind once the guest has rewound into the host function.
    pub fn stop_rewind(&self, store: &mut impl AsStoreMut) -> Result<(), AsyncifyError> {
        Ok(self.stop_rewind.call(store)?)
    }

    fn buffer_start(&self, data: AsyncifyData) -> Result<u64, AsyncifyError> {
        data.header
            .checked_add(self.header_size())
            .filter(|start| *start <= data.end)
            .ok_or(AsyncifyError::InvalidData)
    }

    fn write_header(
        &self,
        store: &mut impl AsStoreMut,
        data: AsyncifyData,
        current: u64,
    ) -> Result<(), AsyncifyError> {
        let view = self.memory.view(store);
        if self.start_unwind.is_64bit() {
            view.write(data.header, &current.to_le_bytes())?;
            view.write(data.header + 8, &data.end.to_le_bytes())?;
        } else {
            let current = u32::try_from(current).map_err(|_| AsyncifyError::InvalidData)?;
            let end = u32::try_from(data.end).map_err(|_| AsyncifyError::InvalidData)?;
            view.write(data.header, &current.to_le_bytes())?;
            view.write(data.header + 4, &end.to_le_bytes())?;
        }
        Ok(())
    }

    fn read_current(
        &self,
        store: &mut impl AsStoreMut,
        data: AsyncifyData,
    ) -> Result<u64, AsyncifyError> {
        let view = self.memory.view(store);
        if self.start_unwind.is_64bit() {
            let mut current = [0; 8];
            view.read(data.header, &mut current)?;
            Ok(u64::from_le_bytes(current))
        } else {
            let mut current = [0; 4];
            view.read(data.header, &mut current)?;
            Ok(u32::from_le_bytes(current).into())
        }
    }
}

/// `asyncify_start_unwind` or `asyncify_start_rewind`, which take the header
/// address as a pointer of the module's width.
#[derive(Clone, Debug)]
enum DataPtrFunction {
    Ptr32(TypedFunction<i32, ()>),
    Ptr64(TypedFunction<i64, ()>),
}

impl DataPtrFunction {
    fn get(exports: &Exports, store: &impl AsStoreRef, name: &str) -> Result<Self, ExportError> {
        exports
            .get_typed_function(store, name)
            .map(Self::Ptr32)
            .or_else(|_| exports.get_typed_function(store, name).map(Self::Ptr64))
    }

    fn is_64bit(&self) -> bool {
        matches!(self, Self::Ptr64(_))
    }

    fn call(&self, store: &mut impl AsStoreMut, ptr: u64) -> Result<(), AsyncifyError> {
        match self {
            Self::Ptr32(function) => {
                let ptr = u32::try_from(ptr).map_err(|_| AsyncifyError::InvalidData)?;
                function.call(store, ptr.cast_signed())?;
            }
            Self::Ptr64(function) => function.call(store, ptr.cast_signed())?,
        }
        Ok(())
    }
}
