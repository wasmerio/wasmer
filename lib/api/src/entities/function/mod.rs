//! Defines the [`Function`] and [`HostFunction`] types and useful traits and data types to
//! interact with them.

pub(crate) mod inner;
pub use inner::*;

pub(crate) mod host;
pub use host::*;

pub(crate) mod env;
pub use env::*;

pub(crate) mod call_wrapper;
pub use call_wrapper::{CallWrapper, WasmCall};

#[cfg(feature = "experimental-async")]
pub(crate) mod async_host;
#[cfg(feature = "experimental-async")]
pub use async_host::{AsyncFunctionEnv, AsyncHostFunction};

use std::{future::Future, pin::Pin};

use wasmer_types::{FunctionType, RawValue};

#[cfg(feature = "experimental-async")]
use crate::AsStoreAsync;
use crate::{
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef, TypedFunction,
    Value, WasmTypeList,
    error::RuntimeError,
    vm::{VMExtern, VMExternFunction, VMFuncRef},
};

/// A WebAssembly `function` instance.
///
/// A function instance is the runtime representation of a function.
/// It effectively is a closure of the original function (defined in either
/// the host or the WebAssembly module) over the runtime [`crate::Instance`] of its
/// originating [`crate::Module`].
///
/// The module instance is used to resolve references to other definitions
/// during execution of the function.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#function-instances>
///
/// # Panics
/// - Closures (functions with captured environments) are not currently supported
///   with native functions. Attempting to create a native `Function` with one will
///   result in a panic.
///   [Closures as host functions tracking issue](https://github.com/wasmerio/wasmer/issues/1840)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Function(pub(crate) BackendFunction);

impl Function {
    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Function::new_typed`] for less runtime overhead.
    pub fn new<FT, F>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Result<Vec<Value>, RuntimeError> + 'static + Send + Sync,
    {
        Self(BackendFunction::new(store, ty, func))
    }

    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Function::new_typed_with_env`] for less runtime overhead.
    ///
    /// Takes a [`FunctionEnv`] that is passed into func. If that is not required,
    /// [`Function::new`] might be an option as well.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// let signature = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    ///
    /// let f = Function::new_with_env(&mut store, &env, &signature, |_env, args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    ///
    /// With constant signature:
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// const I32_I32_TO_I32: ([Type; 2], [Type; 1]) = ([Type::I32, Type::I32], [Type::I32]);
    ///
    /// let f = Function::new_with_env(&mut store, &env, I32_I32_TO_I32, |_env, args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    pub fn new_with_env<FT, F, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        Self(BackendFunction::new_with_env(store, env, ty, func))
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        Self(BackendFunction::new_typed(store, func))
    }

    /// Creates a new host `Function` with an environment from a typed function.
    ///
    /// The function signature is automatically retrieved using the
    /// Rust typing system.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Store, Function, FunctionEnv, FunctionEnvMut};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    /// ```
    pub fn new_typed_with_env<T: Send + 'static, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        Self(BackendFunction::new_typed_with_env(store, env, func))
    }

    /// Creates a new async host `Function` (dynamic) with the provided
    /// signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Self::new_typed_async`] for less runtime overhead.
    ///
    /// The provided closure returns a future that resolves to the function results.
    /// When invoked synchronously (via [`Function::call`]) the future will run to
    /// completion immediately, provided it doesn't suspend. When invoked through
    /// [`Function::call_async`], the future may suspend and resume as needed.
    #[cfg(feature = "experimental-async")]
    pub fn new_async<FT, F, Fut>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
    {
        Self(BackendFunction::new_async(store, ty, func))
    }

    /// Creates a new async host `Function` (dynamic) with the provided
    /// signature and environment.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Self::new_typed_with_env_async`] for less runtime overhead.
    ///
    /// Takes an [`AsyncFunctionEnvMut`] that is passed into func. If
    /// that is not required, [`Self::new_async`] might be an option as well.
    #[cfg(feature = "experimental-async")]
    pub fn new_with_env_async<FT, F, Fut, T: 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
    {
        Self(BackendFunction::new_with_env_async(store, env, ty, func))
    }

    /// Creates a new async host `Function` from a native typed function.
    ///
    /// The future can return either the raw result tuple or any type that implements
    /// [`IntoResult`](crate::IntoResult) for the result tuple (e.g. `Result<Rets, E>`).
    #[cfg(feature = "experimental-async")]
    pub fn new_typed_async<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        Rets: WasmTypeList + 'static,
        Args: WasmTypeList + 'static,
        F: AsyncHostFunction<(), Args, Rets, WithoutEnv> + 'static,
    {
        Self(BackendFunction::new_typed_async(store, func))
    }

    /// Creates a new async host `Function` with an environment from a typed function.
    #[cfg(feature = "experimental-async")]
    pub fn new_typed_with_env_async<T: 'static, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        Rets: WasmTypeList + 'static,
        Args: WasmTypeList + 'static,
        F: AsyncHostFunction<T, Args, Rets, WithEnv> + 'static,
    {
        Self(BackendFunction::new_typed_with_env_async(store, env, func))
    }

    /// Returns the [`FunctionType`] of the `Function`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.ty(&mut store).params(), vec![Type::I32, Type::I32]);
    /// assert_eq!(f.ty(&mut store).results(), vec![Type::I32]);
    /// ```
    pub fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        self.0.ty(store)
    }

    /// Returns the number of parameters that this function takes.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.param_arity(&mut store), 2);
    /// ```
    pub fn param_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).params().len()
    }

    /// Returns the number of results this function produces.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.result_arity(&mut store), 1);
    /// ```
    pub fn result_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).results().len()
    }

    /// Call the function.
    ///
    /// Depending on where the Function is defined, it will call it.
    /// 1. If the function is defined inside a WebAssembly, it will call the trampoline
    ///    for the function signature.
    /// 2. If the function is defined in the host (in a native way), it will
    ///    call the trampoline.
    ///
    /// On the JavaScript backend, a guest's function can be called with any
    /// store, but the host functions it calls run only with their own, lent
    /// by the call: one of another store fails the call without running. So
    /// does one that JavaScript reaches by calling a guest's export as a
    /// function, outside any call or from a host function: a host function
    /// calls guests with this method instead.
    ///
    /// Calls nested in host functions on one thread fail with a stack
    /// overflow once they are 100 deep, so that the engine's exception for
    /// running out of stack does not unwind Rust frames without running them.
    /// This is a mitigation: a guest with frames of a few KB can run the
    /// engine out of stack before that.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// assert_eq!(sum.call(&mut store, &[Value::I32(1), Value::I32(2)]).unwrap().to_vec(), vec![Value::I32(3)]);
    /// ```
    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.0.call(store, params)
    }

    /// Calls the function through a [`CallWrapper`], which decides when and
    /// how many times the function is entered.
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Instance, Module, Store, Value, WasmCall};
    /// # let mut store = Store::default();
    /// # let wasm_bytes = wat2wasm(br#"(module (func (export "one") (result i32) i32.const 1))"#).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    /// let one = instance.exports.get_function("one").unwrap();
    ///
    /// let mut entries = 0;
    /// let mut count_entries = |call: &mut WasmCall<'_>| {
    ///     entries += 1;
    ///     call.invoke()
    /// };
    /// let results = one.call_wrapped(&mut store, &mut count_entries, &[]).unwrap();
    /// assert_eq!(results.to_vec(), vec![Value::I32(1)]);
    /// assert_eq!(entries, 1);
    /// ```
    pub fn call_wrapped(
        &self,
        store: &mut impl AsStoreMut,
        wrapper: &mut dyn CallWrapper,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.0.call_wrapped(store, wrapper, params)
    }

    /// Calls the function asynchronously.
    ///
    /// The returned future drives execution of the WebAssembly function on a
    /// coroutine stack. Host functions created with [`Function::new_async`] may
    /// suspend execution by awaiting futures, and their completion will resume
    /// the Wasm instance according to the JSPI proposal.
    ///
    /// On the `sys` backend that coroutine has a fixed stack of 1 MiB. The
    /// host functions the guest calls run on it; one that needs more stack
    /// runs into the stack's guard page, and the process is killed. The
    /// guest's own frames are on a stack of their own, whose size
    /// `wasmer_vm::set_stack_size` sets. The future of an async host function
    /// is polled by the future of the call, on the executor's stack.
    ///
    /// On `sys`, as on `js`, a guest suspends only where no host function
    /// holds a borrow of its store: a guest that a synchronous host function
    /// calls, on its own store or another, cannot be suspended, and an async
    /// host function it calls fails with `TrapCode::YieldOutsideAsyncContext`
    /// unless its future is ready at once. An async host function that needs
    /// to wait for a call into a guest awaits that guest's `call_async`.
    ///
    /// On `sys`, dropping the call while its guest waits in an async host
    /// function unwinds the coroutine with a panic: guest frames are
    /// discarded, and the frames of the call between them run their
    /// destructors; none of them is a host function's. A call dropped while
    /// its thread exits, from a thread local, or with `baremetal`, is
    /// forgotten instead, with its stack.
    ///
    /// On the `js` backend the engine suspends the guest's stack (JSPI). A
    /// function that is neither a WebAssembly function nor an async host
    /// function, a synchronous host function or JavaScript an embedder gave,
    /// is called as [`Function::call`] calls it, and an async host function
    /// called directly returns the values its future returned, as they are.
    /// The first call on a thread compiles a small WebAssembly module, whose
    /// trap unwinds a guest that must not run again: on a host that refuses
    /// to compile it, the call fails before it enters anything. The
    /// call holds the store's write lock while its guest runs, and gives it
    /// up while the guest waits in an async host function. Dropping the call
    /// while its guest waits unwinds the guest, which runs no more code, and
    /// gives the store back. The call keeps the store for as long as the
    /// engine may still resume the guest where Wasmer does not see it, also
    /// once the call is dropped, since nothing can stop the engine then:
    ///
    /// * while the guest is suspended in a `WebAssembly.Suspending` function
    ///   that Wasmer did not make;
    /// * while the promise the guest waits on waits for a thenable: an
    ///   `externref` result of an async host function whose `then` reads
    ///   differently the second time, as a `Proxy` can make it, which the
    ///   promise takes for a thenable (any other thenable result fails the
    ///   import);
    /// * on JavaScriptCore, while a thenable the guest returned is pending:
    ///   the engine resolves the call's promise with it, and the call ends
    ///   with what it settles with.
    ///
    /// A thenable that never settles keeps the store for good: it cannot be
    /// taken back with `StoreAsync::into_store`.
    ///
    /// A guest entered while another guest of the same store runs, by a
    /// synchronous call, a host function, or a JavaScript function given with
    /// `Function::from_jsvalue`, cannot be suspended: an async host function
    /// it calls fails with `TrapCode::YieldOutsideAsyncContext`. JavaScript
    /// that Wasmer never saw, an import of a module instantiated without it,
    /// must not call a guest's export while a guest of the same store runs:
    /// on engines that run an async import before they refuse to suspend its
    /// guest (Node.js 24, and JavaScriptCore beneath a JavaScript frame),
    /// Wasmer cannot tell the refused import from one that waits.
    #[must_use = "This function spawns a future that must be awaited to produce results"]
    #[cfg(feature = "experimental-async")]
    pub fn call_async(
        &self,
        store: &impl AsStoreAsync,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static {
        self.0.call_async(store, params)
    }

    #[doc(hidden)]
    #[allow(missing_docs)]
    pub fn call_raw(
        &self,
        store: &mut impl AsStoreMut,
        params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.0.call_raw(store, params)
    }

    pub(crate) fn vm_funcref(&self, store: &impl AsStoreRef) -> VMFuncRef {
        self.0.vm_funcref(store)
    }

    pub(crate) unsafe fn from_vm_funcref(store: &mut impl AsStoreMut, funcref: VMFuncRef) -> Self {
        unsafe { Self(BackendFunction::from_vm_funcref(store, funcref)) }
    }

    /// Transform this WebAssembly function into a typed function.
    /// See [`TypedFunction`] to learn more.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    /// let sum_typed: TypedFunction<(i32, i32), i32> = sum.typed(&mut store).unwrap();
    ///
    /// assert_eq!(sum_typed.call(&mut store, 1, 2).unwrap(), 3);
    /// ```
    ///
    /// # Errors
    ///
    /// If the `Args` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed : TypedFunction<(i64, i64), i32> = sum.typed(&mut store).unwrap();
    /// ```
    ///
    /// If the `Rets` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed: TypedFunction<(i32, i32), i64> = sum.typed(&mut store).unwrap();
    /// ```
    pub fn typed<Args, Rets>(
        &self,
        store: &impl AsStoreRef,
    ) -> Result<TypedFunction<Args, Rets>, RuntimeError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        self.0.typed(store)
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternFunction) -> Self {
        Self(BackendFunction::from_vm_extern(store, vm_extern))
    }

    /// Checks whether this `Function` can be used with the given store.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl<'a> Exportable<'a> for Function {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(func),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
