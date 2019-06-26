use crate::alternative_stack::begin_unsafe_unwind;
use crate::import::{ImportObject, Namespace};
use crate::trampoline::{CallContext, TrampolineBufferBuilder};
use crate::vm::Ctx;
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

pub fn patch_import_object(x: &mut ImportObject) {
    struct Intrinsics {
        suspend: fn(&mut Ctx),
        check_interrupt: fn(&mut Ctx),
    }

    lazy_static! {
        static ref INTRINSICS: Intrinsics = {
            let mut builder = TrampolineBufferBuilder::new();
            let idx_suspend =
                builder.add_context_rsp_state_preserving_trampoline(suspend, ::std::ptr::null());
            let idx_check_interrupt = builder
                .add_context_rsp_state_preserving_trampoline(check_interrupt, ::std::ptr::null());
            let trampolines = builder.build();

            let ret = Intrinsics {
                suspend: unsafe { ::std::mem::transmute(trampolines.get_trampoline(idx_suspend)) },
                check_interrupt: unsafe {
                    ::std::mem::transmute(trampolines.get_trampoline(idx_check_interrupt))
                },
            };
            ::std::mem::forget(trampolines);
            ret
        };
    }

    let mut ns = Namespace::new();

    let suspend_fn = INTRINSICS.suspend;
    let check_interrupt_fn = INTRINSICS.check_interrupt;

    ns.insert("suspend", func!(suspend_fn));
    ns.insert("check_interrupt", func!(check_interrupt_fn));
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
            eprintln!("{}", "Suspending instance.".green().bold());
        }
        build_instance_image(ctx, es_image)
    };

    begin_unsafe_unwind(Box::new(image));
}
