use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::InternalField,
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
    Instance,
};

use crate::runtime_breakpoints::{push_runtime_breakpoint, BREAKPOINT_VALUE_MEMORY_LIMIT};

static FIELD_MEMORY_GROW_COUNT: InternalField = InternalField::allocate();

static FIELD_OPERAND_BACKUP: InternalField = InternalField::allocate();

pub struct OpcodeControl {
    pub max_memory_grow: usize,
    pub max_memory_grow_delta: usize,
}

impl OpcodeControl {
    pub fn new(max_memory_grow: usize, max_memory_grow_delta: usize) -> OpcodeControl {
        OpcodeControl {
            max_memory_grow,
            max_memory_grow_delta,
        }
    }

    fn inject_memory_grow_count_limit(&mut self, sink: &mut EventSink) {
        sink.push(Event::Internal(InternalEvent::GetInternal(
            FIELD_MEMORY_GROW_COUNT.index() as _,
        )));
        sink.push(Event::WasmOwned(Operator::I64Const {
            value: self.max_memory_grow as i64,
        }));
        sink.push(Event::WasmOwned(Operator::I64GeU));
        sink.push(Event::WasmOwned(Operator::If {
            ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
        }));
        push_runtime_breakpoint(sink, BREAKPOINT_VALUE_MEMORY_LIMIT);
        sink.push(Event::WasmOwned(Operator::End));
    }

    fn inject_memory_grow_count_increment(&mut self, sink: &mut EventSink) {
        sink.push(Event::Internal(InternalEvent::GetInternal(
            FIELD_MEMORY_GROW_COUNT.index() as _,
        )));
        sink.push(Event::WasmOwned(Operator::I64Const { value: 1 as i64 }));
        sink.push(Event::WasmOwned(Operator::I64Add));
        sink.push(Event::Internal(InternalEvent::SetInternal(
            FIELD_MEMORY_GROW_COUNT.index() as _,
        )));
    }

    fn inject_memory_grow_delta_limit(&mut self, sink: &mut EventSink) {
        sink.push(Event::Internal(InternalEvent::GetInternal(
            FIELD_OPERAND_BACKUP.index() as _,
        )));
        sink.push(Event::WasmOwned(Operator::I64Const {
            value: self.max_memory_grow_delta as i64,
        }));
        sink.push(Event::WasmOwned(Operator::I64GtU));
        sink.push(Event::WasmOwned(Operator::If {
            ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
        }));
        push_runtime_breakpoint(sink, BREAKPOINT_VALUE_MEMORY_LIMIT);
        sink.push(Event::WasmOwned(Operator::End));
    }
}

impl FunctionMiddleware for OpcodeControl {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
        _: u32,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                match *op {
                    Operator::MemoryGrow { reserved } => {
                        if reserved != 0 {
                            return Err("MemoryGrow must have memory index 0".to_string());
                        }

                        // Before attempting anything with memory.grow, the current memory.grow
                        // count is checked against the self.max_memory_grow limit.
                        self.inject_memory_grow_count_limit(sink);
                        self.inject_memory_grow_count_increment(sink);

                        // Backup the top of the stack (the parameter for memory.grow) in order to
                        // duplicate it: once for the comparison against max_memory_grow_delta and
                        // again for memory.grow itself, assuming the comparison passes.
                        sink.push(Event::Internal(InternalEvent::SetInternal(
                            FIELD_OPERAND_BACKUP.index() as _,
                        )));

                        // Set up the comparison against max_memory_grow_delta.
                        self.inject_memory_grow_delta_limit(sink);

                        // Bring back the backed-up operand for memory.grow.
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            FIELD_OPERAND_BACKUP.index() as _,
                        )));
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        sink.push(op);
        Ok(())
    }
}

/// Set internal field `FIELD_MEMORY_GROW_COUNT` to 0.
pub fn reset_memory_grow_count(instance: &mut Instance) {
    instance.set_internal(&FIELD_MEMORY_GROW_COUNT, 0);
}
