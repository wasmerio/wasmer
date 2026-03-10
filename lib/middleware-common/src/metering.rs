use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::{Ctx, InternalField},
    wasmparser::{Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
    Instance,
};

use crate::metering_costs::{get_opcode_index, get_local_allocate_cost_index};
use crate::runtime_breakpoints::{push_runtime_breakpoint, BREAKPOINT_VALUE_OUT_OF_GAS};

static FIELD_USED_POINTS: InternalField = InternalField::allocate();
static FIELD_POINTS_LIMIT: InternalField = InternalField::allocate();

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

pub struct Metering<'a> {
    unmetered_locals: usize,
    current_block: u64,
    func_locals_costs: u32,
    opcode_costs: &'a [u32],
}

impl<'a> Metering<'a> {
    pub fn new(opcode_costs: &'a [u32], unmetered_locals: usize) -> Metering<'a> {
        Metering {
            unmetered_locals,
            current_block: 0,
            func_locals_costs: 0,
            opcode_costs,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ExecutionLimitExceededError;

impl<'q> FunctionMiddleware for Metering<'q> {
    type Error = String;

    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
        _source_loc: u32,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                self.current_block = self.func_locals_costs as u64;
            }
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                let opcode_index = get_opcode_index(op);
                self.current_block += self.opcode_costs[opcode_index] as u64;
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
                            FIELD_USED_POINTS.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64Const {
                            value: self.current_block as i64,
                        }));
                        sink.push(Event::WasmOwned(Operator::I64Add));
                        sink.push(Event::Internal(InternalEvent::SetInternal(
                            FIELD_USED_POINTS.index() as _,
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
                            FIELD_USED_POINTS.index() as _,
                        )));
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            FIELD_POINTS_LIMIT.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64GeU));
                        sink.push(Event::WasmOwned(Operator::If {
                            ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType),
                        }));
                        push_runtime_breakpoint(sink, BREAKPOINT_VALUE_OUT_OF_GAS);
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

    fn feed_local(
        &mut self,
        _ty: WpType,
        n: usize,
        _loc: u32,
    ) -> Result<(), Self::Error>{
        if n > self.unmetered_locals {
            let metered_locals = (n  - self.unmetered_locals) as u32;
            let cost_index = get_local_allocate_cost_index();
            let cost = self.opcode_costs[cost_index];
            // n is already limited by Wasmparser; the following casting and multiplication are
            // safe from overflowing
            self.func_locals_costs += cost * metered_locals;
        }
        Ok(())
    }
}

/// Returns the number of points used by an Instance.
pub fn get_points_used(instance: &Instance) -> u64 {
    instance.get_internal(&FIELD_USED_POINTS)
}

/// Sets the number of points used by an Instance.
pub fn set_points_used(instance: &mut Instance, value: u64) {
    instance.set_internal(&FIELD_USED_POINTS, value);
}

/// Sets the limit of points to be used by an Instance.
pub fn set_points_limit(instance: &mut Instance, value: u64) {
    instance.set_internal(&FIELD_POINTS_LIMIT, value);
}

/// Returns the number of points used in a Ctx.
pub fn get_points_used_ctx(ctx: &Ctx) -> u64 {
    ctx.get_internal(&FIELD_USED_POINTS)
}

/// Sets the number of points used in a Ctx.
pub fn set_points_used_ctx(ctx: &mut Ctx, value: u64) {
    ctx.set_internal(&FIELD_USED_POINTS, value);
}
