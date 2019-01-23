use super::{
    env, errno, exception, io, jmp, lock, math, memory, nullfunc, process, signal, storage,
    storage::{
        dynamic_base, dynamictop_ptr, memory_base, stack_max, stacktop, statictop, STATIC_BUMP,
    },
    syscalls, time, utils, varargs,
};
use crate::storage::align_memory;
use hashbrown::{
    hash_map::{Entry, Values},
    HashMap, HashSet,
};
use std::mem;
use wasmer_runtime_core::{
    export::{Context, Export, FuncPointer, GlobalPointer, MemoryPointer, TablePointer},
    import::{ImportObject, Namespace},
    memory::LinearMemory,
    module::{ImportName, Module},
    structures::TypedIndex,
    types::{
        FuncSig, GlobalDesc, ImportedMemoryIndex, LocalMemoryIndex, Memory, Table,
        Type::{self, *},
        Value,
    },
    vm::{Func, LocalGlobal, LocalMemory, LocalTable},
};

/// TODO: Abstract to HostData that takes in or registers known data and generates missing data.
/// This contains data owned by the Emscripten environment which can then be referenced
/// by a webassembly instance.
pub struct EmscriptenData {
    // Memories owned by the Emscripten environment.
    pub memories: HashMap<String, HashMap<String, (Vec<u8>, Box<LocalMemory>, Export)>>,
    // Tables owned by the Emscripten environment.
    pub tables: HashMap<String, HashMap<String, (Vec<u8>, Box<LocalTable>, Export)>>,
    // Globals owned by the Emscripten environment.
    pub globals: HashMap<String, HashMap<String, (LocalGlobal, Export)>>,
}

impl EmscriptenData {
    /// Creates a new Emscripten Data
    pub fn new(module: &Module) -> EmscriptenData {
        let memories = EmscriptenData::generate_memories(module);
        let tables = EmscriptenData::generate_tables(module);
        let globals = EmscriptenData::generate_globals(module);
        Self {
            memories,
            tables,
            globals,
        }
    }

    /// Generates owned memories based on imports definition.
    pub fn generate_memories(
        module: &Module,
    ) -> HashMap<String, HashMap<String, (Vec<u8>, Box<LocalMemory>, Export)>> {
        let imported_memories = &module.0.imported_memories;
        let mut memories: HashMap<String, HashMap<String, (Vec<u8>, Box<LocalMemory>, Export)>> =
            HashMap::new();

        // Iterate imported memories.
        for (index, (name, memory)) in imported_memories.iter() {
            // Get details of memory.
            let ImportName { namespace, name } = name.clone();
            let Memory { min, max, shared } = memory.clone();

            let size = (min * LinearMemory::PAGE_SIZE) as usize;

            // TODO: Protect memory!
            // Create owned memory.
            let mut memory = vec![0; size];

            // Create a LocalMemory that references the owned memory.
            let mut local_memory = Box::new(LocalMemory {
                base: memory.as_mut_ptr(),
                size,
                index: LocalMemoryIndex::new(index.index()),
            });

            // Create an export interface for owned memory.
            // This can be reused later when generating an environment imports.
            let export = Export::Memory {
                local: unsafe {
                    MemoryPointer::new(std::mem::transmute::<&mut LocalMemory, *mut LocalMemory>(
                        local_memory.as_mut(),
                    ))
                },
                ctx: Context::Internal,
                memory: Memory { min, max, shared },
            };

            // Create a tuple from the data.
            let memory = (memory, local_memory, export);

            // Add the data to respective namespace.
            if let Some(namespace) = memories.get_mut(&namespace) {
                namespace.insert(name, memory);
            } else {
                let mut memory_namespace = HashMap::new();
                memory_namespace.insert(name, memory);
                memories.insert(namespace, memory_namespace);
            }
        }

        memories
    }

    /// Generates owned tables based on imports definition.
    pub fn generate_tables(
        module: &Module,
    ) -> HashMap<String, HashMap<String, (Vec<u8>, Box<LocalTable>, Export)>> {
        let imported_tables = &module.0.imported_tables;
        let mut tables: HashMap<String, HashMap<String, (Vec<u8>, Box<LocalTable>, Export)>> =
            HashMap::new();

        // Iterate imported memories.
        for (_, (name, table)) in imported_tables.iter() {
            let ImportName { namespace, name } = name.clone();
            let Table { ty, min, max } = table.clone();

            // TODO: Protect memory!
            // Create owned table.
            let mut table = vec![0; min as _];

            // Create a LocalTable that references the owned table.
            let mut local_table = Box::new(LocalTable {
                base: table.as_mut_ptr(),
                current_elements: min as _,
                capacity: max.unwrap_or(min) as _,
            });

            // Create an export interface for owned table.
            // This can be reused later when generating an environment imports.
            let export = Export::Table {
                local: unsafe {
                    TablePointer::new(std::mem::transmute::<&mut LocalTable, *mut LocalTable>(
                        local_table.as_mut(),
                    ))
                },
                ctx: Context::Internal,
                table: Table { ty, min, max },
            };

            // Create a tuple from the data.
            let table = (table, local_table, export);

            // Add the data to respective namespace.
            if let Some(namespace) = tables.get_mut(&namespace) {
                namespace.insert(name, table);
            } else {
                let mut table_namespace = HashMap::new();
                table_namespace.insert(name, table);
                tables.insert(namespace, table_namespace);
            }
        }

        tables
    }

    /// Generates owned globals based on known data and imports definition.
    pub fn generate_globals(
        module: &Module,
    ) -> HashMap<String, HashMap<String, (LocalGlobal, Export)>> {
        let imported_globals = &module.0.imported_globals;
        let mut globals: HashMap<String, HashMap<String, (LocalGlobal, Export)>> = HashMap::new();
        let mut known_globals: HashMap<String, u64> = HashMap::new();

        // Create known globals that are always present in the emscripten environment.
        known_globals.insert("STACKTOP".into(), stacktop(STATIC_BUMP) as _);
        known_globals.insert("STACK_MAX".into(), stack_max(STATIC_BUMP) as _);
        known_globals.insert("DYNAMICTOP_PTR".into(), dynamictop_ptr(STATIC_BUMP) as _);
        // Emscripten has two versions of `memoryBase`.
        known_globals.insert("memoryBase".into(), memory_base() as _);
        known_globals.insert("__memory_base".into(), memory_base() as _);
        // tempDoublePtr.
        known_globals.insert("tempDoublePtr".into(), stacktop(STATIC_BUMP) as _);
        // Emscripten has two versions of `tableBase`.
        known_globals.insert("tableBase".into(), 0);
        known_globals.insert("__table_base".into(), 0);
        known_globals.insert("Infinity".into(), std::f64::INFINITY.to_bits() as _);
        known_globals.insert("NaN".into(), std::f64::NAN.to_bits() as _);

        // Iterate imported globals
        for (_, (name, global)) in imported_globals.iter() {
            let ImportName { namespace, name } = name.clone();
            let GlobalDesc { mutable, ty } = global.clone();

            // Create a LocalGlobal that contains owned global data.
            let local_global = LocalGlobal {
                // If it is a know global, replace value as appropriate.
                data: match known_globals.get(&name) {
                    Some(value) => value.clone(),
                    None => 0,
                },
            };

            // Create an export interface for owned global.
            // This can be reused later when generating an environment imports.
            let export = Export::Global {
                local: unsafe {
                    GlobalPointer::new(std::mem::transmute::<&LocalGlobal, *mut LocalGlobal>(
                        &local_global,
                    ))
                },
                global: GlobalDesc { mutable, ty },
            };

            // Create a tuple from the data.
            let global = (local_global, export);

            // Add the data to respective namespace.
            if let Some(namespace) = globals.get_mut(&namespace) {
                namespace.insert(name, global);
            } else {
                let mut global_namespace = HashMap::new();
                global_namespace.insert(name, global);
                globals.insert(namespace, global_namespace);
            }
        }

        globals
    }
}

/// EmscriptenImportObject
pub struct EmscriptenImportObject {}

impl EmscriptenImportObject {
    /// Generates an Emscripten environment that can be imported by a
    /// webassembly instance.
    pub fn generate(data: &EmscriptenData) -> ImportObject {
        let mut imports = ImportObject::new();
        let mut keys = Vec::new();
        let functions = EmscriptenImportObject::get_functions();
        let EmscriptenData {
            memories,
            tables,
            globals,
        } = data;

        // Combine all keys.
        keys.extend(memories.keys().into_iter().cloned());
        keys.extend(tables.keys().into_iter().cloned());
        keys.extend(globals.keys().into_iter().cloned());
        keys.extend(functions.keys().into_iter().cloned());

        println!("globals = {:#?}", globals);

        // Remove duplicate entires.
        let set: HashSet<_> = keys.drain(..).collect();
        keys.extend(set.into_iter());

        // Iterate all namespaces.
        for key in keys {
            // Create new namespace.
            let mut namespace = Namespace::new();

            // Check if memories has such namespace.
            if let Some(memory_namespace) = memories.get(&key) {
                // Combine namespace with the memory namespace.
                // namespace.map.extend(
                //     memory_namespace.into_iter().map(
                //         |(k, v)| (k.clone(), v.2.clone())
                //     )
                // );
            }

            // Check if tables has such namespace.
            if let Some(table_namespace) = tables.get(&key) {
                // Combine namespace with the table namespace.
                // namespace.map.extend(
                //     table_namespace.into_iter().map(
                //         |(k, v)| (k.clone(), v.2.clone())
                //     )
                // );
            }

            // Check if globals has such namespace.
            if let Some(global_namespace) = globals.get(&key) {
                // Combine namespace with the global namespace.
                // namespace.map.extend(
                //     global_namespace.into_iter().map(
                //         |(k, v)| (k.clone(), v.1.clone())
                //     )
                // );
            }

            // Check if functions has such namespace.
            if let Some(function_namespace) = functions.get(&key) {
                // Combine namespace with the function namespace.
                // namespace.map.extend(
                //     function_namespace.map.iter().map(
                //         |(k, v)| (k.clone(), v.clone())
                //     )
                // );
            }

            // Register namespace in imports
            imports.register(key, namespace);
        }

        imports
    }

    /// Exposes all functions in Emscripten environment.
    pub fn get_functions() -> HashMap<String, Namespace> {
        let mut namespaces = HashMap::new();
        let mut env_namespace = Namespace::new();
        let mut asm_namespace = Namespace::new();

        // Print function
        env_namespace.insert(
            "printf",
            Export::Function {
                func: func!(io, printf),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "putchar",
            Export::Function {
                func: func!(io, putchar),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        // Lock
        env_namespace.insert(
            "___lock",
            Export::Function {
                func: func!(lock, ___lock),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "___unlock",
            Export::Function {
                func: func!(lock, ___unlock),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "___wait",
            Export::Function {
                func: func!(lock, ___wait),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32, I32],
                    returns: vec![],
                },
            },
        );
        // Env
        env_namespace.insert(
            "_getenv",
            Export::Function {
                func: func!(env, _getenv),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_setenv",
            Export::Function {
                func: func!(env, _setenv),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_putenv",
            Export::Function {
                func: func!(env, _putenv),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_unsetenv",
            Export::Function {
                func: func!(env, _unsetenv),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_getpwnam",
            Export::Function {
                func: func!(env, _getpwnam),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_getgrnam",
            Export::Function {
                func: func!(env, _getgrnam),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___buildEnvironment",
            Export::Function {
                func: func!(env, ___build_environment),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );
        // Errno
        env_namespace.insert(
            "___setErrNo",
            Export::Function {
                func: func!(errno, ___seterrno),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );
        // Syscalls
        env_namespace.insert(
            "___syscall1",
            Export::Function {
                func: func!(syscalls, ___syscall1),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "___syscall3",
            Export::Function {
                func: func!(syscalls, ___syscall3),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall4",
            Export::Function {
                func: func!(syscalls, ___syscall4),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall5",
            Export::Function {
                func: func!(syscalls, ___syscall5),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall6",
            Export::Function {
                func: func!(syscalls, ___syscall6),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall12",
            Export::Function {
                func: func!(syscalls, ___syscall12),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall20",
            Export::Function {
                func: func!(syscalls, ___syscall20),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall39",
            Export::Function {
                func: func!(syscalls, ___syscall39),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall40",
            Export::Function {
                func: func!(syscalls, ___syscall40),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall54",
            Export::Function {
                func: func!(syscalls, ___syscall54),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall57",
            Export::Function {
                func: func!(syscalls, ___syscall57),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall63",
            Export::Function {
                func: func!(syscalls, ___syscall63),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall64",
            Export::Function {
                func: func!(syscalls, ___syscall64),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall102",
            Export::Function {
                func: func!(syscalls, ___syscall102),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall114",
            Export::Function {
                func: func!(syscalls, ___syscall114),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall122",
            Export::Function {
                func: func!(syscalls, ___syscall122),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall140",
            Export::Function {
                func: func!(syscalls, ___syscall140),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall142",
            Export::Function {
                func: func!(syscalls, ___syscall142),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall145",
            Export::Function {
                func: func!(syscalls, ___syscall145),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall146",
            Export::Function {
                func: func!(syscalls, ___syscall146),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall180",
            Export::Function {
                func: func!(syscalls, ___syscall180),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall181",
            Export::Function {
                func: func!(syscalls, ___syscall181),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall192",
            Export::Function {
                func: func!(syscalls, ___syscall192),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall195",
            Export::Function {
                func: func!(syscalls, ___syscall195),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall197",
            Export::Function {
                func: func!(syscalls, ___syscall197),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall201",
            Export::Function {
                func: func!(syscalls, ___syscall201),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall202",
            Export::Function {
                func: func!(syscalls, ___syscall202),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall212",
            Export::Function {
                func: func!(syscalls, ___syscall212),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall221",
            Export::Function {
                func: func!(syscalls, ___syscall221),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall330",
            Export::Function {
                func: func!(syscalls, ___syscall330),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___syscall340",
            Export::Function {
                func: func!(syscalls, ___syscall340),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );
        // Process
        env_namespace.insert(
            "abort",
            Export::Function {
                func: func!(process, em_abort),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_abort",
            Export::Function {
                func: func!(process, _abort),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "abortStackOverflow",
            Export::Function {
                func: func!(process, abort_stack_overflow),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_llvm_trap",
            Export::Function {
                func: func!(process, _llvm_trap),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_fork",
            Export::Function {
                func: func!(process, _fork),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_exit",
            Export::Function {
                func: func!(process, _exit),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "_system",
            Export::Function {
                func: func!(process, _system),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_popen",
            Export::Function {
                func: func!(process, _popen),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        // Signal
        env_namespace.insert(
            "_sigemptyset",
            Export::Function {
                func: func!(signal, _sigemptyset),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_sigaddset",
            Export::Function {
                func: func!(signal, _sigaddset),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_sigprocmask",
            Export::Function {
                func: func!(signal, _sigprocmask),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_sigaction",
            Export::Function {
                func: func!(signal, _sigaction),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_signal",
            Export::Function {
                func: func!(signal, _signal),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );
        // Memory
        env_namespace.insert(
            "abortOnCannotGrowMemory",
            Export::Function {
                func: func!(memory, abort_on_cannot_grow_memory),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_emscripten_memcpy_big",
            Export::Function {
                func: func!(memory, _emscripten_memcpy_big),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "enlargeMemory",
            Export::Function {
                func: func!(memory, enlarge_memory),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "getTotalMemory",
            Export::Function {
                func: func!(memory, get_total_memory),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___map_file",
            Export::Function {
                func: func!(memory, ___map_file),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );
        // Exception
        env_namespace.insert(
            "___cxa_allocate_exception",
            Export::Function {
                func: func!(exception, ___cxa_allocate_exception),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___cxa_allocate_exception",
            Export::Function {
                func: func!(exception, ___cxa_throw),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "___cxa_throw",
            Export::Function {
                func: func!(exception, ___cxa_throw),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32],
                    returns: vec![],
                },
            },
        );
        // NullFuncs
        env_namespace.insert(
            "nullFunc_ii",
            Export::Function {
                func: func!(nullfunc, nullfunc_ii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_iii",
            Export::Function {
                func: func!(nullfunc, nullfunc_iii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_iiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_iiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_iiiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_iiiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_iiiiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_iiiiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_v",
            Export::Function {
                func: func!(nullfunc, nullfunc_v),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_vi",
            Export::Function {
                func: func!(nullfunc, nullfunc_vi),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_vii",
            Export::Function {
                func: func!(nullfunc, nullfunc_vii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_viii",
            Export::Function {
                func: func!(nullfunc, nullfunc_viii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_viiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_viiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_viiiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_viiiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );

        env_namespace.insert(
            "nullFunc_viiiiii",
            Export::Function {
                func: func!(nullfunc, nullfunc_viiiiii),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![],
                },
            },
        );
        // Time
        env_namespace.insert(
            "_gettimeofday",
            Export::Function {
                func: func!(time, _gettimeofday),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_clock_gettime",
            Export::Function {
                func: func!(time, _clock_gettime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "___clock_gettime",
            Export::Function {
                func: func!(time, ___clock_gettime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_clock",
            Export::Function {
                func: func!(time, _clock),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_difftime",
            Export::Function {
                func: func!(time, _difftime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![F64],
                },
            },
        );

        env_namespace.insert(
            "_asctime",
            Export::Function {
                func: func!(time, _asctime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_asctime_r",
            Export::Function {
                func: func!(time, _asctime_r),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_localtime",
            Export::Function {
                func: func!(time, _localtime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_time",
            Export::Function {
                func: func!(time, _time),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_strftime",
            Export::Function {
                func: func!(time, _strftime),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32, I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_localtime_r",
            Export::Function {
                func: func!(time, _localtime_r),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_getpagesize",
            Export::Function {
                func: func!(env, _getpagesize),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "_sysconf",
            Export::Function {
                func: func!(env, _sysconf),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        // Math
        asm_namespace.insert(
            "f64-rem",
            Export::Function {
                func: func!(math, f64_rem),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![F64, F64],
                    returns: vec![F64],
                },
            },
        );

        env_namespace.insert(
            "_llvm_log10_f64",
            Export::Function {
                func: func!(math, _llvm_log10_f64),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![F64],
                    returns: vec![F64],
                },
            },
        );

        env_namespace.insert(
            "_llvm_log2_f64",
            Export::Function {
                func: func!(math, _llvm_log2_f64),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![F64],
                    returns: vec![F64],
                },
            },
        );

        env_namespace.insert(
            "_llvm_log10_f32",
            Export::Function {
                func: func!(math, _llvm_log10_f32),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![F64],
                    returns: vec![F64],
                },
            },
        );

        env_namespace.insert(
            "_llvm_log2_f32",
            Export::Function {
                func: func!(math, _llvm_log2_f32),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![F64],
                    returns: vec![F64],
                },
            },
        );

        // Jmp
        env_namespace.insert(
            "__setjmp",
            Export::Function {
                func: func!(jmp, __setjmp),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32],
                    returns: vec![I32],
                },
            },
        );

        env_namespace.insert(
            "__longjmp",
            Export::Function {
                func: func!(jmp, __longjmp),
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![I32, I32],
                    returns: vec![],
                },
            },
        );

        // println!("env_namespace = {:#?}", env_namespace);

        // mock_external!(env_namespace, _waitpid);
        // mock_external!(env_namespace, _utimes);
        // mock_external!(env_namespace, _usleep);
        // // mock_external!(env_namespace, _time);
        // // mock_external!(env_namespace, _sysconf);
        // // mock_external!(env_namespace, _strftime);
        // mock_external!(env_namespace, _sigsuspend);
        // // mock_external!(env_namespace, _sigprocmask);
        // // mock_external!(env_namespace, _sigemptyset);
        // // mock_external!(env_namespace, _sigaddset);
        // // mock_external!(env_namespace, _sigaction);
        // mock_external!(env_namespace, _setitimer);
        // mock_external!(env_namespace, _setgroups);
        // mock_external!(env_namespace, _setgrent);
        // mock_external!(env_namespace, _sem_wait);
        // mock_external!(env_namespace, _sem_post);
        // mock_external!(env_namespace, _sem_init);
        // mock_external!(env_namespace, _sched_yield);
        // mock_external!(env_namespace, _raise);
        mock_external!(env_namespace, _mktime, [I32 => I32]);
        // // mock_external!(env_namespace, _localtime_r);
        // // mock_external!(env_namespace, _localtime);
        // mock_external!(env_namespace, _llvm_stacksave);
        // mock_external!(env_namespace, _llvm_stackrestore);
        // mock_external!(env_namespace, _kill);
        mock_external!(env_namespace, _gmtime_r, [I32, I32 => I32]);
        // // mock_external!(env_namespace, _gettimeofday);
        // // mock_external!(env_namespace, _getpagesize);
        // mock_external!(env_namespace, _getgrent);
        // mock_external!(env_namespace, _getaddrinfo);
        // // mock_external!(env_namespace, _fork);
        // // mock_external!(env_namespace, _exit);
        // mock_external!(env_namespace, _execve);
        // mock_external!(env_namespace, _endgrent);
        // // mock_external!(env_namespace, _clock_gettime);
        // mock_external!(env_namespace, ___syscall97);
        mock_external!(env_namespace, ___syscall91, [I32, I32 => I32]);
        // mock_external!(env_namespace, ___syscall85);
        // mock_external!(env_namespace, ___syscall75);
        // mock_external!(env_namespace, ___syscall66);
        // // mock_external!(env_namespace, ___syscall64);
        // // mock_external!(env_namespace, ___syscall63);
        // // mock_external!(env_namespace, ___syscall60);
        // // mock_external!(env_namespace, ___syscall54);
        // // mock_external!(env_namespace, ___syscall39);
        mock_external!(env_namespace, ___syscall38, [I32, I32 => I32]);
        // // mock_external!(env_namespace, ___syscall340);
        // mock_external!(env_namespace, ___syscall334);
        // mock_external!(env_namespace, ___syscall300);
        // mock_external!(env_namespace, ___syscall295);
        // mock_external!(env_namespace, ___syscall272);
        // mock_external!(env_namespace, ___syscall268);
        // // mock_external!(env_namespace, ___syscall221);
        // mock_external!(env_namespace, ___syscall220);
        // // mock_external!(env_namespace, ___syscall212);
        // // mock_external!(env_namespace, ___syscall201);
        // mock_external!(env_namespace, ___syscall199);
        // // mock_external!(env_namespace, ___syscall197);
        // mock_external!(env_namespace, ___syscall196);
        // // mock_external!(env_namespace, ___syscall195);
        // mock_external!(env_namespace, ___syscall194);
        // mock_external!(env_namespace, ___syscall191);
        // // mock_external!(env_namespace, ___syscall181);
        // // mock_external!(env_namespace, ___syscall180);
        // mock_external!(env_namespace, ___syscall168);
        // // mock_external!(env_namespace, ___syscall146);
        // // mock_external!(env_namespace, ___syscall145);
        // // mock_external!(env_namespace, ___syscall142);
        // mock_external!(env_namespace, ___syscall140);
        // // mock_external!(env_namespace, ___syscall122);
        // // mock_external!(env_namespace, ___syscall102);
        // // mock_external!(env_namespace, ___syscall20);
        // mock_external!(env_namespace, ___syscall15);
        mock_external!(env_namespace, ___syscall10, [I32, I32 => I32]);
        mock_external!(env_namespace, _dlopen, [I32, I32 => I32]);
        mock_external!(env_namespace, _dlclose, [I32 => I32]);
        mock_external!(env_namespace, _dlsym, [I32, I32 => I32]);
        mock_external!(env_namespace, _dlerror, [ => I32]);

        namespaces.insert("env".into(), env_namespace);
        namespaces.insert("asm2wasm".into(), asm_namespace);

        namespaces
    }
}

// pub struct EmscriptenLocalFunctions {
//    pub malloc: extern "C" fn(i32, &Instance) -> u32,
//    pub free: extern "C" fn(i32, &mut Instance),
//    pub memalign: extern "C" fn(u32, u32, &mut Instance) -> u32,
//    pub memset: extern "C" fn(u32, i32, u32, &mut Instance) -> u32,
//    pub stack_alloc: extern "C" fn(u32, &Instance) -> u32,
//    pub jumps: Vec<UnsafeCell<[c_int; 27]>>,
// }
