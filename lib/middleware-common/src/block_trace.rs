use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    wasmparser::Operator,
};

pub struct BlockTrace {
    func_idx: usize,
    evt_idx: usize,
}

impl BlockTrace {
    pub fn new() -> BlockTrace {
        BlockTrace {
            func_idx: std::usize::MAX,
            evt_idx: 0,
        }
    }
}

impl FunctionMiddleware for BlockTrace {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                self.func_idx = self.func_idx.wrapping_add(1);
                self.evt_idx = 0;
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> enter_func % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            Event::Wasm(Operator::Call { .. }) => {
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> leave_call % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            Event::Wasm(Operator::Block { .. }) => {
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> block % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            Event::Wasm(Operator::Loop { .. }) => {
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> loop % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            Event::Wasm(Operator::If { .. }) => {
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> if % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            Event::Wasm(Operator::Else { .. }) => {
                let func_idx = self.func_idx;
                let evt_idx = self.evt_idx;
                sink.push(op);
                sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                    move |info| {
                        eprintln!(
                            "[BlockTrace] ({}, {}) -> else % {:?}",
                            func_idx,
                            evt_idx,
                            info.fault
                                .and_then(|x| unsafe { x.read_stack(Some(1)) })
                                .unwrap()
                                .frames[0]
                        );
                        Ok(())
                    },
                ))))
            }
            _ => {
                sink.push(op);
            }
        }
        self.evt_idx += 1;
        Ok(())
    }
}
