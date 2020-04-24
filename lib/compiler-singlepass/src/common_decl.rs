use std::collections::BTreeMap;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RegisterIndex(pub usize);

/// Information of an inline breakpoint.
///
/// TODO: Move this into runtime.
#[derive(Clone, Debug)]
pub struct InlineBreakpoint {
    /// Size in bytes taken by this breakpoint's instruction sequence.
    pub size: usize,

    /// Type of the inline breakpoint.
    pub ty: InlineBreakpointType,
}

/// The type of an inline breakpoint.
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum InlineBreakpointType {
    /// A middleware invocation breakpoint.
    Middleware,
}

/// A kind of wasm or constant value
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum WasmAbstractValue {
    /// A wasm runtime value
    Runtime,
    /// A wasm constant value
    Const(u64),
}

/// A container for the state of a running wasm instance.
#[derive(Clone, Debug)]
pub struct MachineState {
    /// Stack values.
    pub stack_values: Vec<MachineValue>,
    /// Register values.
    pub register_values: Vec<MachineValue>,
    /// Previous frame.
    pub prev_frame: BTreeMap<usize, MachineValue>,
    /// Wasm stack.
    pub wasm_stack: Vec<WasmAbstractValue>,
    /// Private depth of the wasm stack.
    pub wasm_stack_private_depth: usize,
    /// Wasm instruction offset.
    pub wasm_inst_offset: usize,
}

/// A diff of two `MachineState`s.
#[derive(Clone, Debug, Default)]
pub struct MachineStateDiff {
    /// Last.
    pub last: Option<usize>,
    /// Stack push.
    pub stack_push: Vec<MachineValue>,
    /// Stack pop.
    pub stack_pop: usize,

    /// Register diff.
    pub reg_diff: Vec<(RegisterIndex, MachineValue)>,

    /// Previous frame diff.
    pub prev_frame_diff: BTreeMap<usize, Option<MachineValue>>, // None for removal

    /// Wasm stack push.
    pub wasm_stack_push: Vec<WasmAbstractValue>,
    /// Wasm stack pop.
    pub wasm_stack_pop: usize,
    /// Private depth of the wasm stack.
    pub wasm_stack_private_depth: usize, // absolute value; not a diff.
    /// Wasm instruction offset.
    pub wasm_inst_offset: usize, // absolute value; not a diff.
}

/// A kind of machine value.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum MachineValue {
    /// Undefined.
    Undefined,
    /// Vmctx.
    Vmctx,
    /// Vmctx Deref.
    VmctxDeref(Vec<usize>),
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
    TwoHalves(Box<(MachineValue, MachineValue)>), // 32-bit values. TODO: optimize: add another type for inner "half" value to avoid boxing?
}

/// A map of function states.
#[derive(Clone, Debug)]
pub struct FunctionStateMap {
    /// Initial.
    pub initial: MachineState,
    /// Local Function Id.
    pub local_function_id: usize,
    /// Locals.
    pub locals: Vec<WasmAbstractValue>,
    /// Shadow size.
    pub shadow_size: usize, // for single-pass backend, 32 bytes on x86-64
    /// Diffs.
    pub diffs: Vec<MachineStateDiff>,
    /// Wasm Function Header target offset.
    pub wasm_function_header_target_offset: Option<SuspendOffset>,
    /// Wasm offset to target offset
    pub wasm_offset_to_target_offset: BTreeMap<usize, SuspendOffset>,
    /// Loop offsets.
    pub loop_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
    /// Call offsets.
    pub call_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
    /// Trappable offsets.
    pub trappable_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
}

/// A kind of suspend offset.
#[derive(Clone, Copy, Debug)]
pub enum SuspendOffset {
    /// A loop.
    Loop(usize),
    /// A call.
    Call(usize),
    /// A trappable.
    Trappable(usize),
}

/// Info for an offset.
#[derive(Clone, Debug)]
pub struct OffsetInfo {
    /// End offset.
    pub end_offset: usize, // excluded bound
    /// Diff Id.
    pub diff_id: usize,
    /// Activate offset.
    pub activate_offset: usize,
}

/// A map of module state.
#[derive(Clone, Debug)]
pub struct ModuleStateMap {
    /// Local functions.
    pub local_functions: BTreeMap<usize, FunctionStateMap>,
    /// Total size.
    pub total_size: usize,
}
