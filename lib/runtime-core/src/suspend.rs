use crate::alternative_stack::begin_unsafe_unwind;
use crate::import::{ImportObject, Namespace};
use crate::trampoline::{CallContext, TrampolineBuffer, TrampolineBufferBuilder};
use crate::vm::Ctx;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn set_interrupted(x: bool) {
    INTERRUPTED.store(x, Ordering::SeqCst);
}

pub fn get_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

pub fn get_and_reset_interrupted() -> bool {
    INTERRUPTED.swap(false, Ordering::SeqCst)
}

struct ImportContext {
    _trampolines: Rc<TrampolineBuffer>,
}

impl ImportContext {
    fn new(trampolines: Rc<TrampolineBuffer>) -> ImportContext {
        ImportContext {
            _trampolines: trampolines,
        }
    }
}

pub fn patch_import_object(x: &mut ImportObject) {
    let mut builder = TrampolineBufferBuilder::new();

    let idx_suspend =
        builder.add_context_rsp_state_preserving_trampoline(suspend, ::std::ptr::null());
    let idx_check_interrupt =
        builder.add_context_rsp_state_preserving_trampoline(check_interrupt, ::std::ptr::null());
    let trampolines = builder.build();

    let suspend_indirect: fn(&mut Ctx) =
        unsafe { ::std::mem::transmute(trampolines.get_trampoline(idx_suspend)) };
    let check_interrupt_indirect: fn(&mut Ctx) =
        unsafe { ::std::mem::transmute(trampolines.get_trampoline(idx_check_interrupt)) };

    let trampolines = Rc::new(trampolines);

    // FIXME: Memory leak!
    ::std::mem::forget(ImportContext::new(trampolines.clone()));

    let mut ns = Namespace::new();
    ns.insert("suspend", func!(suspend_indirect));
    ns.insert("check_interrupt", func!(check_interrupt_indirect));
    x.register("wasmer_suspend", ns);
}

#[allow(clippy::cast_ptr_alignment)]
unsafe extern "C" fn check_interrupt(ctx: &mut Ctx, _: *const CallContext, stack: *const u64) {
    if get_and_reset_interrupted() {
        do_suspend(ctx, stack);
    }
}

#[allow(clippy::cast_ptr_alignment)]
unsafe extern "C" fn suspend(ctx: &mut Ctx, _: *const CallContext, stack: *const u64) {
    do_suspend(ctx, stack);
}

unsafe fn do_suspend(ctx: &mut Ctx, mut stack: *const u64) -> ! {
    use crate::state::x64::{build_instance_image, read_stack, X64Register, GPR};

    let image = {
        let msm = (*ctx.module)
            .runnable_module
            .get_module_state_map()
            .unwrap();
        let code_base = (*ctx.module).runnable_module.get_code().unwrap().as_ptr() as usize;

        let mut known_registers: [Option<u64>; 24] = [None; 24];

        let r15 = *stack;
        let r14 = *stack.offset(1);
        let r13 = *stack.offset(2);
        let r12 = *stack.offset(3);
        let rbx = *stack.offset(4);
        stack = stack.offset(5);

        known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(r15);
        known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(r14);
        known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(r13);
        known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(r12);
        known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(rbx);

        let es_image = read_stack(&msm, code_base, stack, known_registers, None);

        {
            use colored::*;
            eprintln!("\n{}", "Suspending instance.".green().bold());
        }
        es_image.print_backtrace_if_needed();
        build_instance_image(ctx, es_image)
    };

    begin_unsafe_unwind(Box::new(image));
}
