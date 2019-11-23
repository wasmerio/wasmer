use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent::*},
    module::ModuleInfo,
    vm::{Ctx, InternalField},
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
    Instance,
};

static INTERNAL_FIELD_USED: InternalField = InternalField::allocate();
static INTERNAL_FIELD_LIMIT: InternalField = InternalField::allocate();

/// Metering is a compiler middleware that calculates the cost of WebAssembly instructions at
/// compile time and will count the cost of executed instructions at runtime. Within the Metering
/// functionality, this instruction cost is called `points`.
///
/// Each instance has an `exec_limit` which is the maximum number of points which can be used by
/// the instance during a function call. If this limit is exceeded, the function call will trap.
/// Each instance has a `points_used` field which can be used to track points used during a
/// function call and should be set back to zero after a function call.
///
/// Each compiler backend with Metering enabled should produce the same cost used at runtime for
/// the same function calls so we can say that the metering is deterministic.
pub struct Metering {
    injections: Vec<Injection>,
    current_block_injections: Vec<Injection>,
    current_block_cost: u64,
}

impl Metering {
    pub fn new() -> Metering {
        Metering {
            injections: Vec::new(),
            current_block_injections: Vec::new(),
            current_block_cost: 0,
        }
    }

    fn set_costs<'a, 'b: 'a>(&mut self) {
        for inj in &mut self.current_block_injections {
            inj.check += self.current_block_cost;
        }
        // Set add of the last injection
        if self.current_block_injections.len() > 0 {
            let last_idx = self.current_block_injections.len() - 1;
            self.current_block_injections[last_idx] = Injection {
                add: self.current_block_cost,
                check: 0,
            };
        }
        self.current_block_cost = 0;
    }

    fn begin<'a, 'b: 'a>(&mut self) {
        self.set_costs();
        self.current_block_injections
            .push(Injection { add: 0, check: 0 });
    }
    fn end<'a, 'b: 'a>(&mut self) {
        self.set_costs();
        self.injections.append(&mut self.current_block_injections);
    }

    fn inject_metering<'a, 'b: 'a>(&self, sink: &mut EventSink<'a, 'b>) {
        let prev: Vec<Event> = sink.buffer.drain(..).collect();
        let mut inj_idx: usize = 1;
        for ev in prev {
            match ev {
                Event::Internal(FunctionBegin(_)) => {
                    sink.push(ev);
                    self.injections[0].inject(sink);
                }
                Event::Wasm(&ref op) | Event::WasmOwned(ref op) => match *op {
                    Operator::End
                    | Operator::If { .. }
                    | Operator::Else
                    | Operator::BrIf { .. }
                    | Operator::Loop { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. } => {
                        sink.push(ev);
                        self.injections[inj_idx].inject(sink);
                        inj_idx += 1;
                    }
                    _ => {
                        sink.push(ev);
                    }
                },
                _ => {
                    sink.push(ev);
                }
            }
        }
    }

    /// increment_cost adds 1 to the current_block_cost.
    ///
    /// Later this may be replaced with a cost map for assigning custom unique cost values to
    /// specific Operators.
    fn increment_cost<'a, 'b: 'a>(&mut self, ev: &Event<'a, 'b>) {
        match ev {
            Event::Internal(ref iev) => match iev {
                FunctionBegin(_) | FunctionEnd | Breakpoint(_) => {
                    return;
                }
                _ => {}
            },
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => match *op {
                Operator::Unreachable | Operator::End | Operator::Else => {
                    return;
                }
                _ => {}
            },
        }
        self.current_block_cost += 1;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ExecutionLimitExceededError;

impl FunctionMiddleware for Metering {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        ev: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        // This involves making two passes over an entire function. The first pass counts the costs
        // of each code segment. The final pass occurs when Event is FunctionEnd and we actually
        // drain the EventSink and repopulate it with metering code injected.
        match ev {
            Event::Internal(ref iev) => match iev {
                FunctionBegin(_) => {
                    self.injections.clear();
                    self.current_block_injections.clear();
                    self.current_block_cost = 0;
                    sink.push(ev);
                    self.begin();
                    return Ok(());
                }
                FunctionEnd => {
                    self.end();
                    self.inject_metering(sink);
                    sink.push(ev);
                    return Ok(());
                }
                _ => {
                    self.increment_cost(&ev);
                    sink.push(ev);
                    return Ok(());
                }
            },
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                self.increment_cost(&ev);
                match *op {
                    Operator::End
                    | Operator::If { .. }
                    | Operator::Else
                    | Operator::Br { .. }
                    | Operator::BrIf { .. }
                    | Operator::BrTable { .. }
                    | Operator::Unreachable
                    | Operator::Return => {
                        self.end();
                    }
                    _ => {}
                }
                match *op {
                    Operator::Loop { .. }
                    | Operator::End
                    | Operator::If { .. }
                    | Operator::Else
                    | Operator::BrIf { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. } => {
                        self.begin();
                    }
                    _ => {}
                }
            }
        }
        sink.push(ev);

        Ok(())
    }
}

/// Returns the number of points used by an Instance.
pub fn get_points_used(instance: &Instance) -> u64 {
    instance.get_internal(&INTERNAL_FIELD_USED)
}

/// Sets the number of points used by an Instance.
pub fn set_points_used(instance: &mut Instance, value: u64) {
    instance.set_internal(&INTERNAL_FIELD_USED, value);
}

/// Returns the number of points used in a Ctx.
pub fn get_points_used_ctx(ctx: &Ctx) -> u64 {
    ctx.get_internal(&INTERNAL_FIELD_USED)
}

/// Sets the number of points used in a Ctx.
pub fn set_points_used_ctx(ctx: &mut Ctx, value: u64) {
    ctx.set_internal(&INTERNAL_FIELD_USED, value);
}

pub fn set_execution_limit(instance: &mut Instance, limit: u64) {
    instance.set_internal(&INTERNAL_FIELD_LIMIT, limit);
}

pub fn set_execution_limit_ctx(ctx: &mut Ctx, limit: u64) {
    ctx.set_internal(&INTERNAL_FIELD_LIMIT, limit);
}

pub fn get_execution_limit(instance: &Instance) -> u64 {
    instance.get_internal(&INTERNAL_FIELD_LIMIT)
}

pub fn get_execution_limit_ctx(ctx: &Ctx) -> u64 {
    ctx.get_internal(&INTERNAL_FIELD_LIMIT)
}

/// Injection is a struct that stores the cost of the subsequent code segment. It injects metering
/// code into the EventSink.
///
/// Code segments may be nested such that multiple segments may begin at different places but all
/// end at the same branching instruction. Thus entering into one code segment guarantees that you
/// will proceed to the nested ones, until the first branching operator is reached. In these cases,
/// the limit check can be done such that we ensure enough gas to complete the entire code segment,
/// including nested parts. However it is important that we only add the cost up to the next
/// metering injection.
///
/// For example, consider the following
///
/// - INJECT METERING CODE (check to if, add cost to next INJECT)
/// | block
/// |    ... (non-branching ops)
/// |    loop
/// |    - INJECT METERING CODE (check to if, add to next INJECT)
/// |    |   ... (non-branching ops)
/// |    |   loop
/// |    |   - INJECT METERING CODE
/// |    |   |    ... (non-branching ops)
/// |____|___|___ if (first branching op)
#[derive(Debug)]
struct Injection {
    check: u64,
    add: u64,
}

impl Injection {
    fn inject<'a, 'b: 'a>(&self, sink: &mut EventSink<'a, 'b>) {
        if self.add == 0 {
            return;
        }
        // PUSH USED
        sink.push(Event::Internal(GetInternal(
            INTERNAL_FIELD_USED.index() as _
        )));

        // PUSH COST (to next Injection)
        sink.push(Event::WasmOwned(Operator::I64Const {
            value: self.add as i64,
        }));

        // USED + COST
        sink.push(Event::WasmOwned(Operator::I64Add));

        // SAVE USED
        sink.push(Event::Internal(SetInternal(
            INTERNAL_FIELD_USED.index() as _
        )));

        // PUSH USED
        sink.push(Event::Internal(GetInternal(
            INTERNAL_FIELD_USED.index() as _
        )));

        if self.check > 0 {
            // PUSH COST (to next branching op)
            sink.push(Event::WasmOwned(Operator::I64Const {
                value: self.check as i64,
            }));

            // USED + COST
            sink.push(Event::WasmOwned(Operator::I64Add));
        }

        // PUSH LIMIT
        sink.push(Event::Internal(GetInternal(
            INTERNAL_FIELD_LIMIT.index() as _
        )));

        // IF USED > LIMIT
        sink.push(Event::WasmOwned(Operator::I64GtU));
        sink.push(Event::WasmOwned(Operator::If {
            ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
        }));

        //          TRAP! EXECUTION LIMIT EXCEEDED
        sink.push(Event::Internal(Breakpoint(Box::new(|_| {
            Err(Box::new(ExecutionLimitExceededError))
        }))));

        // ENDIF
        sink.push(Event::WasmOwned(Operator::End));
    }
}
