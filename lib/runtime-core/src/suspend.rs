use crate::import::{ImportObject, Namespace};
use crate::trampoline::{CallContext, TrampolineBuffer, TrampolineBufferBuilder};
use crate::vm::Ctx;
use bincode::serialize;
use std::ffi::c_void;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;

pub struct SuspendConfig {
    pub image_path: String,
}

struct ImportContext {
    next: Option<(*mut c_void, fn(*mut c_void))>,
    trampolines: Rc<TrampolineBuffer>,
    config: Rc<SuspendConfig>,
}

impl ImportContext {
    fn new(trampolines: Rc<TrampolineBuffer>, config: Rc<SuspendConfig>) -> ImportContext {
        ImportContext {
            trampolines,
            next: None,
            config,
        }
    }
}

fn destroy_import_context(x: *mut c_void) {
    unsafe {
        let ctx = Box::from_raw(x as *mut ImportContext);
        if let Some(x) = ctx.next {
            (x.1)(x.0);
        }
    }
}

pub fn patch_import_object(x: &mut ImportObject, config: SuspendConfig) {
    let config = Rc::new(config);
    let mut builder = TrampolineBufferBuilder::new();

    let config_ptr: &SuspendConfig = &*config;
    let idx = builder.add_context_rsp_state_preserving_trampoline(
        suspend,
        config_ptr as *const SuspendConfig as *const CallContext,
    );
    let trampolines = builder.build();

    let suspend_indirect: fn(&mut Ctx) =
        unsafe { ::std::mem::transmute(trampolines.get_trampoline(idx)) };

    let trampolines = Rc::new(trampolines);

    // FIXME: Memory leak!
    ::std::mem::forget(ImportContext::new(trampolines.clone(), config.clone()));

    let mut ns = Namespace::new();
    ns.insert("suspend", func!(suspend_indirect));
    x.register("wasmer_suspend", ns);
}

unsafe extern "C" fn suspend(
    ctx: &mut Ctx,
    config_ptr_raw: *const CallContext,
    mut stack: *const u64,
) {
    use crate::state::x64::{build_instance_image, read_stack, X64Register, GPR};

    {
        let config = &*(config_ptr_raw as *const SuspendConfig);

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
        let image = build_instance_image(ctx, es_image);
        let image_bin = serialize(&image).unwrap();
        let mut f = File::create(&config.image_path).unwrap();
        f.write_all(&image_bin).unwrap();
    }

    ::std::process::exit(0);
}
