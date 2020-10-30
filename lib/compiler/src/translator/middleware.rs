//! The middleware parses the function binary bytecodes and transform them
//! with the chosen functions.

use smallvec::SmallVec;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::Deref;
use wasmer_types::LocalFunctionIndex;
use wasmparser::{BinaryReader, Operator, Result as WpResult, Type};

/// A shared builder for function middlewares.
pub trait FunctionMiddlewareGenerator: Debug + Send + Sync {
    /// Generates a `FunctionMiddleware` for a given function.
    fn generate(&self, local_function_index: LocalFunctionIndex) -> Box<dyn FunctionMiddleware>;
}

/// A function middleware specialized for a single function.
pub trait FunctionMiddleware: Debug {
    /// Processes the given event, module info and sink.
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> WpResult<()> {
        state.push_operator(operator);
        Ok(())
    }
}

/// A Middleware binary reader of the WebAssembly structures and types.
#[derive(Debug)]
pub struct MiddlewareBinaryReader<'a> {
    /// Parsing state.
    state: MiddlewareReaderState<'a>,

    /// The backing middleware chain for this reader.
    chain: Vec<Box<dyn FunctionMiddleware>>,
}

/// The state of the binary reader. Exposed to middlewares to push their outputs.
#[derive(Debug)]
pub struct MiddlewareReaderState<'a> {
    /// Raw binary reader.
    inner: BinaryReader<'a>,

    /// The pending operations added by the middleware.
    pending_operations: VecDeque<Operator<'a>>,
}

/// Trait for generating middleware chains from "prototype" (generator) chains.
pub trait GenerateMiddlewareChain {
    /// Generates a middleware chain.
    fn generate_middleware_chain(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Vec<Box<dyn FunctionMiddleware>>;
}

impl<T: Deref<Target = dyn FunctionMiddlewareGenerator>> GenerateMiddlewareChain for [T] {
    /// Generates a middleware chain.
    fn generate_middleware_chain(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Vec<Box<dyn FunctionMiddleware>> {
        self.iter()
            .map(|x| x.generate(local_function_index))
            .collect()
    }
}

impl<'a> MiddlewareReaderState<'a> {
    /// Push an operator.
    pub fn push_operator(&mut self, operator: Operator<'a>) {
        self.pending_operations.push_back(operator);
    }
}

impl<'a> MiddlewareBinaryReader<'a> {
    /// Constructs a `MiddlewareBinaryReader` with an explicit starting offset.
    pub fn new_with_offset(data: &'a [u8], original_offset: usize) -> Self {
        let inner = BinaryReader::new_with_offset(data, original_offset);
        Self {
            state: MiddlewareReaderState {
                inner,
                pending_operations: VecDeque::new(),
            },
            chain: vec![],
        }
    }

    /// Replaces the middleware chain with a new one.
    pub fn set_middleware_chain(&mut self, stages: Vec<Box<dyn FunctionMiddleware>>) {
        self.chain = stages;
    }

    /// Read a `count` indicating the number of times to call `read_local_decl`.
    pub fn read_local_count(&mut self) -> WpResult<u32> {
        self.state.inner.read_var_u32()
    }

    /// Read a `(count, value_type)` declaration of local variables of the same type.
    pub fn read_local_decl(&mut self, locals_total: &mut usize) -> WpResult<(u32, Type)> {
        let count = self.state.inner.read_var_u32()?;
        let ty = self.state.inner.read_type()?;
        Ok((count, ty))
    }

    /// Reads the next available `Operator`.
    pub fn read_operator(&mut self) -> WpResult<Operator<'a>> {
        // Try to fill the `self.pending_operations` buffer, until it is non-empty.
        while self.state.pending_operations.is_empty() {
            let raw_op = self.state.inner.read_operator()?;

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

    /// Returns the inner `BinaryReader`'s current position.
    pub fn current_position(&self) -> usize {
        self.state.inner.current_position()
    }

    /// Returns the inner `BinaryReader`'s original position (with the offset)
    pub fn original_position(&self) -> usize {
        self.state.inner.original_position()
    }

    /// Returns the number of bytes remaining in the inner `BinaryReader`.
    pub fn bytes_remaining(&self) -> usize {
        self.state.inner.bytes_remaining()
    }

    /// Returns whether the inner `BinaryReader` has reached the end of the file.
    pub fn eof(&self) -> bool {
        self.state.inner.eof()
    }
}
