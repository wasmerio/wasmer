#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
// This allow attribute is ignored when placed directly on fields that also
// have a #[wasmer(...)] attribute. As a dirty workaround it is for now
// allowed for the whole library.
#![allow(clippy::type_complexity)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

#[macro_use]
extern crate log;

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::f64;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use wasmer::{
    imports, namespace, AsStoreMut, Exports, Function, FunctionEnvMut, FunctionType, Global,
    Imports, Instance, Memory, MemoryType, Module, Pages, RuntimeError, Table, TableType,
    TypedFunction, Value, WasmPtr,
};
use wasmer_types::Type as ValType;

#[cfg(unix)]
use ::libc::DIR as LibcDir;

// We use a placeholder for windows
#[cfg(not(unix))]
type LibcDir = u8;

#[macro_use]
mod macros;

// EMSCRIPTEN APIS
mod bitwise;
mod emscripten_target;
mod env;
mod errno;
mod exception;
mod exec;
mod exit;
mod inet;
mod io;
mod jmp;
mod libc;
mod linking;
mod lock;
mod math;
mod memory;
mod process;
mod pthread;
mod signal;
mod storage;
mod syscalls;
mod time;
mod ucontext;
mod unistd;
mod utils;
mod varargs;

pub use self::storage::{align_memory, static_alloc};
pub use self::utils::{
    allocate_cstr_on_stack, allocate_on_stack, get_emscripten_memory_size, get_emscripten_metadata,
    get_emscripten_table_size, is_emscripten_module,
};

#[derive(Clone)]
/// The environment provided to the Emscripten imports.
pub struct EmEnv {
    memory: Arc<RwLock<Option<Memory>>>,
    data: Arc<Mutex<Option<EmscriptenData>>>,
    funcs: Arc<Mutex<EmscriptenFunctions>>,
}

impl Default for EmEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl EmEnv {
    /// Create a new EmEnv, with default value to be set later (set_memory, set_functions and set_data)
    pub fn new() -> Self {
        Self {
            memory: Arc::new(RwLock::new(None)),
            data: Arc::new(Mutex::new(None)),
            funcs: Arc::new(Mutex::new(EmscriptenFunctions::new())),
        }
    }

    pub fn set_memory(&mut self, memory: Memory) {
        let mut w = self.memory.write().unwrap();
        *w = Some(memory);
    }

    /// Get a reference to the memory
    pub fn memory(&self, _mem_idx: u32) -> Memory {
        (&*self.memory.read().unwrap()).as_ref().cloned().unwrap()
    }

    pub fn set_functions(&mut self, funcs: EmscriptenFunctions) {
        self.funcs = Arc::new(Mutex::new(funcs));
    }

    pub fn set_data(
        &mut self,
        data: &EmscriptenGlobalsData,
        mapped_dirs: HashMap<String, PathBuf>,
    ) {
        let mut w = self.data.lock().unwrap();
        *w = Some(EmscriptenData::new(data.clone(), mapped_dirs));
    }
}

#[derive(Debug, Clone)]
pub struct LibcDirWrapper(pub *mut LibcDir);

impl std::ops::Deref for LibcDirWrapper {
    type Target = *mut LibcDir;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl Send for LibcDirWrapper {}
unsafe impl Sync for LibcDirWrapper {}

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5_242_880;
// TODO: make this variable
const STATIC_BUMP: u32 = 215_536;

lazy_static! {
    static ref OLD_ABORT_ON_CANNOT_GROW_MEMORY_SIG: FunctionType =
        FunctionType::new(vec![], vec![ValType::I32]);
}

// The address globals begin at. Very low in memory, for code size and optimization opportunities.
// Above 0 is static memory, starting with globals.
// Then the stack.
// Then 'dynamic' memory for sbrk.
const GLOBAL_BASE: u32 = 1024;
const STATIC_BASE: u32 = GLOBAL_BASE;

#[derive(Clone, Default)]
pub struct EmscriptenFunctions {
    pub malloc: Option<TypedFunction<u32, u32>>,
    pub free: Option<TypedFunction<u32, ()>>,
    pub memalign: Option<TypedFunction<(u32, u32), u32>>,
    pub memset: Option<TypedFunction<(u32, u32, u32), u32>>,
    pub stack_alloc: Option<TypedFunction<u32, u32>>,

    pub dyn_call_i: Option<TypedFunction<i32, i32>>,
    pub dyn_call_ii: Option<TypedFunction<(i32, i32), i32>>,
    pub dyn_call_iii: Option<TypedFunction<(i32, i32, i32), i32>>,
    pub dyn_call_iiii: Option<TypedFunction<(i32, i32, i32, i32), i32>>,
    pub dyn_call_iifi: Option<TypedFunction<(i32, i32, f64, i32), i32>>,
    pub dyn_call_v: Option<TypedFunction<i32, ()>>,
    pub dyn_call_vi: Option<TypedFunction<(i32, i32), ()>>,
    pub dyn_call_vii: Option<TypedFunction<(i32, i32, i32), ()>>,
    pub dyn_call_viii: Option<TypedFunction<(i32, i32, i32, i32), ()>>,
    pub dyn_call_viiii: Option<TypedFunction<(i32, i32, i32, i32, i32), ()>>,

    // round 2
    pub dyn_call_dii: Option<TypedFunction<(i32, i32, i32), f64>>,
    pub dyn_call_diiii: Option<TypedFunction<(i32, i32, i32, i32, i32), f64>>,
    pub dyn_call_iiiii: Option<TypedFunction<(i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_vd: Option<TypedFunction<(i32, f64), ()>>,
    pub dyn_call_viiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiiiiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiiiiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_iij: Option<TypedFunction<(i32, i32, i32, i32), i32>>,
    pub dyn_call_iji: Option<TypedFunction<(i32, i32, i32, i32), i32>>,
    pub dyn_call_iiji: Option<TypedFunction<(i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiijj: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_j: Option<TypedFunction<i32, i32>>,
    pub dyn_call_ji: Option<TypedFunction<(i32, i32), i32>>,
    pub dyn_call_jii: Option<TypedFunction<(i32, i32, i32), i32>>,
    pub dyn_call_jij: Option<TypedFunction<(i32, i32, i32, i32), i32>>,
    pub dyn_call_jjj: Option<TypedFunction<(i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_viiij: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiijiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiijiiiiii:
        Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viij: Option<TypedFunction<(i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viiji: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viijiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viijj: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_vj: Option<TypedFunction<(i32, i32, i32), ()>>,
    pub dyn_call_vjji: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_vij: Option<TypedFunction<(i32, i32, i32, i32), ()>>,
    pub dyn_call_viji: Option<TypedFunction<(i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_vijiii: Option<TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_vijj: Option<TypedFunction<(i32, i32, i32, i32, i32, i32), ()>>,
    pub dyn_call_viid: Option<TypedFunction<(i32, i32, i32, f64), ()>>,
    pub dyn_call_vidd: Option<TypedFunction<(i32, i32, f64, f64), ()>>,
    pub dyn_call_viidii: Option<TypedFunction<(i32, i32, i32, f64, i32, i32), ()>>,
    pub dyn_call_viidddddddd:
        Option<TypedFunction<(i32, i32, i32, f64, f64, f64, f64, f64, f64, f64, f64), ()>>,

    pub stack_save: Option<TypedFunction<(), i32>>,
    pub stack_restore: Option<TypedFunction<i32, ()>>,
    pub set_threw: Option<TypedFunction<(i32, i32), ()>>,
}

#[derive(Clone, Default)]
pub struct EmscriptenData {
    pub globals: EmscriptenGlobalsData,

    pub jumps: Arc<Mutex<Vec<[u32; 27]>>>,
    pub opened_dirs: HashMap<i32, Box<LibcDirWrapper>>,

    pub temp_ret_0: i32,

    pub mapped_dirs: HashMap<String, PathBuf>,
}

impl EmscriptenData {
    pub fn new(
        globals: EmscriptenGlobalsData,
        mapped_dirs: HashMap<String, PathBuf>,
    ) -> EmscriptenData {
        EmscriptenData {
            globals,
            temp_ret_0: 0,
            mapped_dirs,
            ..Default::default()
        }
    }
}

impl EmscriptenFunctions {
    pub fn new() -> EmscriptenFunctions {
        EmscriptenFunctions {
            ..Default::default()
        }
    }
    pub fn malloc_ref(&self) -> Option<&TypedFunction<u32, u32>> {
        self.malloc.as_ref()
    }
    pub fn free_ref(&self) -> Option<&TypedFunction<u32, ()>> {
        self.free.as_ref()
    }
    pub fn memalign_ref(&self) -> Option<&TypedFunction<(u32, u32), u32>> {
        self.memalign.as_ref()
    }
    pub fn memset_ref(&self) -> Option<&TypedFunction<(u32, u32, u32), u32>> {
        self.memset.as_ref()
    }
    pub fn stack_alloc_ref(&self) -> Option<&TypedFunction<u32, u32>> {
        self.stack_alloc.as_ref()
    }

    pub fn dyn_call_i_ref(&self) -> Option<&TypedFunction<i32, i32>> {
        self.dyn_call_i.as_ref()
    }
    pub fn dyn_call_ii_ref(&self) -> Option<&TypedFunction<(i32, i32), i32>> {
        self.dyn_call_ii.as_ref()
    }
    pub fn dyn_call_iii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32), i32>> {
        self.dyn_call_iii.as_ref()
    }
    pub fn dyn_call_iiii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), i32>> {
        self.dyn_call_iiii.as_ref()
    }
    pub fn dyn_call_iifi_ref(&self) -> Option<&TypedFunction<(i32, i32, f64, i32), i32>> {
        self.dyn_call_iifi.as_ref()
    }
    pub fn dyn_call_v_ref(&self) -> Option<&TypedFunction<i32, ()>> {
        self.dyn_call_v.as_ref()
    }
    pub fn dyn_call_vi_ref(&self) -> Option<&TypedFunction<(i32, i32), ()>> {
        self.dyn_call_vi.as_ref()
    }
    pub fn dyn_call_vii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32), ()>> {
        self.dyn_call_vii.as_ref()
    }
    pub fn dyn_call_viii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), ()>> {
        self.dyn_call_viii.as_ref()
    }
    pub fn dyn_call_viiii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiii.as_ref()
    }
    pub fn dyn_call_dii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32), f64>> {
        self.dyn_call_dii.as_ref()
    }
    pub fn dyn_call_diiii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), f64>> {
        self.dyn_call_diiii.as_ref()
    }
    pub fn dyn_call_iiiii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiii.as_ref()
    }
    pub fn dyn_call_iiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiii.as_ref()
    }
    pub fn dyn_call_iiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiiii.as_ref()
    }
    pub fn dyn_call_iiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiiiii.as_ref()
    }
    pub fn dyn_call_iiiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiiiiii.as_ref()
    }
    pub fn dyn_call_iiiiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiiiiiii.as_ref()
    }
    pub fn dyn_call_iiiiiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiiiiiiiiii.as_ref()
    }
    pub fn dyn_call_vd_ref(&self) -> Option<&TypedFunction<(i32, f64), ()>> {
        self.dyn_call_vd.as_ref()
    }
    pub fn dyn_call_viiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiii.as_ref()
    }
    pub fn dyn_call_viiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiiii.as_ref()
    }
    pub fn dyn_call_viiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiiiii.as_ref()
    }
    pub fn dyn_call_viiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiiiiii.as_ref()
    }
    pub fn dyn_call_viiiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiiiiiii.as_ref()
    }
    pub fn dyn_call_viiiiiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiiiiiiiii.as_ref()
    }
    pub fn dyn_call_iij_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), i32>> {
        self.dyn_call_iij.as_ref()
    }
    pub fn dyn_call_iji_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), i32>> {
        self.dyn_call_iji.as_ref()
    }
    pub fn dyn_call_iiji_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiji.as_ref()
    }
    pub fn dyn_call_iiijj_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_iiijj.as_ref()
    }
    pub fn dyn_call_j_ref(&self) -> Option<&TypedFunction<i32, i32>> {
        self.dyn_call_j.as_ref()
    }
    pub fn dyn_call_ji_ref(&self) -> Option<&TypedFunction<(i32, i32), i32>> {
        self.dyn_call_ji.as_ref()
    }
    pub fn dyn_call_jii_ref(&self) -> Option<&TypedFunction<(i32, i32, i32), i32>> {
        self.dyn_call_jii.as_ref()
    }
    pub fn dyn_call_jij_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), i32>> {
        self.dyn_call_jij.as_ref()
    }
    pub fn dyn_call_jjj_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), i32>> {
        self.dyn_call_jjj.as_ref()
    }
    pub fn dyn_call_viiij_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiij.as_ref()
    }
    pub fn dyn_call_viiijiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiijiiii.as_ref()
    }
    pub fn dyn_call_viiijiiiiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), ()>>
    {
        self.dyn_call_viiijiiiiii.as_ref()
    }
    pub fn dyn_call_viij_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viij.as_ref()
    }
    pub fn dyn_call_viiji_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viiji.as_ref()
    }
    pub fn dyn_call_viijiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viijiii.as_ref()
    }
    pub fn dyn_call_viijj_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viijj.as_ref()
    }
    pub fn dyn_call_vj_ref(&self) -> Option<&TypedFunction<(i32, i32, i32), ()>> {
        self.dyn_call_vj.as_ref()
    }
    pub fn dyn_call_vjji_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_vjji.as_ref()
    }
    pub fn dyn_call_vij_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32), ()>> {
        self.dyn_call_vij.as_ref()
    }
    pub fn dyn_call_viji_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_viji.as_ref()
    }
    pub fn dyn_call_vijiii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_vijiii.as_ref()
    }
    pub fn dyn_call_vijj_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, i32, i32, i32), ()>> {
        self.dyn_call_vijj.as_ref()
    }
    pub fn dyn_call_viid_ref(&self) -> Option<&TypedFunction<(i32, i32, i32, f64), ()>> {
        self.dyn_call_viid.as_ref()
    }
    pub fn dyn_call_vidd_ref(&self) -> Option<&TypedFunction<(i32, i32, f64, f64), ()>> {
        self.dyn_call_vidd.as_ref()
    }
    pub fn dyn_call_viidii_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, f64, i32, i32), ()>> {
        self.dyn_call_viidii.as_ref()
    }
    pub fn dyn_call_viidddddddd_ref(
        &self,
    ) -> Option<&TypedFunction<(i32, i32, i32, f64, f64, f64, f64, f64, f64, f64, f64), ()>> {
        self.dyn_call_viidddddddd.as_ref()
    }

    pub fn stack_save_ref(&self) -> Option<&TypedFunction<(), i32>> {
        self.stack_save.as_ref()
    }
    pub fn stack_restore_ref(&self) -> Option<&TypedFunction<i32, ()>> {
        self.stack_restore.as_ref()
    }
    pub fn set_threw_ref(&self) -> Option<&TypedFunction<(i32, i32), ()>> {
        self.set_threw.as_ref()
    }
}

/// Call the global constructors for C++ and set up the emscripten environment.
///
/// Note that this function does not completely set up Emscripten to be called.
/// before calling this function, please initialize `Ctx::data` with a pointer
/// to [`EmscriptenData`].
pub fn set_up_emscripten(
    ctx: &mut FunctionEnvMut<'_, EmEnv>,
    instance: &mut Instance,
) -> Result<(), RuntimeError> {
    // ATINIT
    // (used by C++)
    if let Ok(func) = instance.exports.get_function("globalCtors") {
        func.call(&mut ctx.as_store_mut(), &[])?;
    }

    if let Ok(func) = instance
        .exports
        .get_function("___emscripten_environ_constructor")
    {
        func.call(&mut ctx.as_store_mut(), &[])?;
    }
    Ok(())
}

/// Call the main function in emscripten, assumes that the emscripten state is
/// set up.
///
/// If you don't want to set it up yourself, consider using [`run_emscripten_instance`].
pub fn emscripten_call_main(
    instance: &mut Instance,
    mut ctx: FunctionEnvMut<'_, EmEnv>,
    path: &str,
    args: &[&str],
) -> Result<(), RuntimeError> {
    let (function_name, main_func) = match instance.exports.get_function("_main") {
        Ok(func) => Ok(("_main", func)),
        Err(_e) => instance
            .exports
            .get_function("main")
            .map(|func| ("main", func)),
    }
    .map_err(|e| RuntimeError::new(e.to_string()))?;
    let num_params = main_func.ty(&ctx).params().len();
    let _result = match num_params {
        2 => {
            let mut new_args = vec![path];
            new_args.extend(args);
            let (argc, argv) = store_module_arguments(ctx.as_store_mut(), new_args);
            let func: &Function = instance
                .exports
                .get(function_name)
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            func.call(
                &mut ctx,
                &[Value::I32(argc as i32), Value::I32(argv as i32)],
            )?;
        }
        0 => {
            let func: &Function = instance
                .exports
                .get(function_name)
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            func.call(&mut ctx, &[])?;
        }
        _ => {
            todo!("Update error type to be able to express this");
            /*return Err(RuntimeError:: CallError::Resolve(ResolveError::ExportWrongType {
                name: "main".to_string(),
            }))*/
        }
    };

    Ok(())
}

/// Top level function to execute emscripten
pub fn run_emscripten_instance(
    instance: &mut Instance,
    mut ctx: FunctionEnvMut<'_, EmEnv>,
    globals: &mut EmscriptenGlobals,
    path: &str,
    args: Vec<&str>,
    entrypoint: Option<String>,
) -> Result<(), RuntimeError> {
    let env = &mut ctx.data_mut();
    env.set_memory(globals.memory.clone());
    // get emscripten export
    let mut emfuncs = EmscriptenFunctions::new();
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "malloc") {
        emfuncs.malloc = Some(func);
    } else if let Ok(func) = instance.exports.get_typed_function(&ctx, "_malloc") {
        emfuncs.malloc = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "free") {
        emfuncs.free = Some(func);
    } else if let Ok(func) = instance.exports.get_typed_function(&ctx, "_free") {
        emfuncs.free = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "memalign") {
        emfuncs.memalign = Some(func);
    } else if let Ok(func) = instance.exports.get_typed_function(&ctx, "_memalign") {
        emfuncs.memalign = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "memset") {
        emfuncs.memset = Some(func);
    } else if let Ok(func) = instance.exports.get_typed_function(&ctx, "_memset") {
        emfuncs.memset = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "stackAlloc") {
        emfuncs.stack_alloc = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_i") {
        emfuncs.dyn_call_i = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_ii") {
        emfuncs.dyn_call_ii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iii") {
        emfuncs.dyn_call_iii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiii") {
        emfuncs.dyn_call_iiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iifi") {
        emfuncs.dyn_call_iifi = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_v") {
        emfuncs.dyn_call_v = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vi") {
        emfuncs.dyn_call_vi = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vii") {
        emfuncs.dyn_call_vii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viii") {
        emfuncs.dyn_call_viii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viiii") {
        emfuncs.dyn_call_viiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_dii") {
        emfuncs.dyn_call_dii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_diiii") {
        emfuncs.dyn_call_diiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiiii") {
        emfuncs.dyn_call_iiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiiiii") {
        emfuncs.dyn_call_iiiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiiiiii") {
        emfuncs.dyn_call_iiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_iiiiiiii")
    {
        emfuncs.dyn_call_iiiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_iiiiiiiii")
    {
        emfuncs.dyn_call_iiiiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_iiiiiiiiii")
    {
        emfuncs.dyn_call_iiiiiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_iiiiiiiiiii")
    {
        emfuncs.dyn_call_iiiiiiiiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vd") {
        emfuncs.dyn_call_vd = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viiiii") {
        emfuncs.dyn_call_viiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viiiiii") {
        emfuncs.dyn_call_viiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiiiiii")
    {
        emfuncs.dyn_call_viiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiiiiiii")
    {
        emfuncs.dyn_call_viiiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiiiiiiii")
    {
        emfuncs.dyn_call_viiiiiiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiiiiiiiii")
    {
        emfuncs.dyn_call_viiiiiiiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iij") {
        emfuncs.dyn_call_iij = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iji") {
        emfuncs.dyn_call_iji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiji") {
        emfuncs.dyn_call_iiji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_iiijj") {
        emfuncs.dyn_call_iiijj = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_j") {
        emfuncs.dyn_call_j = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_ji") {
        emfuncs.dyn_call_ji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_jii") {
        emfuncs.dyn_call_jii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_jij") {
        emfuncs.dyn_call_jij = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_jjj") {
        emfuncs.dyn_call_jjj = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viiij") {
        emfuncs.dyn_call_viiij = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiijiiii")
    {
        emfuncs.dyn_call_viiijiiii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viiijiiiiii")
    {
        emfuncs.dyn_call_viiijiiiiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viij") {
        emfuncs.dyn_call_viij = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viiji") {
        emfuncs.dyn_call_viiji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viijiii") {
        emfuncs.dyn_call_viijiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viijj") {
        emfuncs.dyn_call_viijj = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vj") {
        emfuncs.dyn_call_vj = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vjji") {
        emfuncs.dyn_call_vjji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vij") {
        emfuncs.dyn_call_vij = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viji") {
        emfuncs.dyn_call_viji = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vijiii") {
        emfuncs.dyn_call_vijiii = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vijj") {
        emfuncs.dyn_call_vijj = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viid") {
        emfuncs.dyn_call_viid = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_vidd") {
        emfuncs.dyn_call_vidd = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "dynCall_viidii") {
        emfuncs.dyn_call_viidii = Some(func);
    }
    if let Ok(func) = instance
        .exports
        .get_typed_function(&ctx, "dynCall_viidddddddd")
    {
        emfuncs.dyn_call_viidddddddd = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "stackSave") {
        emfuncs.stack_save = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "stackRestore") {
        emfuncs.stack_restore = Some(func);
    }
    if let Ok(func) = instance.exports.get_typed_function(&ctx, "setThrew") {
        emfuncs.set_threw = Some(func);
    }
    ctx.data_mut().set_functions(emfuncs);

    set_up_emscripten(&mut ctx.as_store_mut(), instance)?;

    // println!("running emscripten instance");

    if let Some(ep) = entrypoint {
        debug!("Running entry point: {}", &ep);
        let arg = unsafe { allocate_cstr_on_stack(&mut ctx.as_store_mut(), args[0]).0 };
        //let (argc, argv) = store_module_arguments(instance.context_mut(), args);
        let func: &Function = instance
            .exports
            .get(&ep)
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        func.call(&mut ctx, &[Value::I32(arg as i32)])?;
    } else {
        emscripten_call_main(instance, ctx, path, &args)?;
    }

    // TODO atexit for emscripten
    // println!("{:?}", data);
    Ok(())
}

fn store_module_arguments(mut ctx: FunctionEnvMut<'_, EmEnv>, args: Vec<&str>) -> (u32, u32) {
    let argc = args.len() + 1;

    let mut args_slice = vec![0; argc];
    for (slot, arg) in args_slice[0..argc].iter_mut().zip(args.iter()) {
        *slot = unsafe { allocate_cstr_on_stack(&mut ctx.as_store_mut(), arg).0 };
    }

    let (argv_offset, argv_slice): (_, &mut [u32]) =
        unsafe { allocate_on_stack(&mut ctx, ((argc) * 4) as u32) };
    assert!(!argv_slice.is_empty());
    for (slot, arg) in argv_slice[0..argc].iter_mut().zip(args_slice.iter()) {
        *slot = *arg
    }
    argv_slice[argc] = 0;

    (argc as u32 - 1, argv_offset)
}

pub fn emscripten_set_up_memory(
    mut ctx: FunctionEnvMut<'_, EmEnv>,
    memory: &Memory,
    globals: &EmscriptenGlobalsData,
) -> Result<(), String> {
    ctx.data_mut().set_memory(memory.clone());
    let dynamictop_ptr = WasmPtr::<i32>::new(globals.dynamictop_ptr).deref(&ctx, memory);
    let dynamic_base = globals.dynamic_base;

    if dynamictop_ptr.offset() >= memory.data_size(&ctx) {
        return Err("dynamictop_ptr beyond memory len".to_string());
    }
    dynamictop_ptr.write(dynamic_base as i32).unwrap();
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct EmscriptenGlobalsData {
    abort: u64,
    // Env namespace
    stacktop: u32,
    stack_max: u32,
    dynamictop_ptr: u32,
    dynamic_base: u32,
    memory_base: u32,
    table_base: u32,
    temp_double_ptr: u32,
    use_old_abort_on_cannot_grow_memory: bool,
}

pub struct EmscriptenGlobals {
    // The emscripten data
    pub data: EmscriptenGlobalsData,
    // The emscripten memory
    pub memory: Memory,
    pub table: Table,
    pub memory_min: Pages,
    pub memory_max: Option<Pages>,
    pub null_function_names: Vec<String>,
}

impl EmscriptenGlobals {
    pub fn new(
        mut ctx: FunctionEnvMut<'_, EmEnv>,
        module: &Module, /*, static_bump: u32 */
    ) -> Result<Self, String> {
        let mut use_old_abort_on_cannot_grow_memory = false;
        for import in module.imports().functions() {
            if import.name() == "abortOnCannotGrowMemory" && import.module() == "env" {
                if import.ty() == &*OLD_ABORT_ON_CANNOT_GROW_MEMORY_SIG {
                    use_old_abort_on_cannot_grow_memory = true;
                }
                break;
            }
        }

        let (table_min, table_max) = get_emscripten_table_size(module)?;
        let (memory_min, memory_max, shared) = get_emscripten_memory_size(module)?;

        // Memory initialization
        let memory_type = MemoryType::new(memory_min, memory_max, shared);
        let memory = Memory::new(&mut ctx, memory_type).unwrap();

        let table_type = TableType {
            ty: ValType::FuncRef,
            minimum: table_min,
            maximum: table_max,
        };
        let table = Table::new(&mut ctx, table_type, Value::FuncRef(None)).unwrap();

        let data = {
            let static_bump = STATIC_BUMP;

            let mut static_top = STATIC_BASE + static_bump;

            let memory_base = STATIC_BASE;
            let table_base = 0;

            let temp_double_ptr = static_top;
            static_top += 16;

            let (dynamic_base, dynamictop_ptr) =
                get_emscripten_metadata(module)?.unwrap_or_else(|| {
                    let dynamictop_ptr = static_alloc(&mut static_top, 4);
                    (
                        align_memory(align_memory(static_top) + TOTAL_STACK),
                        dynamictop_ptr,
                    )
                });

            let stacktop = align_memory(static_top);
            let stack_max = stacktop + TOTAL_STACK;

            EmscriptenGlobalsData {
                abort: 0,
                stacktop,
                stack_max,
                dynamictop_ptr,
                dynamic_base,
                memory_base,
                table_base,
                temp_double_ptr,
                use_old_abort_on_cannot_grow_memory,
            }
        };

        emscripten_set_up_memory(ctx, &memory, &data)?;

        let mut null_function_names = vec![];
        for import in module.imports().functions() {
            if import.module() == "env"
                && (import.name().starts_with("nullFunction_")
                    || import.name().starts_with("nullFunc_"))
            {
                null_function_names.push(import.name().to_string())
            }
        }

        Ok(Self {
            data,
            memory,
            table,
            memory_min,
            memory_max,
            null_function_names,
        })
    }
}

pub fn generate_emscripten_env(
    ctx: &mut FunctionEnvMut<'_, EmEnv>,
    globals: &mut EmscriptenGlobals,
) -> Imports {
    let abort_on_cannot_grow_memory_export = if globals.data.use_old_abort_on_cannot_grow_memory {
        Function::new_native(ctx, crate::memory::abort_on_cannot_grow_memory_old)
    } else {
        Function::new_native(ctx, crate::memory::abort_on_cannot_grow_memory)
    };

    let mut env_ns: Exports = namespace! {
        "memory" => globals.memory.clone(),
        "table" => globals.table.clone(),

        // Globals
        "STACKTOP" => Global::new(ctx, Value::I32(globals.data.stacktop as i32)),
        "STACK_MAX" => Global::new(ctx, Value::I32(globals.data.stack_max as i32)),
        "DYNAMICTOP_PTR" => Global::new(ctx, Value::I32(globals.data.dynamictop_ptr as i32)),
        "fb" => Global::new(ctx, Value::I32(globals.data.table_base as i32)),
        "tableBase" => Global::new(ctx, Value::I32(globals.data.table_base as i32)),
        "__table_base" => Global::new(ctx, Value::I32(globals.data.table_base as i32)),
        "ABORT" => Global::new(ctx, Value::I32(globals.data.abort as i32)),
        "gb" => Global::new(ctx, Value::I32(globals.data.memory_base as i32)),
        "memoryBase" => Global::new(ctx, Value::I32(globals.data.memory_base as i32)),
        "__memory_base" => Global::new(ctx, Value::I32(globals.data.memory_base as i32)),
        "tempDoublePtr" => Global::new(ctx, Value::I32(globals.data.temp_double_ptr as i32)),

        // inet
        "_inet_addr" => Function::new_native(ctx, crate::inet::addr),

        // IO
        "printf" => Function::new_native(ctx, crate::io::printf),
        "putchar" => Function::new_native(ctx, crate::io::putchar),
        "___lock" => Function::new_native(ctx, crate::lock::___lock),
        "___unlock" => Function::new_native(ctx, crate::lock::___unlock),
        "___wait" => Function::new_native(ctx, crate::lock::___wait),
        "_flock" => Function::new_native(ctx, crate::lock::_flock),
        "_chroot" => Function::new_native(ctx, crate::io::chroot),
        "_getprotobyname" => Function::new_native(ctx, crate::io::getprotobyname),
        "_getprotobynumber" => Function::new_native(ctx, crate::io::getprotobynumber),
        "_getpwuid" => Function::new_native(ctx, crate::io::getpwuid),
        "_sigdelset" => Function::new_native(ctx, crate::io::sigdelset),
        "_sigfillset" => Function::new_native(ctx, crate::io::sigfillset),
        "_tzset" => Function::new_native(ctx, crate::io::tzset),
        "_strptime" => Function::new_native(ctx, crate::io::strptime),

        // exec
        "_execvp" => Function::new_native(ctx, crate::exec::execvp),
        "_execl" => Function::new_native(ctx, crate::exec::execl),
        "_execle" => Function::new_native(ctx, crate::exec::execle),

        // exit
        "__exit" => Function::new_native(ctx, crate::exit::exit),

        // Env
        "___assert_fail" => Function::new_native(ctx, crate::env::___assert_fail),
        "_getenv" => Function::new_native(ctx, crate::env::_getenv),
        "_setenv" => Function::new_native(ctx, crate::env::_setenv),
        "_putenv" => Function::new_native(ctx, crate::env::_putenv),
        "_unsetenv" => Function::new_native(ctx, crate::env::_unsetenv),
        "_getpwnam" => Function::new_native(ctx, crate::env::_getpwnam),
        "_getgrnam" => Function::new_native(ctx, crate::env::_getgrnam),
        "___buildEnvironment" => Function::new_native(ctx, crate::env::___build_environment),
        "___setErrNo" => Function::new_native(ctx, crate::errno::___seterrno),
        "_getpagesize" => Function::new_native(ctx, crate::env::_getpagesize),
        "_sysconf" => Function::new_native(ctx, crate::env::_sysconf),
        "_getaddrinfo" => Function::new_native(ctx, crate::env::_getaddrinfo),
        "_times" => Function::new_native(ctx, crate::env::_times),
        "_pathconf" => Function::new_native(ctx, crate::env::_pathconf),
        "_fpathconf" => Function::new_native(ctx, crate::env::_fpathconf),

        // Syscalls
        "___syscall1" => Function::new_native(ctx, crate::syscalls::___syscall1),
        "___syscall3" => Function::new_native(ctx, crate::syscalls::___syscall3),
        "___syscall4" => Function::new_native(ctx, crate::syscalls::___syscall4),
        "___syscall5" => Function::new_native(ctx, crate::syscalls::___syscall5),
        "___syscall6" => Function::new_native(ctx, crate::syscalls::___syscall6),
        "___syscall9" => Function::new_native(ctx, crate::syscalls::___syscall9),
        "___syscall10" => Function::new_native(ctx, crate::syscalls::___syscall10),
        "___syscall12" => Function::new_native(ctx, crate::syscalls::___syscall12),
        "___syscall14" => Function::new_native(ctx, crate::syscalls::___syscall14),
        "___syscall15" => Function::new_native(ctx, crate::syscalls::___syscall15),
        "___syscall20" => Function::new_native(ctx, crate::syscalls::___syscall20),
        "___syscall21" => Function::new_native(ctx, crate::syscalls::___syscall21),
        "___syscall25" => Function::new_native(ctx, crate::syscalls::___syscall25),
        "___syscall29" => Function::new_native(ctx, crate::syscalls::___syscall29),
        "___syscall32" => Function::new_native(ctx, crate::syscalls::___syscall32),
        "___syscall33" => Function::new_native(ctx, crate::syscalls::___syscall33),
        "___syscall34" => Function::new_native(ctx, crate::syscalls::___syscall34),
        "___syscall36" => Function::new_native(ctx, crate::syscalls::___syscall36),
        "___syscall39" => Function::new_native(ctx, crate::syscalls::___syscall39),
        "___syscall38" => Function::new_native(ctx, crate::syscalls::___syscall38),
        "___syscall40" => Function::new_native(ctx, crate::syscalls::___syscall40),
        "___syscall41" => Function::new_native(ctx, crate::syscalls::___syscall41),
        "___syscall42" => Function::new_native(ctx, crate::syscalls::___syscall42),
        "___syscall51" => Function::new_native(ctx, crate::syscalls::___syscall51),
        "___syscall52" => Function::new_native(ctx, crate::syscalls::___syscall52),
        "___syscall53" => Function::new_native(ctx, crate::syscalls::___syscall53),
        "___syscall54" => Function::new_native(ctx, crate::syscalls::___syscall54),
        "___syscall57" => Function::new_native(ctx, crate::syscalls::___syscall57),
        "___syscall60" => Function::new_native(ctx, crate::syscalls::___syscall60),
        "___syscall63" => Function::new_native(ctx, crate::syscalls::___syscall63),
        "___syscall64" => Function::new_native(ctx, crate::syscalls::___syscall64),
        "___syscall66" => Function::new_native(ctx, crate::syscalls::___syscall66),
        "___syscall75" => Function::new_native(ctx, crate::syscalls::___syscall75),
        "___syscall77" => Function::new_native(ctx, crate::syscalls::___syscall77),
        "___syscall83" => Function::new_native(ctx, crate::syscalls::___syscall83),
        "___syscall85" => Function::new_native(ctx, crate::syscalls::___syscall85),
        "___syscall91" => Function::new_native(ctx, crate::syscalls::___syscall91),
        "___syscall94" => Function::new_native(ctx, crate::syscalls::___syscall94),
        "___syscall96" => Function::new_native(ctx, crate::syscalls::___syscall96),
        "___syscall97" => Function::new_native(ctx, crate::syscalls::___syscall97),
        "___syscall102" => Function::new_native(ctx, crate::syscalls::___syscall102),
        "___syscall110" => Function::new_native(ctx, crate::syscalls::___syscall110),
        "___syscall114" => Function::new_native(ctx, crate::syscalls::___syscall114),
        "___syscall118" => Function::new_native(ctx, crate::syscalls::___syscall118),
        "___syscall121" => Function::new_native(ctx, crate::syscalls::___syscall121),
        "___syscall122" => Function::new_native(ctx, crate::syscalls::___syscall122),
        "___syscall125" => Function::new_native(ctx, crate::syscalls::___syscall125),
        "___syscall132" => Function::new_native(ctx, crate::syscalls::___syscall132),
        "___syscall133" => Function::new_native(ctx, crate::syscalls::___syscall133),
        "___syscall140" => Function::new_native(ctx, crate::syscalls::___syscall140),
        "___syscall142" => Function::new_native(ctx, crate::syscalls::___syscall142),
        "___syscall144" => Function::new_native(ctx, crate::syscalls::___syscall144),
        "___syscall145" => Function::new_native(ctx, crate::syscalls::___syscall145),
        "___syscall146" => Function::new_native(ctx, crate::syscalls::___syscall146),
        "___syscall147" => Function::new_native(ctx, crate::syscalls::___syscall147),
        "___syscall148" => Function::new_native(ctx, crate::syscalls::___syscall148),
        "___syscall150" => Function::new_native(ctx, crate::syscalls::___syscall150),
        "___syscall151" => Function::new_native(ctx, crate::syscalls::___syscall151),
        "___syscall152" => Function::new_native(ctx, crate::syscalls::___syscall152),
        "___syscall153" => Function::new_native(ctx, crate::syscalls::___syscall153),
        "___syscall163" => Function::new_native(ctx, crate::syscalls::___syscall163),
        "___syscall168" => Function::new_native(ctx, crate::syscalls::___syscall168),
        "___syscall180" => Function::new_native(ctx, crate::syscalls::___syscall180),
        "___syscall181" => Function::new_native(ctx, crate::syscalls::___syscall181),
        "___syscall183" => Function::new_native(ctx, crate::syscalls::___syscall183),
        "___syscall191" => Function::new_native(ctx, crate::syscalls::___syscall191),
        "___syscall192" => Function::new_native(ctx, crate::syscalls::___syscall192),
        "___syscall193" => Function::new_native(ctx, crate::syscalls::___syscall193),
        "___syscall194" => Function::new_native(ctx, crate::syscalls::___syscall194),
        "___syscall195" => Function::new_native(ctx, crate::syscalls::___syscall195),
        "___syscall196" => Function::new_native(ctx, crate::syscalls::___syscall196),
        "___syscall197" => Function::new_native(ctx, crate::syscalls::___syscall197),
        "___syscall198" => Function::new_native(ctx, crate::syscalls::___syscall198),
        "___syscall199" => Function::new_native(ctx, crate::syscalls::___syscall199),
        "___syscall200" => Function::new_native(ctx, crate::syscalls::___syscall200),
        "___syscall201" => Function::new_native(ctx, crate::syscalls::___syscall201),
        "___syscall202" => Function::new_native(ctx, crate::syscalls::___syscall202),
        "___syscall205" => Function::new_native(ctx, crate::syscalls::___syscall205),
        "___syscall207" => Function::new_native(ctx, crate::syscalls::___syscall207),
        "___syscall209" => Function::new_native(ctx, crate::syscalls::___syscall209),
        "___syscall211" => Function::new_native(ctx, crate::syscalls::___syscall211),
        "___syscall212" => Function::new_native(ctx, crate::syscalls::___syscall212),
        "___syscall218" => Function::new_native(ctx, crate::syscalls::___syscall218),
        "___syscall219" => Function::new_native(ctx, crate::syscalls::___syscall219),
        "___syscall220" => Function::new_native(ctx, crate::syscalls::___syscall220),
        "___syscall221" => Function::new_native(ctx, crate::syscalls::___syscall221),
        "___syscall268" => Function::new_native(ctx, crate::syscalls::___syscall268),
        "___syscall269" => Function::new_native(ctx, crate::syscalls::___syscall269),
        "___syscall272" => Function::new_native(ctx, crate::syscalls::___syscall272),
        "___syscall295" => Function::new_native(ctx, crate::syscalls::___syscall295),
        "___syscall296" => Function::new_native(ctx, crate::syscalls::___syscall296),
        "___syscall297" => Function::new_native(ctx, crate::syscalls::___syscall297),
        "___syscall298" => Function::new_native(ctx, crate::syscalls::___syscall298),
        "___syscall300" => Function::new_native(ctx, crate::syscalls::___syscall300),
        "___syscall301" => Function::new_native(ctx, crate::syscalls::___syscall301),
        "___syscall302" => Function::new_native(ctx, crate::syscalls::___syscall302),
        "___syscall303" => Function::new_native(ctx, crate::syscalls::___syscall303),
        "___syscall304" => Function::new_native(ctx, crate::syscalls::___syscall304),
        "___syscall305" => Function::new_native(ctx, crate::syscalls::___syscall305),
        "___syscall306" => Function::new_native(ctx, crate::syscalls::___syscall306),
        "___syscall307" => Function::new_native(ctx, crate::syscalls::___syscall307),
        "___syscall308" => Function::new_native(ctx, crate::syscalls::___syscall308),
        "___syscall320" => Function::new_native(ctx, crate::syscalls::___syscall320),
        "___syscall324" => Function::new_native(ctx, crate::syscalls::___syscall324),
        "___syscall330" => Function::new_native(ctx, crate::syscalls::___syscall330),
        "___syscall331" => Function::new_native(ctx, crate::syscalls::___syscall331),
        "___syscall333" => Function::new_native(ctx, crate::syscalls::___syscall333),
        "___syscall334" => Function::new_native(ctx, crate::syscalls::___syscall334),
        "___syscall337" => Function::new_native(ctx, crate::syscalls::___syscall337),
        "___syscall340" => Function::new_native(ctx, crate::syscalls::___syscall340),
        "___syscall345" => Function::new_native(ctx, crate::syscalls::___syscall345),

        // Process
        "abort" => Function::new_native(ctx, crate::process::em_abort),
        "_abort" => Function::new_native(ctx, crate::process::_abort),
        "_prctl" => Function::new_native(ctx, crate::process::_prctl),
        "abortStackOverflow" => Function::new_native(ctx, crate::process::abort_stack_overflow),
        "_llvm_trap" => Function::new_native(ctx, crate::process::_llvm_trap),
        "_fork" => Function::new_native(ctx, crate::process::_fork),
        "_exit" => Function::new_native(ctx, crate::process::_exit),
        "_system" => Function::new_native(ctx, crate::process::_system),
        "_popen" => Function::new_native(ctx, crate::process::_popen),
        "_endgrent" => Function::new_native(ctx, crate::process::_endgrent),
        "_execve" => Function::new_native(ctx, crate::process::_execve),
        "_kill" => Function::new_native(ctx, crate::process::_kill),
        "_llvm_stackrestore" => Function::new_native(ctx, crate::process::_llvm_stackrestore),
        "_llvm_stacksave" => Function::new_native(ctx, crate::process::_llvm_stacksave),
        "_llvm_eh_typeid_for" => Function::new_native(ctx, crate::process::_llvm_eh_typeid_for),
        "_raise" => Function::new_native(ctx, crate::process::_raise),
        "_sem_init" => Function::new_native(ctx, crate::process::_sem_init),
        "_sem_destroy" => Function::new_native(ctx, crate::process::_sem_destroy),
        "_sem_post" => Function::new_native(ctx, crate::process::_sem_post),
        "_sem_wait" => Function::new_native(ctx, crate::process::_sem_wait),
        "_getgrent" => Function::new_native(ctx, crate::process::_getgrent),
        "_sched_yield" => Function::new_native(ctx, crate::process::_sched_yield),
        "_setgrent" => Function::new_native(ctx, crate::process::_setgrent),
        "_setgroups" => Function::new_native(ctx, crate::process::_setgroups),
        "_setitimer" => Function::new_native(ctx, crate::process::_setitimer),
        "_usleep" => Function::new_native(ctx, crate::process::_usleep),
        "_nanosleep" => Function::new_native(ctx, crate::process::_nanosleep),
        "_utime" => Function::new_native(ctx, crate::process::_utime),
        "_utimes" => Function::new_native(ctx, crate::process::_utimes),
        "_wait" => Function::new_native(ctx, crate::process::_wait),
        "_wait3" => Function::new_native(ctx, crate::process::_wait3),
        "_wait4" => Function::new_native(ctx, crate::process::_wait4),
        "_waitid" => Function::new_native(ctx, crate::process::_waitid),
        "_waitpid" => Function::new_native(ctx, crate::process::_waitpid),

        // Emscripten
        "_emscripten_asm_const_i" => Function::new_native(ctx, crate::emscripten_target::asm_const_i),
        "_emscripten_exit_with_live_runtime" => Function::new_native(ctx, crate::emscripten_target::exit_with_live_runtime),

        // Signal
        "_sigemptyset" => Function::new_native(ctx, crate::signal::_sigemptyset),
        "_sigaddset" => Function::new_native(ctx, crate::signal::_sigaddset),
        "_sigprocmask" => Function::new_native(ctx, crate::signal::_sigprocmask),
        "_sigaction" => Function::new_native(ctx, crate::signal::_sigaction),
        "_signal" => Function::new_native(ctx, crate::signal::_signal),
        "_sigsuspend" => Function::new_native(ctx, crate::signal::_sigsuspend),

        // Memory
        "abortOnCannotGrowMemory" => abort_on_cannot_grow_memory_export,
        "_emscripten_memcpy_big" => Function::new_native(ctx, crate::memory::_emscripten_memcpy_big),
        "_emscripten_get_heap_size" => Function::new_native(ctx, crate::memory::_emscripten_get_heap_size),
        "_emscripten_resize_heap" => Function::new_native(ctx, crate::memory::_emscripten_resize_heap),
        "enlargeMemory" => Function::new_native(ctx, crate::memory::enlarge_memory),
        "segfault" => Function::new_native(ctx, crate::memory::segfault),
        "alignfault" => Function::new_native(ctx, crate::memory::alignfault),
        "ftfault" => Function::new_native(ctx, crate::memory::ftfault),
        "getTotalMemory" => Function::new_native(ctx, crate::memory::get_total_memory),
        "_sbrk" => Function::new_native(ctx, crate::memory::sbrk),
        "___map_file" => Function::new_native(ctx, crate::memory::___map_file),

        // Exception
        "___cxa_allocate_exception" => Function::new_native(ctx, crate::exception::___cxa_allocate_exception),
        "___cxa_current_primary_exception" => Function::new_native(ctx, crate::exception::___cxa_current_primary_exception),
        "___cxa_decrement_exception_refcount" => Function::new_native(ctx, crate::exception::___cxa_decrement_exception_refcount),
        "___cxa_increment_exception_refcount" => Function::new_native(ctx, crate::exception::___cxa_increment_exception_refcount),
        "___cxa_rethrow_primary_exception" => Function::new_native(ctx, crate::exception::___cxa_rethrow_primary_exception),
        "___cxa_throw" => Function::new_native(ctx, crate::exception::___cxa_throw),
        "___cxa_begin_catch" => Function::new_native(ctx, crate::exception::___cxa_begin_catch),
        "___cxa_end_catch" => Function::new_native(ctx, crate::exception::___cxa_end_catch),
        "___cxa_uncaught_exception" => Function::new_native(ctx, crate::exception::___cxa_uncaught_exception),
        "___cxa_pure_virtual" => Function::new_native(ctx, crate::exception::___cxa_pure_virtual),

        // Time
        "_gettimeofday" => Function::new_native(ctx, crate::time::_gettimeofday),
        "_clock_getres" => Function::new_native(ctx, crate::time::_clock_getres),
        "_clock_gettime" => Function::new_native(ctx, crate::time::_clock_gettime),
        "_clock_settime" => Function::new_native(ctx, crate::time::_clock_settime),
        "___clock_gettime" => Function::new_native(ctx, crate::time::_clock_gettime),
        "_clock" => Function::new_native(ctx, crate::time::_clock),
        "_difftime" => Function::new_native(ctx, crate::time::_difftime),
        "_asctime" => Function::new_native(ctx, crate::time::_asctime),
        "_asctime_r" => Function::new_native(ctx, crate::time::_asctime_r),
        "_localtime" => Function::new_native(ctx, crate::time::_localtime),
        "_time" => Function::new_native(ctx, crate::time::_time),
        "_timegm" => Function::new_native(ctx, crate::time::_timegm),
        "_strftime" => Function::new_native(ctx, crate::time::_strftime),
        "_strftime_l" => Function::new_native(ctx, crate::time::_strftime_l),
        "_localtime_r" => Function::new_native(ctx, crate::time::_localtime_r),
        "_gmtime_r" => Function::new_native(ctx, crate::time::_gmtime_r),
        "_ctime" => Function::new_native(ctx, crate::time::_ctime),
        "_ctime_r" => Function::new_native(ctx, crate::time::_ctime_r),
        "_mktime" => Function::new_native(ctx, crate::time::_mktime),
        "_gmtime" => Function::new_native(ctx, crate::time::_gmtime),

        // Math
        "sqrt" => Function::new_native(ctx, crate::math::sqrt),
        "floor" => Function::new_native(ctx, crate::math::floor),
        "fabs" => Function::new_native(ctx, crate::math::fabs),
        "f64-rem" => Function::new_native(ctx, crate::math::f64_rem),
        "_llvm_copysign_f32" => Function::new_native(ctx, crate::math::_llvm_copysign_f32),
        "_llvm_copysign_f64" => Function::new_native(ctx, crate::math::_llvm_copysign_f64),
        "_llvm_log10_f64" => Function::new_native(ctx, crate::math::_llvm_log10_f64),
        "_llvm_log2_f64" => Function::new_native(ctx, crate::math::_llvm_log2_f64),
        "_llvm_log10_f32" => Function::new_native(ctx, crate::math::_llvm_log10_f32),
        "_llvm_log2_f32" => Function::new_native(ctx, crate::math::_llvm_log2_f64),
        "_llvm_sin_f64" => Function::new_native(ctx, crate::math::_llvm_sin_f64),
        "_llvm_cos_f64" => Function::new_native(ctx, crate::math::_llvm_cos_f64),
        "_llvm_exp2_f32" => Function::new_native(ctx, crate::math::_llvm_exp2_f32),
        "_llvm_exp2_f64" => Function::new_native(ctx, crate::math::_llvm_exp2_f64),
        "_llvm_trunc_f64" => Function::new_native(ctx, crate::math::_llvm_trunc_f64),
        "_llvm_fma_f64" => Function::new_native(ctx, crate::math::_llvm_fma_f64),
        "_emscripten_random" => Function::new_native(ctx, crate::math::_emscripten_random),

        // Jump
        "__setjmp" => Function::new_native(ctx, crate::jmp::__setjmp),
        "__longjmp" => Function::new_native(ctx, crate::jmp::__longjmp),
        "_longjmp" => Function::new_native(ctx, crate::jmp::_longjmp),
        "_emscripten_longjmp" => Function::new_native(ctx, crate::jmp::_longjmp),

        // Bitwise
        "_llvm_bswap_i64" => Function::new_native(ctx, crate::bitwise::_llvm_bswap_i64),

        // libc
        "_execv" => Function::new_native(ctx, crate::libc::execv),
        "_endpwent" => Function::new_native(ctx, crate::libc::endpwent),
        "_fexecve" => Function::new_native(ctx, crate::libc::fexecve),
        "_fpathconf" => Function::new_native(ctx, crate::libc::fpathconf),
        "_getitimer" => Function::new_native(ctx, crate::libc::getitimer),
        "_getpwent" => Function::new_native(ctx, crate::libc::getpwent),
        "_killpg" => Function::new_native(ctx, crate::libc::killpg),
        "_pathconf" => Function::new_native(ctx, crate::libc::pathconf),
        "_siginterrupt" => Function::new_native(ctx, crate::signal::_siginterrupt),
        "_setpwent" => Function::new_native(ctx, crate::libc::setpwent),
        "_sigismember" => Function::new_native(ctx, crate::libc::sigismember),
        "_sigpending" => Function::new_native(ctx, crate::libc::sigpending),
        "___libc_current_sigrtmax" => Function::new_native(ctx, crate::libc::current_sigrtmax),
        "___libc_current_sigrtmin" => Function::new_native(ctx, crate::libc::current_sigrtmin),

        // Linking
        "_dlclose" => Function::new_native(ctx, crate::linking::_dlclose),
        "_dlerror" => Function::new_native(ctx, crate::linking::_dlerror),
        "_dlopen" => Function::new_native(ctx, crate::linking::_dlopen),
        "_dlsym" => Function::new_native(ctx, crate::linking::_dlsym),

        // wasm32-unknown-emscripten
        "_alarm" => Function::new_native(ctx, crate::emscripten_target::_alarm),
        "_atexit" => Function::new_native(ctx, crate::emscripten_target::_atexit),
        "setTempRet0" => Function::new_native(ctx, crate::emscripten_target::setTempRet0),
        "getTempRet0" => Function::new_native(ctx, crate::emscripten_target::getTempRet0),
        "invoke_i" => Function::new_native(ctx, crate::emscripten_target::invoke_i),
        "invoke_ii" => Function::new_native(ctx, crate::emscripten_target::invoke_ii),
        "invoke_iii" => Function::new_native(ctx, crate::emscripten_target::invoke_iii),
        "invoke_iiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiii),
        "invoke_iifi" => Function::new_native(ctx, crate::emscripten_target::invoke_iifi),
        "invoke_v" => Function::new_native(ctx, crate::emscripten_target::invoke_v),
        "invoke_vi" => Function::new_native(ctx, crate::emscripten_target::invoke_vi),
        "invoke_vj" => Function::new_native(ctx, crate::emscripten_target::invoke_vj),
        "invoke_vjji" => Function::new_native(ctx, crate::emscripten_target::invoke_vjji),
        "invoke_vii" => Function::new_native(ctx, crate::emscripten_target::invoke_vii),
        "invoke_viii" => Function::new_native(ctx, crate::emscripten_target::invoke_viii),
        "invoke_viiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiii),
        "__Unwind_Backtrace" => Function::new_native(ctx, crate::emscripten_target::__Unwind_Backtrace),
        "__Unwind_FindEnclosingFunction" => Function::new_native(ctx, crate::emscripten_target::__Unwind_FindEnclosingFunction),
        "__Unwind_GetIPInfo" => Function::new_native(ctx, crate::emscripten_target::__Unwind_GetIPInfo),
        "___cxa_find_matching_catch_2" => Function::new_native(ctx, crate::emscripten_target::___cxa_find_matching_catch_2),
        "___cxa_find_matching_catch_3" => Function::new_native(ctx, crate::emscripten_target::___cxa_find_matching_catch_3),
        "___cxa_free_exception" => Function::new_native(ctx, crate::emscripten_target::___cxa_free_exception),
        "___resumeException" => Function::new_native(ctx, crate::emscripten_target::___resumeException),
        "_dladdr" => Function::new_native(ctx, crate::emscripten_target::_dladdr),
        "_pthread_attr_destroy" => Function::new_native(ctx, crate::pthread::_pthread_attr_destroy),
        "_pthread_attr_getstack" => Function::new_native(ctx, crate::pthread::_pthread_attr_getstack),
        "_pthread_attr_init" => Function::new_native(ctx, crate::pthread::_pthread_attr_init),
        "_pthread_attr_setstacksize" => Function::new_native(ctx, crate::pthread::_pthread_attr_setstacksize),
        "_pthread_cleanup_pop" => Function::new_native(ctx, crate::pthread::_pthread_cleanup_pop),
        "_pthread_cleanup_push" => Function::new_native(ctx, crate::pthread::_pthread_cleanup_push),
        "_pthread_cond_destroy" => Function::new_native(ctx, crate::pthread::_pthread_cond_destroy),
        "_pthread_cond_init" => Function::new_native(ctx, crate::pthread::_pthread_cond_init),
        "_pthread_cond_signal" => Function::new_native(ctx, crate::pthread::_pthread_cond_signal),
        "_pthread_cond_timedwait" => Function::new_native(ctx, crate::pthread::_pthread_cond_timedwait),
        "_pthread_cond_wait" => Function::new_native(ctx, crate::pthread::_pthread_cond_wait),
        "_pthread_condattr_destroy" => Function::new_native(ctx, crate::pthread::_pthread_condattr_destroy),
        "_pthread_condattr_init" => Function::new_native(ctx, crate::pthread::_pthread_condattr_init),
        "_pthread_condattr_setclock" => Function::new_native(ctx, crate::pthread::_pthread_condattr_setclock),
        "_pthread_create" => Function::new_native(ctx, crate::pthread::_pthread_create),
        "_pthread_detach" => Function::new_native(ctx, crate::pthread::_pthread_detach),
        "_pthread_equal" => Function::new_native(ctx, crate::pthread::_pthread_equal),
        "_pthread_exit" => Function::new_native(ctx, crate::pthread::_pthread_exit),
        "_pthread_self" => Function::new_native(ctx, crate::pthread::_pthread_self),
        "_pthread_getattr_np" => Function::new_native(ctx, crate::pthread::_pthread_getattr_np),
        "_pthread_getspecific" => Function::new_native(ctx, crate::pthread::_pthread_getspecific),
        "_pthread_join" => Function::new_native(ctx, crate::pthread::_pthread_join),
        "_pthread_key_create" => Function::new_native(ctx, crate::pthread::_pthread_key_create),
        "_pthread_mutex_destroy" => Function::new_native(ctx, crate::pthread::_pthread_mutex_destroy),
        "_pthread_mutex_init" => Function::new_native(ctx, crate::pthread::_pthread_mutex_init),
        "_pthread_mutexattr_destroy" => Function::new_native(ctx, crate::pthread::_pthread_mutexattr_destroy),
        "_pthread_mutexattr_init" => Function::new_native(ctx, crate::pthread::_pthread_mutexattr_init),
        "_pthread_mutexattr_settype" => Function::new_native(ctx, crate::pthread::_pthread_mutexattr_settype),
        "_pthread_once" => Function::new_native(ctx, crate::pthread::_pthread_once),
        "_pthread_rwlock_destroy" => Function::new_native(ctx, crate::pthread::_pthread_rwlock_destroy),
        "_pthread_rwlock_init" => Function::new_native(ctx, crate::pthread::_pthread_rwlock_init),
        "_pthread_rwlock_rdlock" => Function::new_native(ctx, crate::pthread::_pthread_rwlock_rdlock),
        "_pthread_rwlock_unlock" => Function::new_native(ctx, crate::pthread::_pthread_rwlock_unlock),
        "_pthread_rwlock_wrlock" => Function::new_native(ctx, crate::pthread::_pthread_rwlock_wrlock),
        "_pthread_setcancelstate" => Function::new_native(ctx, crate::pthread::_pthread_setcancelstate),
        "_pthread_setspecific" => Function::new_native(ctx, crate::pthread::_pthread_setspecific),
        "_pthread_sigmask" => Function::new_native(ctx, crate::pthread::_pthread_sigmask),
        "___gxx_personality_v0" => Function::new_native(ctx, crate::emscripten_target::___gxx_personality_v0),
        "_gai_strerror" => Function::new_native(ctx, crate::env::_gai_strerror),
        "_getdtablesize" => Function::new_native(ctx, crate::emscripten_target::_getdtablesize),
        "_gethostbyaddr" => Function::new_native(ctx, crate::emscripten_target::_gethostbyaddr),
        "_gethostbyname" => Function::new_native(ctx, crate::emscripten_target::_gethostbyname),
        "_gethostbyname_r" => Function::new_native(ctx, crate::emscripten_target::_gethostbyname_r),
        "_getloadavg" => Function::new_native(ctx, crate::emscripten_target::_getloadavg),
        "_getnameinfo" => Function::new_native(ctx, crate::emscripten_target::_getnameinfo),
        "invoke_dii" => Function::new_native(ctx, crate::emscripten_target::invoke_dii),
        "invoke_diiii" => Function::new_native(ctx, crate::emscripten_target::invoke_diiii),
        "invoke_iiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiii),
        "invoke_iiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiii),
        "invoke_iiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiiii),
        "invoke_iiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiiiii),
        "invoke_iiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiiiiii),
        "invoke_iiiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiiiiiii),
        "invoke_iiiiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_iiiiiiiiiii),
        "invoke_vd" => Function::new_native(ctx, crate::emscripten_target::invoke_vd),
        "invoke_viiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiii),
        "invoke_viiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiii),
        "invoke_viiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiiii),
        "invoke_viiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiiiii),
        "invoke_viiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiiiiii),
        "invoke_viiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiiiiii),
        "invoke_viiiiiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiiiiiiiii),
        "invoke_iij" => Function::new_native(ctx, crate::emscripten_target::invoke_iij),
        "invoke_iji" => Function::new_native(ctx, crate::emscripten_target::invoke_iji),
        "invoke_iiji" => Function::new_native(ctx, crate::emscripten_target::invoke_iiji),
        "invoke_iiijj" => Function::new_native(ctx, crate::emscripten_target::invoke_iiijj),
        "invoke_j" => Function::new_native(ctx, crate::emscripten_target::invoke_j),
        "invoke_ji" => Function::new_native(ctx, crate::emscripten_target::invoke_ji),
        "invoke_jii" => Function::new_native(ctx, crate::emscripten_target::invoke_jii),
        "invoke_jij" => Function::new_native(ctx, crate::emscripten_target::invoke_jij),
        "invoke_jjj" => Function::new_native(ctx, crate::emscripten_target::invoke_jjj),
        "invoke_viiij" => Function::new_native(ctx, crate::emscripten_target::invoke_viiij),
        "invoke_viiijiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiijiiii),
        "invoke_viiijiiiiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viiijiiiiii),
        "invoke_viij" => Function::new_native(ctx, crate::emscripten_target::invoke_viij),
        "invoke_viiji" => Function::new_native(ctx, crate::emscripten_target::invoke_viiji),
        "invoke_viijiii" => Function::new_native(ctx, crate::emscripten_target::invoke_viijiii),
        "invoke_viijj" => Function::new_native(ctx, crate::emscripten_target::invoke_viijj),
        "invoke_vij" => Function::new_native(ctx, crate::emscripten_target::invoke_vij),
        "invoke_viji" => Function::new_native(ctx, crate::emscripten_target::invoke_viji),
        "invoke_vijiii" => Function::new_native(ctx, crate::emscripten_target::invoke_vijiii),
        "invoke_vijj" => Function::new_native(ctx, crate::emscripten_target::invoke_vijj),
        "invoke_vidd" => Function::new_native(ctx, crate::emscripten_target::invoke_vidd),
        "invoke_viid" => Function::new_native(ctx, crate::emscripten_target::invoke_viid),
        "invoke_viidii" => Function::new_native(ctx, crate::emscripten_target::invoke_viidii),
        "invoke_viidddddddd" => Function::new_native(ctx, crate::emscripten_target::invoke_viidddddddd),

        // ucontext
        "_getcontext" => Function::new_native(ctx, crate::ucontext::_getcontext),
        "_makecontext" => Function::new_native(ctx, crate::ucontext::_makecontext),
        "_setcontext" => Function::new_native(ctx, crate::ucontext::_setcontext),
        "_swapcontext" => Function::new_native(ctx, crate::ucontext::_swapcontext),

        // unistd
        "_confstr" => Function::new_native(ctx, crate::unistd::confstr),
    };

    // Compatibility with newer versions of Emscripten
    let mut to_insert: Vec<(String, _)> = vec![];
    for (k, v) in env_ns.iter() {
        if let Some(k) = k.strip_prefix('_') {
            if !env_ns.contains(k) {
                to_insert.push((k.to_string(), v.clone()));
            }
        }
    }

    for (k, v) in to_insert {
        env_ns.insert(k, v);
    }

    for null_function_name in globals.null_function_names.iter() {
        env_ns.insert(
            null_function_name.as_str(),
            Function::new_native(ctx, nullfunc),
        );
    }

    let import_object: Imports = imports! {
        "env" => env_ns,
        "global" => {
          "NaN" => Global::new(ctx, Value::F64(f64::NAN)),
          "Infinity" => Global::new(ctx, Value::F64(f64::INFINITY)),
        },
        "global.Math" => {
            "pow" => Function::new_native(ctx, crate::math::pow),
            "exp" => Function::new_native(ctx, crate::math::exp),
            "log" => Function::new_native(ctx, crate::math::log),
        },
        "asm2wasm" => {
            "f64-rem" => Function::new_native(ctx, crate::math::f64_rem),
            "f64-to-int" => Function::new_native(ctx, crate::math::f64_to_int),
        },
    };

    import_object
}

pub fn nullfunc(ctx: FunctionEnvMut<'_, EmEnv>, _x: u32) {
    use crate::process::abort_with_message;
    debug!("emscripten::nullfunc_i {}", _x);
    abort_with_message(
        ctx,
        "Invalid function pointer. Perhaps this is an invalid value \
    (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an \
    incorrect type, which will fail? (it is worth building your source files with -Werror (\
    warnings are errors), as warnings can indicate undefined behavior which can cause this)",
    );
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
