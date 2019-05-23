use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::InternalField,
    wasmparser::{Operator, Type as WpType},
};

static INTERNAL_FIELD: InternalField = InternalField::allocate();

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
                            ty: WpType::EmptyBlockType,
                        }));
                        sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                            move |ctx| {
                                eprintln!("execution limit reached");
                                unsafe {
                                    (ctx.throw)();
                                }
                            },
                        ))));
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
