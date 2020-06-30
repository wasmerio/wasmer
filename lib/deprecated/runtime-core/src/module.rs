use crate::{
    cache::Artifact,
    error::{InstantiationError, RuntimeError},
    import::{ImportObject, Namespace},
    instance::{Instance, PreInstance},
    new,
    typed_func::DynamicCtx,
    types::{FuncSig, Value},
    vm,
};
use new::wasmer_runtime::Export;
use std::{
    cell::RefCell,
    collections::HashMap,
    convert::{AsRef, Infallible},
    ptr,
};

pub use new::wasm_common::{DataInitializer, ExportIndex};
pub use new::wasmer_runtime::{
    //
    MemoryStyle as MemoryType,
    ModuleInfo,
    TableElements as TableInitializer,
};

#[derive(Clone)]
pub struct Module {
    pub(crate) new_module: new::wasmer::Module,
}

impl Module {
    pub(crate) fn new(new_module: new::wasmer::Module) -> Self {
        Self { new_module }
    }

    pub fn instantiate(
        &self,
        import_object: &crate::import::ImportObject,
    ) -> Result<Instance, InstantiationError> {
        let pre_instance = Box::new(PreInstance::new());

        let import_object = {
            // Replace the fake `vm::Ctx` of host functions.
            // Since `ImportObject` has a readonly API, we have no
            // choice than rebuilding an entire `ImportObject`.

            let mut new_import_object = ImportObject::new();
            let mut new_namespaces: HashMap<String, Namespace> = HashMap::new();
            let store = self.new_module.store();

            import_object
                .clone_ref()
                .into_iter()
                .map(|((namespace, name), export)| match export {
                    Export::Function(mut function) => {
                        // That's an ugly hack. Go your way :-].
                        {
                            // That's a static host function
                            // constructed with
                            // `new::wasmer::Function::new_env`.
                            if !function.address.is_null() {
                                // An empty `vm::Ctx` was created in
                                // `Func` as a host function
                                // environment. The internals of
                                // `new::wasmer::externals::Function`
                                // passes the environment inside the
                                // `VMContext` pointer. That's another
                                // hack.  This empty `vm::Ctx` must be
                                // replaced by another `vm::Ctx` value
                                // owned by `Instance`, because that's the
                                // only way to update this structure
                                // correctly for compatibility
                                // purposes. Before doing this operation,
                                // we must drop the empty `vm::Ctx` first.
                                unsafe {
                                    ptr::drop_in_place::<vm::Ctx>(function.vmctx as _);
                                }

                                // And now we can update the pointer to
                                // `VMContext`, which is actually a
                                // `vm::Ctx` pointer, to fallback on the
                                // environment hack.
                                function.vmctx = pre_instance.vmctx_ptr() as _;
                            }
                            // That's a dynamic host function
                            // constructed with
                            // `new::wasmer::Function::new_dynamic_env`.
                            else {
                                // `new::wasmer::VMDynamicFunctionWithEnv`
                                // is private to `new::wasmer`. Let's
                                // replicate it, and hope the layout
                                // is the same!
                                struct VMDynamicFunctionWithEnv<Env>
                                where
                                    Env: Sized + 'static,
                                {
                                    #[allow(unused)]
                                    function_type: FuncSig,
                                    #[allow(unused)]
                                    func: Box<
                                        dyn Fn(
                                                &mut Env,
                                                &[Value],
                                            )
                                                -> Result<Vec<Value>, RuntimeError>
                                            + 'static,
                                    >,
                                    env: RefCell<Env>,
                                }

                                // Get back the `vmctx` as it is
                                // stored by
                                // `new::wasmer::Function::new_dynamic_env`.
                                let vmctx: Box<
                                    new::wasmer_runtime::VMDynamicFunctionContext<
                                        VMDynamicFunctionWithEnv<DynamicCtx>,
                                    >,
                                > = unsafe { Box::from_raw(function.vmctx as *mut _) };

                                // Replace the environment by ours.
                                vmctx.ctx.env.borrow_mut().vmctx = pre_instance.vmctx();

                                // … without anyone noticing…
                                function.vmctx = Box::into_raw(vmctx) as _;
                            }
                        }

                        (
                            (namespace, name),
                            new::wasmer::Extern::from_export(store, Export::Function(function)),
                        )
                    }
                    export => (
                        (namespace, name),
                        new::wasmer::Extern::from_export(store, export),
                    ),
                })
                .for_each(|((namespace, name), extern_)| {
                    if !new_namespaces.contains_key(&namespace) {
                        new_namespaces.insert(namespace.clone(), Namespace::new());
                    }

                    let new_namespace = new_namespaces.get_mut(&namespace).unwrap(); // it is safe because it has been verified that the key exists.
                    new_namespace.insert(&name, extern_);
                });

            new_namespaces
                .into_iter()
                .for_each(|(namespace_name, namespace)| {
                    new_import_object.register(namespace_name, namespace);
                });

            new_import_object
        };

        Ok(Instance::new(
            pre_instance,
            new::wasmer::Instance::new(&self.new_module, &import_object)?,
        ))
    }

    pub fn cache(&self) -> Result<Artifact, Infallible> {
        Ok(Artifact::new(self.new_module.clone()))
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.new_module.info()
    }

    pub fn imports(&self) -> Vec<crate::types::ImportDescriptor> {
        self.new_module.imports().collect()
    }

    pub fn exports(&self) -> Vec<crate::types::ExportDescriptor> {
        self.new_module.exports().collect()
    }

    pub fn custom_sections<'a>(&self, name: impl AsRef<str>) -> Option<Vec<Vec<u8>>> {
        let custom_sections: Vec<Vec<u8>> = self
            .new_module
            .custom_sections(name.as_ref())
            .map(|custom_section| custom_section.to_vec())
            .collect();

        if custom_sections.is_empty() {
            None
        } else {
            Some(custom_sections)
        }
    }
}
