use std::fmt::Debug;
use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::InternalField,
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
};

static FIELD_CALL_DEPTH: InternalField = InternalField::allocate();
static FIELD_STATIC_SLOTS: InternalField = InternalField::allocate();
static FIELD_STACK_DEPTH: InternalField = InternalField::allocate();

/// Error generated when the total number of call frames exceeded the limit.
#[derive(Copy, Clone, Debug)]
pub struct CallDepthExceededError;

/// Error generated when the total number of "static slots" (locals + arguments) across
/// the whole call stack exceeded the limit.
#[derive(Copy, Clone, Debug)]
pub struct StaticSlotLimitExceededError;

/// Error generated when the total depth of value stacks across the whole call stack
/// exceeded the limit.
#[derive(Copy, Clone, Debug)]
pub struct StackDepthExceededError;

/// StackLimit is a compiler middleware that deterministically limit the size
/// of virtual stack each module is allowed to use.
pub struct StackLimit {
    config: StackLimitConfig,
    current_static_slots: Option<usize>,
    last_function_local_stack_depth: usize,
}

/// Configuration for StackLimit.
#[derive(Clone, Debug)]
pub struct StackLimitConfig {
    /// Maximum number of call frames allowed.
    pub max_call_depth: Option<usize>,
    /// Maximum value stack depth across all stack frames.
    /// Note that this is currently an approximation. Consistency is guaranteed
    /// but this value does not precisely correspond to the real value stack depth.
    pub max_value_stack_depth: Option<usize>,
    /// Maximum number of static slots (arguments + locals) allowed across all stack frames.
    pub max_static_slot_count: Option<usize>,
}

impl StackLimit {
    pub fn new(config: StackLimitConfig) -> StackLimit {
        StackLimit {
            config,
            current_static_slots: None,
            last_function_local_stack_depth: 0,
        }
    }
}

/// Emits a sequence into `sink` that adds `delta` to `field` and checks whether the value after
/// addition exceeds `limit`.
fn emit_limit_check<'a, 'b, E: Copy + Clone + Send + Sync + Debug + 'static>(
    sink: &mut EventSink<'a, 'b>,
    field: &InternalField,
    delta: usize,
    limit: usize,
    err: E,
) {
    if delta == 0 {
        return;
    }

    sink.push(Event::Internal(InternalEvent::GetInternal(
        field.index() as _
    )));

    if delta != 0 {
        sink.push(Event::WasmOwned(Operator::I64Const {
            value: delta as i64,
        }));
        sink.push(Event::WasmOwned(Operator::I64Add));
    }

    sink.push(Event::Internal(InternalEvent::SetInternal(
        field.index() as _
    )));

    sink.push(Event::Internal(InternalEvent::GetInternal(
        field.index() as _
    )));
    sink.push(Event::WasmOwned(Operator::I64Const {
        value: limit as i64,
    }));
    sink.push(Event::WasmOwned(Operator::I64GtU));
    sink.push(Event::WasmOwned(Operator::If {
        ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
    }));
    sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
        move |_| Err(Box::new(err)),
    ))));
    sink.push(Event::WasmOwned(Operator::End));
}

/// Resumes the field previously updated by `emit_limit_check`.
fn emit_limit_resume<'a, 'b>(sink: &mut EventSink<'a, 'b>, field: &InternalField, delta: usize) {
    if delta == 0 {
        return;
    }

    sink.push(Event::Internal(InternalEvent::GetInternal(
        field.index() as _
    )));
    sink.push(Event::WasmOwned(Operator::I64Const {
        value: delta as i64,
    }));
    sink.push(Event::WasmOwned(Operator::I64Sub));
    sink.push(Event::Internal(InternalEvent::SetInternal(
        field.index() as _
    )));
}

impl FunctionMiddleware for StackLimit {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        let mut op = Some(op);
        match *op.as_ref().unwrap() {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                self.current_static_slots = None;
                self.last_function_local_stack_depth = 0;
                if let Some(limit) = self.config.max_call_depth {
                    emit_limit_check(sink, &FIELD_CALL_DEPTH, 1, limit, CallDepthExceededError);
                }
            }
            Event::Internal(InternalEvent::FunctionStaticSlotCount(count)) => {
                self.current_static_slots = Some(count);
                if let Some(limit) = self.config.max_static_slot_count {
                    emit_limit_check(
                        sink,
                        &FIELD_STATIC_SLOTS,
                        count,
                        limit,
                        StaticSlotLimitExceededError,
                    );
                }
            }
            Event::Internal(InternalEvent::ValueStackGrow(new_depth)) => {
                // Check but do not writeback just yet.
                // FIELD_STACK_DEPTH happens only at function calls.
                if let Some(limit) = self.config.max_value_stack_depth {
                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: new_depth as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Add));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: limit as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64GtU));
                    sink.push(Event::WasmOwned(Operator::If {
                        ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
                    }));
                    sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                        move |_| Err(Box::new(StackDepthExceededError)),
                    ))));
                    sink.push(Event::WasmOwned(Operator::End));
                }

                self.last_function_local_stack_depth = new_depth;
            }
            Event::Internal(InternalEvent::FunctionEnd) => {
                if let Some(_) = self.config.max_call_depth {
                    emit_limit_resume(sink, &FIELD_CALL_DEPTH, 1);
                }
                if let Some(_) = self.config.max_static_slot_count {
                    emit_limit_resume(
                        sink,
                        &FIELD_STATIC_SLOTS,
                        self.current_static_slots.unwrap(),
                    );
                    self.current_static_slots = None;
                }
            }
            Event::Wasm(Operator::Call { .. }) | Event::Wasm(Operator::CallIndirect { .. }) => {
                if let Some(limit) = self.config.max_value_stack_depth {
                    // Stack sizes are statically determined and therefore it's safe to
                    // check against the static previous stack depth here.
                    if self.last_function_local_stack_depth > 0 {
                        emit_limit_check(
                            sink,
                            &FIELD_STACK_DEPTH,
                            self.last_function_local_stack_depth,
                            limit,
                            StackDepthExceededError,
                        );
                        sink.push(op.take().unwrap());
                        emit_limit_resume(
                            sink,
                            &FIELD_STACK_DEPTH,
                            self.last_function_local_stack_depth,
                        );
                    }
                }
            }
            _ => {}
        }
        if let Some(op) = op {
            sink.push(op);
        }
        Ok(())
    }
}
