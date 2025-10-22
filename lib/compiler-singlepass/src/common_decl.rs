#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RegisterIndex(pub usize);

/// Whether a value is determined at compile-time or run-time.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum WasmAbstractValue {
    /// This value is only known at runtime.
    Runtime,
    /// A constant value.
    Const(u64),
}

/// A container for the state of a running wasm instance.
#[derive(Clone, Debug)]
pub struct MachineState {
    /// Stack values.
    pub stack_values: Vec<MachineValue>,
    /// Register values.
    pub register_values: Vec<MachineValue>,
    /// Wasm stack.
    pub wasm_stack: Vec<WasmAbstractValue>,
    /// Wasm instruction offset.
    pub wasm_inst_offset: usize,
}

/// A kind of machine value.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum MachineValue {
    /// Undefined.
    Undefined,
    /// Vmctx.
    Vmctx,
    /// Vmctx Deref.
    _VmctxDeref(Vec<usize>),
    /// Preserve Register.
    PreserveRegister(RegisterIndex),
    /// Copy Stack BP Relative.
    CopyStackBPRelative(i32), // relative to Base Pointer, in byte offset
    /// Explicit Shadow.
    ExplicitShadow, // indicates that all values above this are above the shadow region
    /// Wasm Stack.
    WasmStack(usize),
    /// Wasm Local.
    WasmLocal(usize),
    /// Two Halves.
    _TwoHalves(Box<(MachineValue, MachineValue)>), // 32-bit values. TODO: optimize: add another type for inner "half" value to avoid boxing?
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
}
