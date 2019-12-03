use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::InternalField,
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
};

static FIELD_STATIC_SLOTS: InternalField = InternalField::allocate();
static FIELD_STACK_DEPTH: InternalField = InternalField::allocate();

#[derive(Copy, Clone, Debug)]
pub struct StaticSlotLimitExceededError;

#[derive(Copy, Clone, Debug)]
pub struct StackDepthExceededError;

/// StackLimit is a compiler middleware that deterministically limit the size
/// of virtual stack each module is allowed to use.
pub struct StackLimit {
    config: StackLimitConfig,
    current_static_slots: Option<usize>,
    prev_stack_depth: usize,
}

pub struct StackLimitConfig {
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
                self.current_static_slots = None;
            }
            Event::Internal(InternalEvent::FunctionStaticSlotCount(count)) => {
                self.current_static_slots = Some(count);
                if let Some(limit) = self.config.max_static_slot_count {
                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STATIC_SLOTS.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: count as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Add));
                    sink.push(Event::Internal(InternalEvent::SetInternal(
                        FIELD_STATIC_SLOTS.index() as _,
                    )));

                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STATIC_SLOTS.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: limit as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64GtU));
                    sink.push(Event::WasmOwned(Operator::If {
                        ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
                    }));
                    sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(|_| {
                        Err(Box::new(StaticSlotLimitExceededError))
                    }))));
                    sink.push(Event::WasmOwned(Operator::End));
                }
            }
            Event::Internal(InternalEvent::ValueStackGrow(new_depth)) => {
                if let Some(limit) = self.config.max_value_stack_depth {
                    // Update.
                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: self.prev_stack_depth as _,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Sub));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: new_depth as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Add));
                    sink.push(Event::Internal(InternalEvent::SetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));

                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: limit as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64GtU));
                    sink.push(Event::WasmOwned(Operator::If {
                        ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
                    }));
                    sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(|_| {
                        Err(Box::new(StackDepthExceededError))
                    }))));
                    sink.push(Event::WasmOwned(Operator::End));

                    self.prev_stack_depth = new_depth;
                }
            }
            Event::Internal(InternalEvent::FunctionEnd) => {
                if let Some(_) = self.config.max_static_slot_count {
                    // Resume the previous static slot count.
                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STATIC_SLOTS.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: self.current_static_slots.unwrap() as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Sub));
                    sink.push(Event::Internal(InternalEvent::SetInternal(
                        FIELD_STATIC_SLOTS.index() as _,
                    )));

                    self.current_static_slots = None;
                }
                if let Some(_) = self.config.max_value_stack_depth {
                    // Resume the previous stack depth.
                    sink.push(Event::Internal(InternalEvent::GetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));
                    sink.push(Event::WasmOwned(Operator::I64Const {
                        value: self.prev_stack_depth as i64,
                    }));
                    sink.push(Event::WasmOwned(Operator::I64Sub));
                    sink.push(Event::Internal(InternalEvent::SetInternal(
                        FIELD_STACK_DEPTH.index() as _,
                    )));
                    self.prev_stack_depth = 0;
                }
            }
            _ => {}
        }
        sink.push(op);
        Ok(())
    }
}
