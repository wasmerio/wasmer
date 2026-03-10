use std::fs::File;
use std::io::Write;

use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::InternalField,
    wasmparser::Operator,
    Instance,
};

static OPCODE_LAST_LOCATION: InternalField = InternalField::allocate();

pub struct OpcodeTracer {
    pub output_file: File,
    local_function_index: u32,
    counter: u32,
}

impl OpcodeTracer {
    pub fn new() -> OpcodeTracer {
        OpcodeTracer {
            output_file: File::create("opcode.trace").unwrap(),
            local_function_index: 0,
            counter: 0,
        }
    }

    fn trace_operator(&mut self, event: &Event) {
        match *event {
            Event::Internal(InternalEvent::FunctionBegin(local_function_index)) => {
                self.local_function_index = local_function_index;
                self.counter = 0;
            }
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                self.output_file
                    .write_all(
                        format!(
                            "[fn: {:08b}({}), operator: {:08b}({})]\t{:?}\n",
                            self.local_function_index,
                            self.local_function_index,
                            self.counter,
                            self.counter,
                            op
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                self.counter += 1;
            }
            _ => {}
        }
    }

    fn push_last_location_tracer(&self, sink: &mut EventSink, source_loc: u32) {
        sink.push(Event::WasmOwned(Operator::I64Const {
            value: source_loc as i64,
        }));
        sink.push(Event::Internal(InternalEvent::SetInternal(
            OPCODE_LAST_LOCATION.index() as _,
        )));
    }
}

impl FunctionMiddleware for OpcodeTracer {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
        source_loc: u32,
    ) -> Result<(), Self::Error> {
        self.trace_operator(&op);
        self.push_last_location_tracer(sink, source_loc);

        sink.push(op);

        Ok(())
    }
}

pub fn get_opcodetracer_last_location(instance: &mut Instance) -> u64 {
    instance.get_internal(&OPCODE_LAST_LOCATION)
}

pub fn reset_opcodetracer_last_location(instance: &mut Instance) {
    instance.set_internal(&OPCODE_LAST_LOCATION, 0);
}
