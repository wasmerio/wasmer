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
    prev_stack_depth: usize,
}

/// Configuration for StackLimit.
#[derive(Clone, Debug)]
pub struct StackLimitConfig {
    pub max_call_depth: Option<usize>,
    pub max_value_stack_depth: Option<usize>,
    pub max_static_slot_count: Option<usize>,
}

impl StackLimit {
    pub fn new(config: StackLimitConfig) -> StackLimit {
        StackLimit {
            config,
            current_static_slots: None,
            prev_stack_depth: 0,
        }
    }
}

/// Emits a sequence into `sink` that adds `delta` to `field` and checks whether the value after
/// addition exceeds `limit`.
/// `sub_prev` specifies whether or not to first substract a value from `field` before adding
/// and checking.
fn emit_limit_check<'a, 'b, E: Copy + Clone + Send + Sync + Debug + 'static>(
    sink: &mut EventSink<'a, 'b>,
    field: &InternalField,
    delta: usize,
    limit: usize,
    err: E,
    sub_prev: Option<usize>,
) {
    if delta == 0 && (sub_prev.is_none() || sub_prev.unwrap() == 0) {
        return;
    }

    sink.push(Event::Internal(InternalEvent::GetInternal(
        field.index() as _
    )));

    if let Some(sub_prev) = sub_prev {
        if sub_prev != 0 {
            sink.push(Event::WasmOwned(Operator::I64Const {
                value: sub_prev as _,
            }));
            sink.push(Event::WasmOwned(Operator::I64Sub));
        }
    }

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
        match op {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                println!("begin");
                self.current_static_slots = None;
                if let Some(limit) = self.config.max_call_depth {
                    emit_limit_check(
                        sink,
                        &FIELD_CALL_DEPTH,
                        1,
                        limit,
                        CallDepthExceededError,
                        None,
                    );
                }
            }
            Event::Internal(InternalEvent::FunctionStaticSlotCount(count)) => {
                println!("ss count = {}", count);
                self.current_static_slots = Some(count);
                if let Some(limit) = self.config.max_static_slot_count {
                    emit_limit_check(
                        sink,
                        &FIELD_STATIC_SLOTS,
                        count,
                        limit,
                        StaticSlotLimitExceededError,
                        None,
                    );
                }
            }
            Event::Internal(InternalEvent::ValueStackGrow(new_depth)) => {
                if let Some(limit) = self.config.max_value_stack_depth {
                    emit_limit_check(
                        sink,
                        &FIELD_STACK_DEPTH,
                        new_depth,
                        limit,
                        StackDepthExceededError,
                        Some(self.prev_stack_depth),
                    );
                    self.prev_stack_depth = new_depth;
                }
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
                if let Some(_) = self.config.max_value_stack_depth {
                    emit_limit_resume(sink, &FIELD_STACK_DEPTH, self.prev_stack_depth);
                    self.prev_stack_depth = 0;
                }
            }
            _ => {}
        }
        sink.push(op);
        Ok(())
    }
}
