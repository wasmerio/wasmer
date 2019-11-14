use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::{Ctx, InternalField},
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
    Instance,
};

static INTERNAL_FIELD: InternalField = InternalField::allocate();

/// Metering is a compiler middleware that calculates the cost of WebAssembly instructions at compile
/// time and will count the cost of executed instructions at runtime. Within the Metering functionality,
/// this instruction cost is called `points`.
///
/// The Metering struct takes a `limit` parameter which is the maximum number of points which can be
/// used by an instance during a function call. If this limit is exceeded, the function call will
/// trap. Each instance has a `points_used` field which can be used to track points used during
/// a function call and should be set back to zero after a function call.
///
/// Each compiler backend with Metering enabled should produce the same cost used at runtime for
/// the same function calls so we can say that the metering is deterministic.
///
pub struct Metering {
    limit: u64,
    current_block: u64,
}

impl Metering {
    pub fn new(limit: u64) -> Metering {
        Metering {
            limit,
            current_block: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ExecutionLimitExceededError;

impl FunctionMiddleware for Metering {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                self.current_block = 0;
            }
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                self.current_block += 1;
                match *op {
                    Operator::Loop { .. }
                    | Operator::Block { .. }
                    | Operator::End
                    | Operator::If { .. }
                    | Operator::Else
                    | Operator::Unreachable
                    | Operator::Br { .. }
                    | Operator::BrTable { .. }
                    | Operator::BrIf { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. }
                    | Operator::Return => {
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64Const {
                            value: self.current_block as i64,
                        }));
                        sink.push(Event::WasmOwned(Operator::I64Add));
                        sink.push(Event::Internal(InternalEvent::SetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        self.current_block = 0;
                    }
                    _ => {}
                }
                match *op {
                    Operator::Br { .. }
                    | Operator::BrTable { .. }
                    | Operator::BrIf { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. } => {
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64Const {
                            value: self.limit as i64,
                        }));
                        sink.push(Event::WasmOwned(Operator::I64GeU));
                        sink.push(Event::WasmOwned(Operator::If {
                            ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
                        }));
                        sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(|_| {
                            Err(Box::new(ExecutionLimitExceededError))
                        }))));
                        sink.push(Event::WasmOwned(Operator::End));
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

/// Returns the number of points used by an Instance.
pub fn get_points_used(instance: &Instance) -> u64 {
    instance.get_internal(&INTERNAL_FIELD)
}

/// Sets the number of points used by an Instance.
pub fn set_points_used(instance: &mut Instance, value: u64) {
    instance.set_internal(&INTERNAL_FIELD, value);
}

/// Returns the number of points used in a Ctx.
pub fn get_points_used_ctx(ctx: &Ctx) -> u64 {
    ctx.get_internal(&INTERNAL_FIELD)
}

/// Sets the number of points used in a Ctx.
pub fn set_points_used_ctx(ctx: &mut Ctx, value: u64) {
    ctx.set_internal(&INTERNAL_FIELD, value);
}
