//! The import module contains the implementation data structures and helper functions used to
//! manipulate and access a wasm module's imports including memories, tables, globals, and
//! functions.
use crate::js::export::Export;
use crate::js::exports::Exportable;
use crate::js::resolver::NamedResolver;
use crate::Extern;
use std::borrow::{Borrow, BorrowMut};
use std::collections::{hash_map::Entry, HashMap};
use std::fmt;

/// All of the import data used when instantiating.
///
/// It's suggested that you use the [`imports!`] macro
/// instead of creating an `Imports` by hand.
///
/// [`imports!`]: macro.imports.html
///
/// # Usage:
/// ```ignore
/// use wasmer::{Exports, Imports, Function};
///
/// let mut import_object = Imports::new();
/// let mut env = Exports::new();
///
/// env.insert("foo", Function::new_native(foo));
/// import_object.register("env", env);
///
/// fn foo(n: i32) -> i32 {
///     n
/// }
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
    /// ```ignore
    /// # use wasmer::{Imports, Instance, Namespace};
    /// let mut import_object = Imports::new();
    /// import_object.get_export("ns", "name");
    /// ```
    pub fn get_export(&self, ns: &str, name: &str) -> Option<Export> {
        if self.map.contains_key(&(ns.to_string(), name.to_string())) {
            let ext = &self.map[&(ns.to_string(), name.to_string())];
            return Some(ext.to_export());
        }
        None
    }

    /// Returns true if the Imports contains namespace with the provided name.
    pub fn contains_namespace(&self, name: &str) -> bool {
        self.map.keys().any(|(k, _)| (k == name))
    }

    /// TODO: Add doc
    pub fn register_namespace(
        &mut self,
        ns: &str,
        contents: impl IntoIterator<Item = (String, Extern)>,
    ) {
        for (name, extern_) in contents.into_iter() {
            self.map.insert((ns.to_string(), name.clone()), extern_);
        }
    }

    /// TODO: Add doc
    pub fn define(&mut self, ns: &str, name: &str, extern_: Extern) {
        self.map.insert((ns.to_string(), name.to_string()), extern_);
    }

    // /// Register anything that implements `LikeNamespace` as a namespace.
    // ///
    // /// # Usage:
    // /// ```ignore
    // /// # use wasmer::{Imports, Instance, Namespace};
    // /// let mut import_object = Imports::new();
    // ///
    // /// import_object.register("namespace0", instance);
    // /// import_object.register("namespace1", namespace);
    // /// // ...
    // /// ```
    // pub fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn LikeNamespace>>
    // where
    //     S: Into<String>,
    //     N: LikeNamespace + Send + Sync + 'static,
    // {
    //     let mut guard = self.map.lock().unwrap();
    //     let map = guard.borrow_mut();

    //     match map.entry(name.into()) {
    //         Entry::Vacant(empty) => {
    //             empty.insert(Box::new(namespace));
    //             None
    //         }
    //         Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
    //     }
    // }

    /// asdfsa
    pub fn resolve_by_name(&self, ns: &str, name: &str) -> Option<Export> {
        self.get_export(ns, name)
    }

    //fn iter(&self) -> impl Iterator<(&str, &str, Extern)> {
    //    todo!()
    //}

    /// Returns the `Imports` as a Javascript `Object`
    pub fn as_jsobject(&self) -> js_sys::Object {
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
                js_sys::Reflect::set(
                    &import_namespace,
                    &name.into(),
                    ext.to_export().as_jsvalue(),
                )
                .expect("Error while setting into the js namespace object");
            }
            js_sys::Reflect::set(&imports, &ns.into(), &import_namespace.into())
                .expect("Error while setting into the js imports object");
        }
        imports
    }
}

impl Into<js_sys::Object> for Imports {
    fn into(self) -> js_sys::Object {
        self.as_jsobject()
    }
}

impl NamedResolver for Imports {
    fn resolve_by_name(&self, ns: &str, name: &str) -> Option<Export> {
        self.get_export(ns, name)
    }
}

impl IntoIterator for &Imports {
    type IntoIter = std::collections::hash_map::IntoIter<(String, String), Extern>;
    type Item = ((String, String), Extern);

    fn into_iter(self) -> Self::IntoIter {
        self.map.clone().into_iter()
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
/// # let store = Store::default();
/// use wasmer::imports;
///
/// let import_object = imports! {
///     "env" => {
///         "foo" => Function::new_native(&store, foo)
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
    use super::*;
    use crate::js::ChainableNamedResolver;
    use crate::js::Type;
    use crate::js::{Global, Store, Val};
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn chaining_works() {
        let store = Store::default();
        let g = Global::new(&store, Val::I32(0));

        let imports1 = imports! {
            "dog" => {
                "happy" => g.clone()
            }
        };

        let imports2 = imports! {
            "dog" => {
                "small" => g.clone()
            },
            "cat" => {
                "small" => g.clone()
            }
        };

        let resolver = imports1.chain_front(imports2);

        let small_cat_export = resolver.resolve_by_name("cat", "small");
        assert!(small_cat_export.is_some());

        let happy = resolver.resolve_by_name("dog", "happy");
        let small = resolver.resolve_by_name("dog", "small");
        assert!(happy.is_some());
        assert!(small.is_some());
    }

    #[wasm_bindgen_test]
    fn extending_conflict_overwrites() {
        let store = Store::default();
        let g1 = Global::new(&store, Val::I32(0));
        let g2 = Global::new(&store, Val::F32(0.));

        let imports1 = imports! {
            "dog" => {
                "happy" => g1,
            },
        };

        let imports2 = imports! {
            "dog" => {
                "happy" => g2,
            },
        };

        let resolver = imports1.chain_front(imports2);
        let happy_dog_entry = resolver.resolve_by_name("dog", "happy").unwrap();

        assert!(if let Export::Global(happy_dog_global) = happy_dog_entry {
            happy_dog_global.ty.ty == Type::F32
        } else {
            false
        });

        // now test it in reverse
        let store = Store::default();
        let g1 = Global::new(&store, Val::I32(0));
        let g2 = Global::new(&store, Val::F32(0.));

        let imports1 = imports! {
            "dog" => {
                "happy" => g1,
            },
        };

        let imports2 = imports! {
            "dog" => {
                "happy" => g2,
            },
        };

        let resolver = imports1.chain_back(imports2);
        let happy_dog_entry = resolver.resolve_by_name("dog", "happy").unwrap();

        assert!(if let Export::Global(happy_dog_global) = happy_dog_entry {
            happy_dog_global.ty.ty == Type::I32
        } else {
            false
        });
    }

    #[wasm_bindgen_test]
    fn namespace() {
        let store = Store::default();
        let g1 = Global::new(&store, Val::I32(0));
        let namespace = namespace! {
            "happy" => g1
        };
        let imports1 = imports! {
            "dog" => namespace
        };

        let happy_dog_entry = imports1.resolve_by_name("dog", "happy").unwrap();

        assert!(if let Export::Global(happy_dog_global) = happy_dog_entry {
            happy_dog_global.ty.ty == Type::I32
        } else {
            false
        });
    }

    #[wasm_bindgen_test]
    fn imports_macro_allows_trailing_comma_and_none() {
        use crate::js::Function;

        let store = Default::default();

        fn func(arg: i32) -> i32 {
            arg + 1
        }

        let _ = imports! {
            "env" => {
                "func" => Function::new_native(&store, func),
            },
        };
        let _ = imports! {
            "env" => {
                "func" => Function::new_native(&store, func),
            }
        };
        let _ = imports! {
            "env" => {
                "func" => Function::new_native(&store, func),
            },
            "abc" => {
                "def" => Function::new_native(&store, func),
            }
        };
        let _ = imports! {
            "env" => {
                "func" => Function::new_native(&store, func)
            },
        };
        let _ = imports! {
            "env" => {
                "func" => Function::new_native(&store, func)
            }
        };
        let _ = imports! {
            "env" => {
                "func1" => Function::new_native(&store, func),
                "func2" => Function::new_native(&store, func)
            }
        };
        let _ = imports! {
            "env" => {
                "func1" => Function::new_native(&store, func),
                "func2" => Function::new_native(&store, func),
            }
        };
    }
}
