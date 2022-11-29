//! The import module contains the implementation data structures and helper functions used to
//! manipulate and access a wasm module's imports including memories, tables, globals, and
//! functions.
use crate::js::error::{InstantiationError, LinkError, WasmError};
use crate::js::export::Export;
use crate::js::exports::Exports;
use crate::js::module::Module;
use crate::js::store::{AsStoreMut, AsStoreRef};
use crate::js::types::AsJs;
use crate::js::ExternType;
use crate::Extern;
use std::collections::HashMap;
use std::fmt;
use wasmer_types::ImportError;

/// All of the import data used when instantiating.
///
/// It's suggested that you use the [`imports!`] macro
/// instead of creating an `Imports` by hand.
///
/// [`imports!`]: macro.imports.html
///
/// # Usage:
/// ```no_run
/// use wasmer::{Exports, Module, Store, Instance, imports, Imports, Function};
/// # fn foo_test(module: Module, store: Store) {
///
/// let host_fn = Function::new_typed(foo);
/// let import_object: Imports = imports! {
///     "env" => {
///         "foo" => host_fn,
///     },
/// };
///
/// let instance = Instance::new(&module, &import_object).expect("Could not instantiate module.");
///
/// fn foo(n: i32) -> i32 {
///     n
/// }
///
/// # }
/// ```
#[derive(Clone, Default)]
pub struct Imports {
    map: HashMap<(String, String), Extern>,
}

impl Imports {
    /// Create a new `Imports`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Gets an export given a ns and a name
    ///
    /// # Usage
    /// ```no_run
    /// # use wasmer::Imports;
    /// let mut import_object = Imports::new();
    /// import_object.get_export("ns", "name");
    /// ```
    pub fn get_export(&self, ns: &str, name: &str) -> Option<Extern> {
        if self.map.contains_key(&(ns.to_string(), name.to_string())) {
            let ext = &self.map[&(ns.to_string(), name.to_string())];
            return Some(ext.clone());
        }
        None
    }

    /// Returns true if the Imports contains namespace with the provided name.
    pub fn contains_namespace(&self, name: &str) -> bool {
        self.map.keys().any(|(k, _)| (k == name))
    }

    /// Register a list of externs into a namespace.
    ///
    /// # Usage:
    /// ```no_run
    /// # use wasmer::{Imports, Exports, Memory};
    /// # fn foo_test(memory: Memory) {
    /// let mut exports = Exports::new()
    /// exports.insert("memory", memory);
    ///
    /// let mut import_object = Imports::new();
    /// import_object.register_namespace("env", exports);
    /// // ...
    /// # }
    /// ```
    pub fn register_namespace(
        &mut self,
        ns: &str,
        contents: impl IntoIterator<Item = (String, Extern)>,
    ) {
        for (name, extern_) in contents.into_iter() {
            self.map.insert((ns.to_string(), name.clone()), extern_);
        }
    }

    /// Add a single import with a namespace `ns` and name `name`.
    ///
    /// # Usage
    /// ```no_run
    /// # let mut store = Default::default();
    /// use wasmer::{Imports, Function};
    /// fn foo(n: i32) -> i32 {
    ///     n
    /// }
    /// let mut import_object = Imports::new();
    /// import_object.define("env", "foo", Function::new_typed(&store, foo));
    /// ```
    pub fn define(&mut self, ns: &str, name: &str, val: impl Into<Extern>) {
        self.map
            .insert((ns.to_string(), name.to_string()), val.into());
    }

    /// Returns the contents of a namespace as an `Exports`.
    ///
    /// Returns `None` if the namespace doesn't exist.
    pub fn get_namespace_exports(&self, name: &str) -> Option<Exports> {
        let ret: Exports = self
            .map
            .iter()
            .filter(|((ns, _), _)| ns == name)
            .map(|((_, name), e)| (name.clone(), e.clone()))
            .collect();
        if ret.is_empty() {
            None
        } else {
            Some(ret)
        }
    }

    /// Resolve and return a vector of imports in the order they are defined in the `module`'s source code.
    ///
    /// This means the returned `Vec<Extern>` might be a subset of the imports contained in `self`.
    pub fn imports_for_module(
        &self,
        module: &Module,
        _store: &mut impl AsStoreMut,
    ) -> Result<Vec<Extern>, LinkError> {
        let mut ret = vec![];
        for import in module.imports() {
            if let Some(imp) = self
                .map
                .get(&(import.module().to_string(), import.name().to_string()))
            {
                ret.push(imp.clone());
            } else {
                return Err(LinkError::Import(
                    import.module().to_string(),
                    import.name().to_string(),
                    ImportError::UnknownImport(import.ty().clone()),
                ));
            }
        }
        Ok(ret)
    }

    /// Returns the `Imports` as a Javascript `Object`
    pub fn as_jsobject(&self, store: &impl AsStoreRef) -> js_sys::Object {
        let imports = js_sys::Object::new();
        let namespaces: HashMap<&str, Vec<(&str, &Extern)>> =
            self.map
                .iter()
                .fold(HashMap::default(), |mut acc, ((ns, name), ext)| {
                    acc.entry(ns.as_str())
                        .or_default()
                        .push((name.as_str(), ext));
                    acc
                });

        for (ns, exports) in namespaces.into_iter() {
            let import_namespace = js_sys::Object::new();
            for (name, ext) in exports {
                js_sys::Reflect::set(&import_namespace, &name.into(), &ext.as_jsvalue(store))
                    .expect("Error while setting into the js namespace object");
            }
            js_sys::Reflect::set(&imports, &ns.into(), &import_namespace.into())
                .expect("Error while setting into the js imports object");
        }
        imports
    }

    /// Iterates through all the imports in this structure
    pub fn iter<'a>(&'a self) -> ImportsIterator<'a> {
        ImportsIterator::new(self)
    }

    /// Create a new `Imports` from a JS Object, it receives a reference to a `Module` to
    /// map and assign the types of each import and the JS Object
    /// that contains the values of imports.
    ///
    /// # Usage
    /// ```ignore
    /// let import_object = Imports::new_from_js_object(&mut store, &module, js_object);
    /// ```
    pub fn new_from_js_object(
        store: &mut impl AsStoreMut,
        module: &Module,
        object: js_sys::Object,
    ) -> Result<Self, WasmError> {
        use crate::js::externals::VMExtern;
        let module_imports: HashMap<(String, String), ExternType> = module
            .imports()
            .map(|import| {
                (
                    (import.module().to_string(), import.name().to_string()),
                    import.ty().clone(),
                )
            })
            .collect::<HashMap<(String, String), ExternType>>();

        let mut map: HashMap<(String, String), Extern> = HashMap::new();

        for module_entry in js_sys::Object::entries(&object).iter() {
            let module_entry: js_sys::Array = module_entry.into();
            let module_name = module_entry.get(0).as_string().unwrap().to_string();
            let module_import_object: js_sys::Object = module_entry.get(1).into();
            for import_entry in js_sys::Object::entries(&module_import_object).iter() {
                let import_entry: js_sys::Array = import_entry.into();
                let import_name = import_entry.get(0).as_string().unwrap().to_string();
                let import_js: wasm_bindgen::JsValue = import_entry.get(1);
                let key = (module_name.clone(), import_name);
                let extern_type = module_imports.get(&key).unwrap();
                let export = VMExtern::from_js_value(import_js, store, extern_type.clone())?;
                let extern_ = Extern::from_vm_extern(store, export);
                map.insert(key, extern_);
            }
        }

        Ok(Self { map })
    }
}

impl AsJs for Imports {
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        let imports_object = js_sys::Object::new();
        for (namespace, name, extern_) in self.iter() {
            let val = js_sys::Reflect::get(&imports_object, &namespace.into()).unwrap();
            if !val.is_undefined() {
                // If the namespace is already set
                js_sys::Reflect::set(
                    &val,
                    &name.into(),
                    &extern_.as_jsvalue(&store.as_store_ref()),
                )
                .unwrap();
            } else {
                // If the namespace doesn't exist
                let import_namespace = js_sys::Object::new();
                js_sys::Reflect::set(
                    &import_namespace,
                    &name.into(),
                    &extern_.as_jsvalue(&store.as_store_ref()),
                )
                .unwrap();
                js_sys::Reflect::set(&imports_object, &namespace.into(), &import_namespace.into())
                    .unwrap();
            }
        }
        imports_object.into()
    }
}

pub struct ImportsIterator<'a> {
    iter: std::collections::hash_map::Iter<'a, (String, String), Extern>,
}

impl<'a> ImportsIterator<'a> {
    fn new(imports: &'a Imports) -> Self {
        let iter = imports.map.iter();
        Self { iter }
    }
}

impl<'a> Iterator for ImportsIterator<'a> {
    type Item = (&'a str, &'a str, &'a Extern);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|(k, v)| (k.0.as_str(), k.1.as_str(), v))
    }
}

impl IntoIterator for &Imports {
    type IntoIter = std::collections::hash_map::IntoIter<(String, String), Extern>;
    type Item = ((String, String), Extern);

    fn into_iter(self) -> Self::IntoIter {
        self.map.clone().into_iter()
    }
}

impl Extend<((String, String), Extern)> for Imports {
    fn extend<T: IntoIterator<Item = ((String, String), Extern)>>(&mut self, iter: T) {
        for ((ns, name), ext) in iter.into_iter() {
            self.define(&ns, &name, ext);
        }
    }
}

impl fmt::Debug for Imports {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        enum SecretMap {
            Empty,
            Some(usize),
        }

        impl SecretMap {
            fn new(len: usize) -> Self {
                if len == 0 {
                    Self::Empty
                } else {
                    Self::Some(len)
                }
            }
        }

        impl fmt::Debug for SecretMap {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self {
                    Self::Empty => write!(f, "(empty)"),
                    Self::Some(len) => write!(f, "(... {} item(s) ...)", len),
                }
            }
        }

        f.debug_struct("Imports")
            .field("map", &SecretMap::new(self.map.len()))
            .finish()
    }
}

// The import! macro for Imports

/// Generate an [`Imports`] easily with the `imports!` macro.
///
/// [`Imports`]: struct.Imports.html
///
/// # Usage
///
/// ```
/// # use wasmer::{Function, Store};
/// # let mut store = Store::default();
/// use wasmer::imports;
///
/// let import_object = imports! {
///     "env" => {
///         "foo" => Function::new_typed(&store, foo)
///     },
/// };
///
/// fn foo(n: i32) -> i32 {
///     n
/// }
/// ```
#[macro_export]
macro_rules! imports {
    ( $( $ns_name:expr => $ns:tt ),* $(,)? ) => {
        {
            let mut import_object = $crate::Imports::new();

            $({
                let namespace = $crate::import_namespace!($ns);

                import_object.register_namespace($ns_name, namespace);
            })*

            import_object
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! namespace {
    ($( $import_name:expr => $import_item:expr ),* $(,)? ) => {
        $crate::import_namespace!( { $( $import_name => $import_item, )* } )
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! import_namespace {
    ( { $( $import_name:expr => $import_item:expr ),* $(,)? } ) => {{
        let mut namespace = $crate::Exports::new();

        $(
            namespace.insert($import_name, $import_item);
        )*

        namespace
    }};

    ( $namespace:ident ) => {
        $namespace
    };
}

#[cfg(test)]
mod test {
    use crate::js::{Global, Store, Value};

    // use wasm_bindgen::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn chaining_works() {
        let mut store = Store::default();

        let g = Global::new(&mut store, Value::I32(0));

        let mut imports1 = imports! {
            "dog" => {
                "happy" => g.clone()
            }
        };

        let imports2 = imports! {
            "dog" => {
                "small" => g.clone()
            },
            "cat" => {
                "small" => g
            }
        };

        imports1.extend(&imports2);

        let small_cat_export = imports1.get_export("cat", "small");
        assert!(small_cat_export.is_some());

        let happy = imports1.get_export("dog", "happy");
        let small = imports1.get_export("dog", "small");
        assert!(happy.is_some());
        assert!(small.is_some());
    }
    // fn namespace() {
    //     let mut store = Store::default();
    //     let g1 = Global::new(&store, Val::I32(0));
    //     let namespace = namespace! {
    //         "happy" => g1
    //     };
    //     let imports1 = imports! {
    //         "dog" => namespace
    //     };

    //     let happy_dog_entry = imports1.get_export("dog", "happy").unwrap();

    //     assert!(
    //         if let Export::Global(happy_dog_global) = happy_dog_entry.to_export() {
    //             happy_dog_global.ty.ty == Type::I32
    //         } else {
    //             false
    //         }
    //     );
    // }

    // fn imports_macro_allows_trailing_comma_and_none() {
    //     use crate::js::Function;

    //     let mut store = Default::default();

    //     fn func(arg: i32) -> i32 {
    //         arg + 1
    //     }

    //     let _ = imports! {
    //         "env" => {
    //             "func" => Function::new_typed(&store, func),
    //         },
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func" => Function::new_typed(&store, func),
    //         }
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func" => Function::new_typed(&store, func),
    //         },
    //         "abc" => {
    //             "def" => Function::new_typed(&store, func),
    //         }
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func" => Function::new_typed(&store, func)
    //         },
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func" => Function::new_typed(&store, func)
    //         }
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func1" => Function::new_typed(&store, func),
    //             "func2" => Function::new_typed(&store, func)
    //         }
    //     };
    //     let _ = imports! {
    //         "env" => {
    //             "func1" => Function::new_typed(&store, func),
    //             "func2" => Function::new_typed(&store, func),
    //         }
    //     };
    // }

    // fn chaining_works() {
    //     let mut store = Store::default();
    //     let g = Global::new(&store, Val::I32(0));

    //     let mut imports1 = imports! {
    //         "dog" => {
    //             "happy" => g.clone()
    //         }
    //     };

    //     let imports2 = imports! {
    //         "dog" => {
    //             "small" => g.clone()
    //         },
    //         "cat" => {
    //             "small" => g.clone()
    //         }
    //     };

    //     imports1.extend(&imports2);

    //     let small_cat_export = imports1.get_export("cat", "small");
    //     assert!(small_cat_export.is_some());

    //     let happy = imports1.get_export("dog", "happy");
    //     let small = imports1.get_export("dog", "small");
    //     assert!(happy.is_some());
    //     assert!(small.is_some());
    // }

    // fn extending_conflict_overwrites() {
    //     let mut store = Store::default();
    //     let g1 = Global::new(&store, Val::I32(0));
    //     let g2 = Global::new(&store, Val::F32(0.));

    //     let mut imports1 = imports! {
    //         "dog" => {
    //             "happy" => g1,
    //         },
    //     };

    //     let imports2 = imports! {
    //         "dog" => {
    //             "happy" => g2,
    //         },
    //     };

    //     imports1.extend(&imports2);
    //     let happy_dog_entry = imports1.get_export("dog", "happy").unwrap();

    //     assert!(
    //         if let Export::Global(happy_dog_global) = happy_dog_entry.to_export() {
    //             happy_dog_global.ty.ty == Type::F32
    //         } else {
    //             false
    //         }
    //     );

    //     // now test it in reverse
    //     let mut store = Store::default();
    //     let g1 = Global::new(&store, Val::I32(0));
    //     let g2 = Global::new(&store, Val::F32(0.));

    //     let imports1 = imports! {
    //         "dog" => {
    //             "happy" => g1,
    //         },
    //     };

    //     let mut imports2 = imports! {
    //         "dog" => {
    //             "happy" => g2,
    //         },
    //     };

    //     imports2.extend(&imports1);
    //     let happy_dog_entry = imports2.get_export("dog", "happy").unwrap();

    //     assert!(
    //         if let Export::Global(happy_dog_global) = happy_dog_entry.to_export() {
    //             happy_dog_global.ty.ty == Type::I32
    //         } else {
    //             false
    //         }
    //     );
    // }
}
