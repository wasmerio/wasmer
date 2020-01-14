use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
};

pub struct CallTrace {
    counter: Arc<AtomicU32>,
}

impl CallTrace {
    pub fn new() -> CallTrace {
        CallTrace {
            counter: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl FunctionMiddleware for CallTrace {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        let counter = self.counter.clone();

        match op {
            Event::Internal(InternalEvent::FunctionBegin(id)) => sink.push(Event::Internal(
                InternalEvent::Breakpoint(Box::new(move |_| {
                    let idx = counter.fetch_add(1, Ordering::SeqCst);
                    eprintln!("[{}] func ({})", idx, id);
                    Ok(())
                })),
            )),
            _ => {}
        }
        sink.push(op);
        Ok(())
    }
}
