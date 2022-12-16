//! The definition of a WASM interface

use crate::interface_matcher::InterfaceMatcher;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Entry, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Interface {
    /// The name the interface gave itself
    pub name: Option<String>,
    /// Things that the module can import
    pub imports: HashMap<(String, String), Import>,
    /// Things that the module must export
    pub exports: HashMap<String, Export>,
}

impl Interface {
    pub fn merge(&self, other: Interface) -> Result<Interface, String> {
        let mut base = self.clone();

        for (key, val) in other.imports {
            match base.imports.entry(key) {
                Entry::Occupied(e) if *e.get() != val => {
                    let (namespace, name) = e.key();
                    let original_value = e.get();
                    return Err(format!("Conflict detected: the import \"{namespace}\" \"{name}\" was found but the definitions were different: {original_value:?} {val:?}"));
                }
                Entry::Occupied(_) => {
                    // it's okay for the imported items to be the same.
                }
                Entry::Vacant(e) => {
                    e.insert(val);
                }
            };
        }

        for (key, val) in other.exports {
            match base.exports.entry(key) {
                Entry::Occupied(e) if *e.get() != val => {
                    let name = e.key();
                    let original_value = e.get();
                    return Err(format!("Conflict detected: the key \"{name}\" was found in exports but the definitions were different: {original_value:?} {val:?}"));
                }
                Entry::Occupied(_) => {
                    // it's okay for the exported items to be the same.
                }
                Entry::Vacant(e) => {
                    e.insert(val);
                }
            };
        }

        Ok(base)
    }

    pub fn create_interface_matcher(&self) -> InterfaceMatcher {
        let mut namespaces = HashSet::new();
        let mut namespace_imports: HashMap<String, HashSet<Import>> =
            HashMap::with_capacity(self.imports.len());
        let mut exports = HashSet::with_capacity(self.exports.len());

        for (_, import) in self.imports.iter() {
            match import {
                Import::Func { namespace, .. } | Import::Global { namespace, .. } => {
                    if !namespaces.contains(namespace) {
                        namespaces.insert(namespace.clone());
                    }
                    let ni = namespace_imports.entry(namespace.clone()).or_default();
                    ni.insert(import.clone());
                }
            }
        }
        for (_, export) in self.exports.iter() {
            exports.insert(export.clone());
        }
        InterfaceMatcher {
            namespaces,
            namespace_imports,
            exports,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Import {
    Func {
        namespace: String,
        name: String,
        params: Vec<WasmType>,
        result: Vec<WasmType>,
    },
    Global {
        namespace: String,
        name: String,
        var_type: WasmType,
    },
}

impl Import {
    pub fn format_key(ns: &str, name: &str) -> (String, String) {
        (ns.to_string(), name.to_string())
    }

    /// Get the key used to look this import up in the Interface's import hashmap
    pub fn get_key(&self) -> (String, String) {
        match self {
            Import::Func {
                namespace, name, ..
            }
            | Import::Global {
                namespace, name, ..
            } => Self::format_key(namespace, name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Export {
    Func {
        name: String,
        params: Vec<WasmType>,
        result: Vec<WasmType>,
    },
    Global {
        name: String,
        var_type: WasmType,
    },
}

impl Export {
    pub fn format_key(name: &str) -> String {
        name.to_string()
    }

    /// Get the key used to look this export up in the Interface's export hashmap
    pub fn get_key(&self) -> String {
        match self {
            Export::Func { name, .. } | Export::Global { name, .. } => Self::format_key(name),
        }
    }
}

/// Primitive wasm type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

impl std::fmt::Display for WasmType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                WasmType::I32 => "i32",
                WasmType::I64 => "i64",
                WasmType::F32 => "f32",
                WasmType::F64 => "f64",
            }
        )
    }
}

#[cfg(test)]
mod test {
    use crate::parser;

    #[test]
    fn merging_works() {
        let interface1_src =
            r#"(interface (func (import "env" "plus_one") (param i32) (result i32)))"#;
        let interface2_src =
            r#"(interface (func (import "env" "plus_one") (param i64) (result i64)))"#;
        let interface3_src =
            r#"(interface (func (import "env" "times_two") (param i64) (result i64)))"#;
        let interface4_src =
            r#"(interface (func (import "env" "times_two") (param i64 i64) (result i64)))"#;
        let interface5_src = r#"(interface (func (export "empty_bank_account") (param) (result)))"#;
        let interface6_src =
            r#"(interface (func (export "empty_bank_account") (param) (result i64)))"#;

        let interface1 = parser::parse_interface(interface1_src).unwrap();
        let interface2 = parser::parse_interface(interface2_src).unwrap();
        let interface3 = parser::parse_interface(interface3_src).unwrap();
        let interface4 = parser::parse_interface(interface4_src).unwrap();
        let interface5 = parser::parse_interface(interface5_src).unwrap();
        let interface6 = parser::parse_interface(interface6_src).unwrap();

        assert!(interface1.merge(interface2.clone()).is_err());
        assert!(interface2.merge(interface1.clone()).is_err());
        assert!(interface1.merge(interface3.clone()).is_ok());
        assert!(interface2.merge(interface3.clone()).is_ok());
        assert!(interface3.merge(interface2).is_ok());
        assert!(
            interface1.merge(interface1.clone()).is_ok(),
            "exact matches are accepted"
        );
        assert!(interface3.merge(interface4).is_err());
        assert!(interface5.merge(interface5.clone()).is_ok());
        assert!(interface5.merge(interface6).is_err());
    }
}
