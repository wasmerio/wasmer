use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
};

pub struct CallTrace;

impl FunctionMiddleware for CallTrace {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Internal(InternalEvent::FunctionBegin(id)) => sink.push(Event::Internal(
                InternalEvent::Breakpoint(Box::new(move |_| {
                    eprintln!("func ({})", id);
                    Ok(())
                })),
            )),
            _ => {}
        }
        sink.push(op);
        Ok(())
    }
}
