//! The middleware parses the function binary bytecodes and transform them
//! with the chosen functions.

use smallvec::SmallVec;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::{Deref, Range};
use wasmer_types::{LocalFunctionIndex, MiddlewareError, ModuleInfo, WasmError, WasmResult};
use wasmparser::{BinaryReader, FunctionBody, Operator, OperatorsReader, ValType};

use super::error::from_binaryreadererror_wasmerror;
use crate::translator::environ::FunctionBinaryReader;

/// A shared builder for function middlewares.
pub trait ModuleMiddleware: Debug + Send + Sync {
    /// Generates a `FunctionMiddleware` for a given function.
    ///
    /// Here we generate a separate object for each function instead of executing directly on per-function operators,
    /// in order to enable concurrent middleware application. Takes immutable `&self` because this function can be called
    /// concurrently from multiple compilation threads.
    fn generate_function_middleware(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Box<dyn FunctionMiddleware>;

    /// Transforms a `ModuleInfo` struct in-place. This is called before application on functions begins.
    fn transform_module_info(&self, _: &mut ModuleInfo) -> Result<(), MiddlewareError> {
        Ok(())
    }
}

/// A function middleware specialized for a single function.
pub trait FunctionMiddleware: Debug {
    /// Processes the given operator.
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> Result<(), MiddlewareError> {
        state.push_operator(operator);
        Ok(())
    }
}

/// A Middleware binary reader of the WebAssembly structures and types.
pub struct MiddlewareBinaryReader<'a> {
    /// Parsing state.
    state: MiddlewareReaderState<'a>,

    /// The backing middleware chain for this reader.
    chain: Vec<Box<dyn FunctionMiddleware>>,
}

enum MiddlewareInnerReader<'a> {
    Binary {
        reader: BinaryReader<'a>,
        original_reader: BinaryReader<'a>,
    },
    Operator(OperatorsReader<'a>),
}

/// The state of the binary reader. Exposed to middlewares to push their outputs.
pub struct MiddlewareReaderState<'a> {
    /// Raw binary reader.
    inner: Option<MiddlewareInnerReader<'a>>,

    /// The pending operations added by the middleware.
    pending_operations: VecDeque<Operator<'a>>,
}

/// Trait for generating middleware chains from "prototype" (generator) chains.
pub trait ModuleMiddlewareChain {
    /// Generates a function middleware chain.
    fn generate_function_middleware_chain(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Vec<Box<dyn FunctionMiddleware>>;

    /// Applies the chain on a `ModuleInfo` struct.
    fn apply_on_module_info(&self, module_info: &mut ModuleInfo) -> Result<(), MiddlewareError>;
}

impl<T: Deref<Target = dyn ModuleMiddleware>> ModuleMiddlewareChain for [T] {
    /// Generates a function middleware chain.
    fn generate_function_middleware_chain(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Vec<Box<dyn FunctionMiddleware>> {
        self.iter()
            .map(|x| x.generate_function_middleware(local_function_index))
            .collect()
    }

    /// Applies the chain on a `ModuleInfo` struct.
    fn apply_on_module_info(&self, module_info: &mut ModuleInfo) -> Result<(), MiddlewareError> {
        for item in self {
            item.transform_module_info(module_info)?;
        }
        Ok(())
    }
}

impl<'a> MiddlewareReaderState<'a> {
    /// Push an operator.
    pub fn push_operator(&mut self, operator: Operator<'a>) {
        self.pending_operations.push_back(operator);
    }
}

impl<'a> Extend<Operator<'a>> for MiddlewareReaderState<'a> {
    fn extend<I: IntoIterator<Item = Operator<'a>>>(&mut self, iter: I) {
        self.pending_operations.extend(iter);
    }
}

impl<'a: 'b, 'b> Extend<&'b Operator<'a>> for MiddlewareReaderState<'a> {
    fn extend<I: IntoIterator<Item = &'b Operator<'a>>>(&mut self, iter: I) {
        self.pending_operations.extend(iter.into_iter().cloned());
    }
}

impl<'a> MiddlewareBinaryReader<'a> {
    /// Constructs a `MiddlewareBinaryReader` with an explicit starting offset.
    pub fn new_with_offset(data: &'a [u8], original_offset: usize) -> Self {
        let inner = BinaryReader::new(data, original_offset);
        Self {
            state: MiddlewareReaderState {
                inner: Some(MiddlewareInnerReader::Binary {
                    original_reader: inner.clone(),
                    reader: inner,
                }),
                pending_operations: VecDeque::new(),
            },
            chain: vec![],
        }
    }

    /// Replaces the middleware chain with a new one.
    pub fn set_middleware_chain(&mut self, stages: Vec<Box<dyn FunctionMiddleware>>) {
        self.chain = stages;
    }
}

impl<'a> FunctionBinaryReader<'a> for MiddlewareBinaryReader<'a> {
    fn read_local_count(&mut self) -> WasmResult<u32> {
        match self.state.inner.as_mut().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader
                .read_var_u32()
                .map_err(from_binaryreadererror_wasmerror),
            MiddlewareInnerReader::Operator(..) => Err(WasmError::InvalidWebAssembly {
                message: "function body already visited".to_string(),
                offset: self.current_position(),
            }),
        }
    }

    fn read_local_decl(&mut self) -> WasmResult<(u32, ValType)> {
        match self.state.inner.as_mut().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => {
                let count = reader
                    .read_var_u32()
                    .map_err(from_binaryreadererror_wasmerror)?;
                let ty: ValType = reader
                    .read::<ValType>()
                    .map_err(from_binaryreadererror_wasmerror)?;
                Ok((count, ty))
            }
            MiddlewareInnerReader::Operator(..) => Err(WasmError::InvalidWebAssembly {
                message: "function body already visited".to_string(),
                offset: self.current_position(),
            }),
        }
    }

    fn read_operator(&mut self) -> WasmResult<Operator<'a>> {
        if let Some(MiddlewareInnerReader::Binary {
            original_reader, ..
        }) = self
            .state
            .inner
            .take_if(|state| matches!(state, MiddlewareInnerReader::Binary { .. }))
        {
            self.state.inner = Some(MiddlewareInnerReader::Operator(
                FunctionBody::new(original_reader)
                    .get_operators_reader()
                    .map_err(from_binaryreadererror_wasmerror)?,
            ))
        }

        if self.chain.is_empty() {
            let Some(MiddlewareInnerReader::Operator(operator_reader)) = &mut self.state.inner
            else {
                unreachable!();
            };
            // We short-circuit in case no chain is used
            return operator_reader
                .read()
                .map_err(from_binaryreadererror_wasmerror);
        }

        // Try to fill the `self.pending_operations` buffer, until it is non-empty.
        while self.state.pending_operations.is_empty() {
            let Some(MiddlewareInnerReader::Operator(operator_reader)) = &mut self.state.inner
            else {
                unreachable!();
            };
            let raw_op = operator_reader
                .read()
                .map_err(from_binaryreadererror_wasmerror)?;

            // Fill the initial raw operator into pending buffer.
            self.state.pending_operations.push_back(raw_op);

            // Run the operator through each stage.
            for stage in &mut self.chain {
                // Take the outputs from the previous stage.
                let pending: SmallVec<[Operator<'a>; 2]> =
                    self.state.pending_operations.drain(0..).collect();

                // ...and feed them into the current stage.
                for pending_op in pending {
                    stage.feed(pending_op, &mut self.state)?;
                }
            }
        }

        Ok(self.state.pending_operations.pop_front().unwrap())
    }

    fn current_position(&self) -> usize {
        match self.state.inner.as_ref().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader.current_position(),
            MiddlewareInnerReader::Operator(operator_reader) => {
                // TODO: do not convert to BinaryReader
                operator_reader.get_binary_reader().current_position()
            }
        }
    }

    fn original_position(&self) -> usize {
        match self.state.inner.as_ref().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader.original_position(),
            MiddlewareInnerReader::Operator(operator_reader) => operator_reader.original_position(),
        }
    }

    fn bytes_remaining(&self) -> usize {
        match self.state.inner.as_ref().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader.bytes_remaining(),
            MiddlewareInnerReader::Operator(operator_reader) => {
                operator_reader.get_binary_reader().bytes_remaining()
            }
        }
    }

    fn eof(&self) -> bool {
        match self.state.inner.as_ref().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader.eof(),
            MiddlewareInnerReader::Operator(operator_reader) => operator_reader.eof(),
        }
    }

    fn range(&self) -> Range<usize> {
        match self.state.inner.as_ref().expect("inner state must exist") {
            MiddlewareInnerReader::Binary { reader, .. } => reader.range(),
            MiddlewareInnerReader::Operator(operator_reader) => {
                operator_reader.get_binary_reader().range()
            }
        }
    }
}
