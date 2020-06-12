//! The middleware parses the function binary bytecodes and transform them
//! with the chosen functions.

use crate::error::CompileError;
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use wasm_common::LocalFunctionIndex;
use wasmparser::{BinaryReader, Operator, Result as WpResult, Type};

/// A registry that holds mappings from names to middleware builder generators.
pub struct MiddlewareRegistry {
    /// Middleware builder generators.
    generators: HashMap<String, Box<dyn MiddlewareBuilderGenerator>>,
}

/// A middleware builder generator.
pub trait MiddlewareBuilderGenerator: Debug + Send + Sync {
    /// Returns the version of the builder.
    fn version(&self) -> u32;

    /// Generates a builder from a configuration.
    fn generate(
        &self,
        configuration: &[u8],
    ) -> Result<Box<dyn FunctionMiddlewareBuilder>, CompileError>;
}

/// A shared builder for function middlewares.
pub trait FunctionMiddlewareBuilder: Debug + Send + Sync {
    /// Creates a `FunctionMiddleware` for a given function.
    fn prepare<'a>(
        &self,
        local_function_index: LocalFunctionIndex,
    ) -> Result<Box<dyn FunctionMiddleware>, CompileError>;
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

impl MiddlewareRegistry {
    /// Create a middleware registry.
    pub fn new() -> MiddlewareRegistry {
        MiddlewareRegistry {
            generators: HashMap::new(),
        }
    }

    /// Register a middleware.
    pub fn register<K: Into<String>, G: MiddlewareBuilderGenerator + 'static>(
        &mut self,
        key: K,
        generator: G,
    ) {
        self.generators.insert(key.into(), Box::new(generator));
    }

    /// Try to instantiate a builder.
    pub fn instantiate_builder<K: AsRef<str>>(
        &self,
        key: K,
        expected_version: u32,
        conf: &[u8],
    ) -> Result<Box<dyn FunctionMiddlewareBuilder>, CompileError> {
        match self.generators.get(key.as_ref()).map(|x| &**x) {
            Some(x) => {
                if x.version() == expected_version {
                    x.generate(conf)
                } else {
                    Err(CompileError::Codegen(format!(
                        "found middleware `{}` but version mismatches: expected `{}`, found `{}`",
                        key.as_ref(),
                        expected_version,
                        x.version()
                    )))
                }
            }
            None => Err(CompileError::Codegen(format!(
                "middleware `{}` not found",
                key.as_ref()
            ))),
        }
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

    /// Adds a stage to the back of the middleware chain.
    pub fn push_middleware_stage(&mut self, stage: Box<dyn FunctionMiddleware>) {
        self.chain.push(stage);
    }

    /// Read a `count` indicating the number of times to call `read_local_decl`.
    pub fn read_local_count(&mut self) -> WpResult<usize> {
        self.state.inner.read_local_count()
    }

    /// Read a `(count, value_type)` declaration of local variables of the same type.
    pub fn read_local_decl(&mut self, locals_total: &mut usize) -> WpResult<(u32, Type)> {
        self.state.inner.read_local_decl(locals_total)
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
