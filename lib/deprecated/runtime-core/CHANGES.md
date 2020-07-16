# Changes between this port and the old `wasmer-runtime-core` API

This document lists *only the differences* between the old and the new
API (the port), i.e. API that didn't change are absent.

## Overall

* Host function must always take a `vm::Ctx` as first parameter

## All changes

By type name in alphabetic order.

### `Artifact`

Before:

```rust
impl Artifact {
    fn deserialize(bytes: &[u8]) -> Result<Self, Error>;
}
```

After:

```rust
impl Artifact {
    unsafe fn deserialize(bytes: &[u8]) -> Result<Self, Error>;
    fn module(self) -> Module;
}
```

The `deserialize` method is now marked as `unsafe` since it is not
checked that `bytes` represent a valid artifact.

The new `module` method is introduced to fetch the `Module` inside the
artifact.

### `DynamicFunc`

Before:

```rust
impl DynamicFunc {
    fn new<F>(signature: Arc<FuncSig>, func: F) -> Self;
}
```

After:

```rust
impl DynamicFunc {
    fn new<F>(signature: &FuncSig, func: F) -> Self;
    fn signature(&self) -> &FuncSig;
    fn params(&self) -> &[Type];
    fn returns(&self) -> &[Type];
    fn call(&self, params: &[Value] -> Result<Box<[Value]>, RuntimeError>;
}
```

The constructor `new` no longer takes an `Arc` but a reference to
`FuncSig`.

`signature`, `params`, `returns` and `call` are new
methods. Previously, it was required to convert `DynamicFunc` into
`Func` with `Into<Func<…, …>>`. Now there is no conversion possible
between the two, and `DynamicFunc` gains its own methods.

### `DynFunc`

Before:

```rust
impl DynFunc {
    fn raw(&self) -> *const Func;
}
```

After:

```rust
impl DynFunc {
}
```

The `raw` method has been removed. It was present for internal
purposes only, not sure it will impact you.

### `ExportDescriptor`

Before:

```rust
struct ExportDescriptor<'a> {
    name: &'a str,
    ty: ExternDescriptor,
} 
```

After:

```rust
struct ExportDescriptor {
    name: String,
    ty: ExternDescriptor,
} 
```

The lifetime on `ExportDescriptor` is no longer necessary: The `name`
field now holds a `String` instead of a `&str`.

### `Export`

Before:

```rust
impl Exports {
    fn get<'a, T: Exportable<'a>>(&'a self, name: &str) -> Result<T, ResolveError>;
    fn into_iter(&self) -> ExportIter;
}
```

After:

```rust
impl Exports {
    fn get<'a, T: Exportable<'a> + Clone + 'a>(&'a self, name: &str) -> Result<T, ExportError>;
    fn iter(&self) -> ExportsIterator<impl Iterator<Item = (&String, &Export)>>;
}
```

The `get` method is different. It returns a `T` where `T` implements
`Exportable<'a>` _and_ `Clone` (that the addition). The returned error
is also `ExportError` instead of `ResolveError`.

The method `into_iter` is now `iter` and returns references to the
export name and export value.

### `Func`

Before:

```rust
impl<Args, Rets> Func<Args, Rets> {
    fn params(&self) -> &'static [Type];
    fn returns(&self) -> &'static [Type];
    fn get_vm_func(&self) -> NonNull<Func>;
}
```

After:

```rust
impl<Args, Rets> Func<Args, Rets> {
    fn params(&self) -> &[Type];
    fn returns(&self) -> &[Type];
    fn dyn_call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError>;
    fn signature(&self) -> &FuncSig;
}
```

The `params` and `returns` return a slice of `[Type]` which has not
the `'static` lifetime.

The `get_vm_func` method has been removed. It was present for internal
purposes, it unlikely it will have an impact on your project.

In addition to the `call` method, there is now a `dyn_call` method,
that calls the function with arguments packed in a slice of `Value`.

Finally, there is a new `signature` method.

### `FuncSig`

Before:

```rust
impl FuncSig {
    fn returns(&self) -> &[Type];
    fn check_param_value_types(&self, params: &[Value]) -> bool;
}
```

After:

```rust
impl FuncSig {
    fn results(&self) -> &[Type];
}
```

The `returns` method has been renamed `results`.

The `check_param_value_types` method has been removed, since it is now
possible to compare two `Vec<Type>`.

### `GlobalInit`

`GlobalInit` has totally changed. It was a `struct`, now it's an
`enum`:

```rust
enum GlobalInit {
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
    V128Const(V128),
    GetGlobal(GlobalIndex),
    RefNullConst,
    RefFunc(FunctionIndex),
}

impl GlobalInit {
    fn from_value<T>(value: Value<T>) -> Self;
    fn to_value<T>(&self) -> Value<T>;
}
```

### `HostFunction`

Before:

```rust
trait HostFunction<Kind, Args, Rets> {
    fn to_raw(self) -> (NonNull<Func>, Option<NonNull<FuncEnv>>);
}
```

After:

```rust
trait HostFunction<Args, Rets, Kind, T> {
    fn function_body_ptr(self) -> *const VMFunctionBody;
}
```

The generic parameters of the `HostFunction` trait has been re-ordered
and a new one has been introduced: `T`. In the new API, it represents
the type of the environment; in this port, it should not be used (or
at worst, use `vm::Ctx`).

The `to_raw` method, aimed at internal using, has been replaced by
`function_body_ptr`. It is subject to another change, so please don't
use it.

### `ImportDescriptor`

Before:

```rust
struct ImportDescriptor {
    namespace: String,
}
```

After:

```rust
struct ImportDescriptor {
    module: String,
}
```

The `namespace` field has been renamed `module`.

### `ImportObject`

Before:

```rust
impl ImportObject {
    fn with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>;
    fn maybe_with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>;
}
```

After:

```rust
impl ImportObject {
    fn call_state_creator(&self) -> Option<(*mut c_void, fn(*mut c_void))>;
    fn get_export(&self, module: &str, name: &str) -> Option<Export>;
    fn clone_ref(&self) -> Self;
}
```

The `with_namespace` and `maybe_with_namespace` methods have been
removed.

The `call_state_creator` method is new, along with `get_export` and
`clone_ref`.

### `Instance`

Before:

```rust
impl Instance {
    fn load<T: Loader>(&self, loader: T) -> Result<T::Instance, T::Error>;
    fn fun<Args, Rets>(&self, name: &str) -> ResolveResult<Args, Rets, Wasm>;
    fn resolve_func(&self, name: &str) -> ResolveError<usize>;
    fn dyn_func(&self, name: &str) -> ResolveResult<DynFunc>;
    fn call(&self, name: &str, params: &[Value]) -> CallResult<Vec<Value>>;
    fn context(&self) -> &Ctx;
    fn context_mut(&mut self) -> &mut Ctx;
    fn exports(&self) -> ExportsIter;
    fn get_internal(&self, fields: &InternalField) -> u64;
    fn set_internal(&self, fields: &InternalField, value: u64);
}
```

After:

```rust
impl Instance {
   fn fun<Args, Rets>(&self, name: &str) -> Result<Func<Args, Rets>, ExportError>;
   fn resolve_func(&self, name: &str) -> Result<usize, ()>;
   fn dyn_func(&self, name: &str) -> Result<DynFunc, ExportError>;
   fn call(&self, name: &str, params: &[Value]) -> Result<Vec<Value>, Box<dyn Error>>;
   fn context(&self) -> Ref<Ctx>;
   fn context_mut(&mut self) -> RefMut<Ctx>;
   fn exports(&self) -> ExportsIterator<impl Iterator<Item = (&String, &Export)>>;
}
```

The `load`, `get_internal` and `set_internal` methods have been removed.

Some `Result`'s errors have changed: The `func` and `dyn_func` methods
return a `Result<…, ExportError>` instead of `ResolveResult<…>`; The
`resolve_func` method returns a `Result<usize, ()>` instead of
`ResolveError<usize>`; The `call` method returns a `Result<…, Box<dyn
Error>>` instead of `CallResult<…>`.

The `context` and `context_mut` methods respectively return a
`Ref<Ctx>` and `RefMut<Ctx>` instead of `&Ctx` and `&mut Ctx`.

The `exports` method returns an `ExportsIterator<impl Iterator<Item =
(&String, &Export)>>` instead of `ExportsIter`. That's basically the same.

### `Memory`

Before:

```rust
impl Memory {
    fn new(desc: MemoryDescriptor) -> Result<Self, CreationError>;
    fn grow(&self, delta: Pages) -> Result<Pages, GrowError>;
}

```

After:

```rust
impl Memory {
    fn new(desc: MemoryDescriptor) -> Result<Self, MemoryError>;
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError>;
}
```

Only the `Result`'s errors have changed between `new` and `grow` from
respectively `CreationError` and `GrowError` to a general
`MemoryError` type.

### `MemoryDescriptor`

Before

```rust
struct MemoryDescriptor {
    memory_type: MemoryType,
}
```

After:

```rust
struct MemoryDescriptor {}
```

The `memory_type` field has been removed.

### `MemoryType`

Before:

```rust
enum MemoryType {
    Static,
    SharedStatic,
}
```

After:

```rust
enum MemoryType {
    Static { bound: Pages },
}
```

The `SharedStatic` variant has been removed.

The `Static` variant is now a structure of type `Static { bound: Pages }`.

### `Module`

Before:

```rust
impl Module {
    fn instantiate(&self, import_object: &ImportObject) -> Result<Instance>;
    fn cache(&self) -> Result<Artifact, CacheError>;
    fn custom_sections(&self, key: impl AsRef<str>) -> Option<&[Vec<u8>]>
}
```

After:

```rust
impl Module {
    fn instantiate(&self, import_object: &ImportObject) -> Result<Instance, InstantiationError>;
    fn cache(&self) -> Result<Artifact, Infallible>;
    fn custom_sections(&self, key: impl AsRef<str>) -> Option<Vec<Vec<u8>>>;
}
```

The `Result`'s errors of `instantiate` and `cache` have changed
respectively from `Error` and `CacheError` to `InstantiationError` and
`Infallible`. For `cache`, it means that it will never fail. The
`Result` is kept to avoid a change in the API.

The `custom_sections` method returns an `Option<Vec<Vec<u8>>` instead
of `Option<&[Vec<u8>]>`.

### `ModuleInfo`

Before:

```rust
struct ModuleInfo {
    backend: String,
    custom_sections: HashMap<String, Vec<Vec<u8>>>,
    data_initializers: Vec<DataInitializer>,
    elem_initializers: Vec<TableInitializer>,
    em_symbol_map: Option<HashMap<u32, String>>,
    func_assoc: Map<FuncIndex, SigIndex>,
    generate_debug_info: bool,
    globals: Map<LocalGlobalIndex, GlobalInit>,
    imported_functions: Map<ImportedFuncIndex, ImportName>,
    imported_globals: Map<ImportedGlobalIndex, (ImportName, GlobalDescriptor)>,
    imported_memories: Map<ImportedMemoryIndex, (ImportName, MemoryDescriptor)>,
    imported_tables: Map<ImportedTableIndex, (ImportName, TableDescriptor)>,
    memories: Map<LocalMemoryIndex, MemoryDescripto>,
    name_table: StringTable<NameIndex>,
    namespace_table: StringTable<NamespaceIndex>,
    signatures: Map<SigIndex, FuncSig>,
    start_func: Option<FuncIndex>,
    tables: Map<LocalTableIndex, TableDescriptor>,
}
```

After:

```rust
struct ModuleInfo {
    custom_sections: IndexMap<String, CustomSectionIndex>,
    custom_sections_data: PrimaryMap<CustomSectionIndex, Arc<[u8]>>,
    func_names: HashMap<FunctionIndex, String>,
    functions: PrimaryMap<FunctionIndex, SignatureIndex>,
    global_initializers: PrimaryMap<LocalGlobalIndex, GlobalInit>,
    globals: PrimaryMap<GlobalIndex, GlobalType>,
    id: ModuleId,
    imports: IndexMap<(String, String, u32), ImportIndex>,
    memories: PrimaryMap<MemoryIndex, MemoryType>,
    name: Option<String>,
    num_imported_funcs: usize,
    num_imported_globals: usize,
    num_imported_memories: usize,
    num_imported_tables: usize,
    passive_data: HashMap<DataIndex, Arc<[u8]>>,
    passive_elements: HashMap<ElemIndex, Box<[FunctionIndex]>>,
    signatures: PrimaryMap<SignatureIndex, FunctionType>,
    start_func: Option<FunctionIndex>,
    table_elements: Vec<TableElements>,
    tables: PrimaryMap<TableIndex, TableType>,
}
```

We are not going to re-phrase the differences here, but clearly a lot
has changed.

### `Namespace`

Before:

```rust
impl Namespace {
    fn insert<S, E>(&mut self, name: S, export: E) -> Option<Box<dyn IsExport + Send>>;
}
```

After:

```rust
impl Namespace {
    fn insert<S, E>(&mut self, name: S, export: E);
}
```

The `insert` method no longer returns a value.

### `NativeWasmType`

Before:

```rust
trait NativeWasmType {
    const TYPE: Type;

    fn from_binary(bits: u64) -> Self;
    fn to_binary(self) -> u64;
}
```

After:

```rust
trait NativeWasmType {
    type Abi: Copy + std::fmt::Debug;
    const WASM_TYPE: Type;

    fn from_binary(binary: i128) -> Self;
    fn to_binary(self) -> i128;
    fn into_abi(self) -> Self::Abi;
    fn from_abi(abi: Self::Abi) -> Self;
    fn to_value<T>(self) -> Value<T>;
}
```

The `TYPE` constant has been renamed `WASM_TYPE`.

The `into_abi`, `from_abi` and `to_value` methods are new, in addition
to the `Abi` type.

The `to_binary` and `from_binary` methods now take a `i128` instead of
a `u64`.

### `Pages`

Before:

```rust
impl Pages {
    fn checked_add(self, rhs: Self) -> Result<Pages, PageError>;
}
```

After:

```rust
impl Pages {
    fn checked_add(self, rhs: Self) -> Option<Self>;
    const fn max_values() -> Self;
}
```

The `checked_add` method now returns an `Option<Self>` rather than a
`Result<Pages, PageError>`.

The constant `max_values` function has been introduced.

### `Table`

Before:

```rust
impl Table {
    fn new(desc: TableDescriptor) -> Result<Self, CreationError>;
    fn set<T: StorableInTable>(&self, index: u32, element: T) -> Result<(), TableAccessError>;
    fn grow(&self, delta: u32) -> Result<u32, GrowError>;
    fn vm_local_table(&mut self) -> *mut LocalTable;
}
```

After:

```rust
impl Table {
    fn new(desc: TableDescriptor, initial_value: Value) -> Result<Self, RuntimeError>;
    fn set(&self, index: u32, element: Value) -> Result<(), RuntimeError>;
    fn get(&self, index: u32) -> Option<Value>;
    fn grow(&self, delta: u32, initial_value: Value) -> Result<u32, RuntimeError>;
}
```

The `new` constructor takes an `initial_value` and returns a
`RuntimeError` in case of an error instead of a `CreationError`.

The `set` method takes a `Value` rather a `T: StorableInTable`. It
also returns a `RuntimeError` rather than a `TableAccessError` in case
of an error.

The `get` method is new!

The `grow` method takes an `initial_value`, just like `new`, and also
returns a `RuntimeError` rather than a `GrowError`.

Finally, the `vm_local_table` method has been removed. It was quite an
internal API, it's unlikely it will impact your project.

### `TableDescriptor`

Before:

```rust
struct TableDescriptor {
    element: ElementType,
}
```

After:

```rust
struct TableDescriptor {
    ty: Type,
}
```

The `element` field has been renamed `ty`.

A `new` constructor has been introduced: `fn new(ty: Type, minimum:
u32, maximum: Option<u32>) -> Self`.

### `WasmTypeList`

Before:

```rust
trait WasmTypeList {
    type RetArray: AsMut<[u64]>;

    fn from_ret_array(array: Self::RetArray) -> Self;
    fn empty_ret_array() -> Self::RetArray;
    fn types() -> &'static [Type];
    fn call<Rets>(self, f: NonNull<Func>, wasm: Wasm, ctx: *mut Ctx) -> Result<Rets, RuntimeError>;
}
```

After:

```rust
trait WasmTypeList {
    type Array: AsMut<[i128]>;

    fn from_array(array: Self::Array) -> Self;
    fn empty_array(self) -> Self::Array;
    fn wasm_types() -> &'static [Type];
    fn from_slice(slice: &[i128]) -> Result<Self, TryFromSliceError>;
    fn into_array(self) -> Self::Array;
}
```

The `RetArray` type has been renamed `Array`. The concrete type must
now implement the `AsMut<[i128]>` trait instead of `AsMut<[u64]>`.

`from_ret_array` and `empty_ret_array` have been renamed
accordingly like `from_array` and `empty_array`. Note that
`empty_array` is now a method instead of an associated function.

The `types` function has been renamed `wasm_types`.

The `call` method has been removed. It was part of an internal API,
it's unlikely it will break your code.

The `from_slice` constructor and the `into_array` method are new!

### `WasmHash`

Before:

```rust
impl WasmHash {
    fn decode(hex_str: &str) -> Result<Self, Error>;
}
```

After:

```rust
impl WasmHash {
    fn decode(hex_str: &str) -> Result<Self, DeserializeError>;
}
```

The `Result`'s error has changed from `Error` to `DeserializeError`
for the `decode` method.
