use crate::{
    backend::RunnableModule,
    backend::{sys::Memory, Backend, CacheGen, Compiler, CompilerConfig, Token},
    cache::{Artifact, Error as CacheError},
    error::{CompileError, CompileResult},
    module::{ModuleInfo, ModuleInner},
    parse::LoadError,
    structures::Map,
    types::{FuncIndex, FuncSig, SigIndex},
};
use smallvec::SmallVec;
use std::fmt::Debug;
use std::marker::PhantomData;
use wasmparser::{Operator, Type as WpType};

pub enum Event<'a, 'b> {
    Internal(InternalEvent),
    Wasm(&'b Operator<'a>),
}

pub enum InternalEvent {
    FunctionBegin(u32),
    FunctionEnd,
    Breakpoint(Box<Fn(BkptInfo) + Send + Sync + 'static>),
    SetInternal(u32),
    GetInternal(u32),
}

pub struct BkptInfo {}

pub trait ModuleCodeGenerator<FCG: FunctionCodeGenerator<E>, RM: RunnableModule, E: Debug> {
    fn new() -> Self;
    fn backend_id() -> Backend;
    fn check_precondition(&mut self, module_info: &ModuleInfo) -> Result<(), E>;

    /// Creates a new function and returns the function-scope code generator for it.
    fn next_function(&mut self) -> Result<&mut FCG, E>;
    fn finalize(self, module_info: &ModuleInfo) -> Result<RM, E>;
    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), E>;

    /// Sets function signatures.
    fn feed_function_signatures(&mut self, assoc: Map<FuncIndex, SigIndex>) -> Result<(), E>;

    /// Adds an import function.
    fn feed_import_function(&mut self) -> Result<(), E>;
}

pub struct StreamingCompiler<
    MCG: ModuleCodeGenerator<FCG, RM, E>,
    FCG: FunctionCodeGenerator<E>,
    RM: RunnableModule + 'static,
    E: Debug,
    CGEN: Fn() -> MiddlewareChain,
> {
    middleware_chain_generator: CGEN,
    _phantom_mcg: PhantomData<MCG>,
    _phantom_fcg: PhantomData<FCG>,
    _phantom_rm: PhantomData<RM>,
    _phantom_e: PhantomData<E>,
}

pub struct SimpleStreamingCompilerGen<
    MCG: ModuleCodeGenerator<FCG, RM, E>,
    FCG: FunctionCodeGenerator<E>,
    RM: RunnableModule + 'static,
    E: Debug,
> {
    _phantom_mcg: PhantomData<MCG>,
    _phantom_fcg: PhantomData<FCG>,
    _phantom_rm: PhantomData<RM>,
    _phantom_e: PhantomData<E>,
}

impl<
        MCG: ModuleCodeGenerator<FCG, RM, E>,
        FCG: FunctionCodeGenerator<E>,
        RM: RunnableModule + 'static,
        E: Debug,
    > SimpleStreamingCompilerGen<MCG, FCG, RM, E>
{
    pub fn new() -> StreamingCompiler<MCG, FCG, RM, E, impl Fn() -> MiddlewareChain> {
        StreamingCompiler::new(|| MiddlewareChain::new())
    }
}

impl<
        MCG: ModuleCodeGenerator<FCG, RM, E>,
        FCG: FunctionCodeGenerator<E>,
        RM: RunnableModule + 'static,
        E: Debug,
        CGEN: Fn() -> MiddlewareChain,
    > StreamingCompiler<MCG, FCG, RM, E, CGEN>
{
    pub fn new(chain_gen: CGEN) -> Self {
        Self {
            middleware_chain_generator: chain_gen,
            _phantom_mcg: PhantomData,
            _phantom_fcg: PhantomData,
            _phantom_rm: PhantomData,
            _phantom_e: PhantomData,
        }
    }
}

impl<
        MCG: ModuleCodeGenerator<FCG, RM, E>,
        FCG: FunctionCodeGenerator<E>,
        RM: RunnableModule + 'static,
        E: Debug,
        CGEN: Fn() -> MiddlewareChain,
    > Compiler for StreamingCompiler<MCG, FCG, RM, E, CGEN>
{
    fn compile(
        &self,
        wasm: &[u8],
        compiler_config: CompilerConfig,
        _: Token,
    ) -> CompileResult<ModuleInner> {
        struct Placeholder;
        impl CacheGen for Placeholder {
            fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError> {
                Err(CacheError::Unknown(
                    "the streaming compiler API doesn't support caching yet".to_string(),
                ))
            }
        }

        let mut mcg = MCG::new();
        let mut chain = (self.middleware_chain_generator)();
        let info = crate::parse::read_module(
            wasm,
            MCG::backend_id(),
            &mut mcg,
            &mut chain,
            &compiler_config,
        )?;
        let exec_context = mcg
            .finalize(&info)
            .map_err(|x| CompileError::InternalError {
                msg: format!("{:?}", x),
            })?;
        Ok(ModuleInner {
            cache_gen: Box::new(Placeholder),
            runnable_module: Box::new(exec_context),
            info: info,
        })
    }

    unsafe fn from_cache(&self, _artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        Err(CacheError::Unknown(
            "the streaming compiler API doesn't support caching yet".to_string(),
        ))
    }
}

pub struct EventSink<'a, 'b> {
    buffer: SmallVec<[Event<'a, 'b>; 2]>,
}

impl<'a, 'b> EventSink<'a, 'b> {
    pub fn push(&mut self, ev: Event<'a, 'b>) {
        self.buffer.push(ev);
    }
}

pub struct MiddlewareChain {
    chain: Vec<Box<GenericFunctionMiddleware>>,
}

impl MiddlewareChain {
    pub fn new() -> MiddlewareChain {
        MiddlewareChain { chain: vec![] }
    }

    pub fn push<M: FunctionMiddleware + 'static>(&mut self, m: M) {
        self.chain.push(Box::new(m));
    }

    pub(crate) fn run<E: Debug, FCG: FunctionCodeGenerator<E>>(
        &mut self,
        fcg: Option<&mut FCG>,
        ev: Event,
        module_info: &ModuleInfo,
    ) -> Result<(), String> {
        let mut sink = EventSink {
            buffer: SmallVec::new(),
        };
        sink.push(ev);
        for m in &mut self.chain {
            let prev: SmallVec<[Event; 2]> = sink.buffer.drain().collect();
            for ev in prev {
                m.feed_event(ev, module_info, &mut sink)?;
            }
        }
        if let Some(fcg) = fcg {
            for ev in sink.buffer {
                fcg.feed_event(ev, module_info)
                    .map_err(|x| format!("{:?}", x))?;
            }
        }

        Ok(())
    }
}

pub trait FunctionMiddleware {
    type Error: Debug;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error>;
}

pub(crate) trait GenericFunctionMiddleware {
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), String>;
}

impl<E: Debug, T: FunctionMiddleware<Error = E>> GenericFunctionMiddleware for T {
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), String> {
        <Self as FunctionMiddleware>::feed_event(self, op, module_info, sink)
            .map_err(|x| format!("{:?}", x))
    }
}

/// The function-scope code generator trait.
pub trait FunctionCodeGenerator<E: Debug> {
    /// Sets the return type.
    fn feed_return(&mut self, ty: WpType) -> Result<(), E>;

    /// Adds a parameter to the function.
    fn feed_param(&mut self, ty: WpType) -> Result<(), E>;

    /// Adds `n` locals to the function.
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), E>;

    /// Called before the first call to `feed_opcode`.
    fn begin_body(&mut self) -> Result<(), E>;

    /// Called for each operator.
    fn feed_event(&mut self, op: Event, module_info: &ModuleInfo) -> Result<(), E>;

    /// Finalizes the function.
    fn finalize(&mut self) -> Result<(), E>;
}
