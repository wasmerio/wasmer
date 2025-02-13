use wasmer_types::{ExternType, FunctionType, Mutability, Type};

/// Utilities to convert between `v8` and `wasmer` values
use crate::{
    v8::{
        bindings::{self, *},
        function,
    },
    BackendFunction, Function, Value,
};

pub trait IntoCApiValue {
    /// Consume [`self`] to produce a [`wasm_val_t`].
    fn into_cv(self) -> wasm_val_t;
}

impl IntoCApiValue for Value {
    fn into_cv(self) -> wasm_val_t {
        match self {
            Value::I32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I32 as _,
                of: wasm_val_t__bindgen_ty_1 { i32_: val },
            },
            Value::I64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I64 as _,
                of: wasm_val_t__bindgen_ty_1 { i64_: val },
            },
            Value::F32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F32 as _,
                of: wasm_val_t__bindgen_ty_1 { f32_: val },
            },
            Value::F64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F64 as _,
                of: wasm_val_t__bindgen_ty_1 { f64_: val },
            },
            Value::FuncRef(Some(val)) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.as_v8().handle) },
                },
            },
            Value::FuncRef(None) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            Value::ExternRef(_) => panic!(
                "Creating host values from guest ExternRefs is not currently supported in V8."
            ),
            Value::ExceptionRef(_) => {
                panic!("Creating host values from guest V128s is not currently supported in V8.")
            }
            Value::V128(_) => {
                panic!("Creating host values from guest V128s is not currently supported in V8.")
            }
        }
    }
}

pub trait IntoWasmerValue {
    /// Consume [`self`] to produce a [`Value`].
    fn into_wv(self) -> Value;
}

impl IntoWasmerValue for wasm_val_t {
    fn into_wv(self) -> Value {
        match self.kind as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { self.of.i32_ }),
            bindings::wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { self.of.i64_ }),
            bindings::wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { self.of.f32_ }),
            bindings::wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { self.of.f64_ }),
            bindings::wasm_valkind_enum_WASM_FUNCREF => Value::FuncRef(Some(Function(
                BackendFunction::V8(crate::backend::v8::function::Function {
                    handle: unsafe { self.of.ref_ as _ },
                }),
            ))),
            bindings::wasm_valkind_enum_WASM_EXTERNREF => {
                panic!("ExternRefs are not currently supported through wasm_c_api")
            }
            _ => unreachable!("v8 kind {} has no matching wasmer type", self.kind),
        }
    }
}

pub trait IntoWasmerType {
    /// Consume [`self`] to produce a [`Type`].
    fn into_wt(self) -> Type;
}

impl IntoWasmerType for wasm_valkind_t {
    fn into_wt(self) -> Type {
        match self as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
            bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
            bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
            bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
            bindings::wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
            bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
            _ => unreachable!("v8 kind {self:?} has no matching wasmer type"),
        }
    }
}

pub trait IntoCApiType {
    /// Consume [`self`] to produce a [`wasm_valkind_t`].
    fn into_ct(self) -> wasm_valkind_t;
}

impl IntoCApiType for Type {
    fn into_ct(self) -> wasm_valkind_t {
        match self as _ {
            Type::I32 => bindings::wasm_valkind_enum_WASM_I32 as _,
            Type::I64 => bindings::wasm_valkind_enum_WASM_I64 as _,
            Type::F32 => bindings::wasm_valkind_enum_WASM_F32 as _,
            Type::F64 => bindings::wasm_valkind_enum_WASM_F64 as _,
            Type::FuncRef => bindings::wasm_valkind_enum_WASM_FUNCREF as _,
            Type::ExternRef => bindings::wasm_valkind_enum_WASM_EXTERNREF as _,
            Type::V128 => panic!("v8 currently does not support V128 types"),
            Type::ExceptionRef => panic!("v8 currently does not support exnrefs"),
        }
    }
}

impl IntoWasmerType for wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(&self as *const _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *const wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *mut wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}

pub trait IntoWasmerExternType {
    /// Consume [`self`] to produce a [`wasm_valkind_t`].
    unsafe fn into_wextt(self) -> Result<ExternType, String>;
}

impl IntoWasmerExternType for wasm_externtype_t {
    unsafe fn into_wextt(mut self) -> Result<ExternType, String> {
        (&self as *const wasm_externtype_t).into_wextt()
    }
}

impl IntoWasmerExternType for *mut wasm_externtype_t {
    unsafe fn into_wextt(self) -> Result<ExternType, String> {
        (self as *const wasm_externtype_t).into_wextt()
    }
}

impl IntoWasmerExternType for *const wasm_externtype_t {
    unsafe fn into_wextt(self) -> Result<ExternType, String> {
        let ret = unsafe {
            let kind = wasm_externtype_kind(self);
            match kind as _ {
                bindings::wasm_externkind_enum_WASM_EXTERN_FUNC => {
                    let functype = wasm_externtype_as_functype_const(self);
                    let params = wasm_functype_params(functype);
                    let params = if params.is_null() || (*params).size == 0 {
                        vec![]
                    } else {
                        std::slice::from_raw_parts((*params).data, (*params).size)
                            .to_vec()
                            .into_iter()
                            .map(|v| v.into_wt())
                            .collect::<Vec<_>>()
                    };

                    let returns = wasm_functype_results(functype);
                    let returns = if returns.is_null() || (*returns).size == 0 {
                        vec![]
                    } else {
                        std::slice::from_raw_parts((*returns).data, (*returns).size)
                            .to_vec()
                            .into_iter()
                            .map(|v| v.into_wt())
                            .collect::<Vec<_>>()
                    };

                    ExternType::Function(FunctionType::new(params, returns))
                }
                bindings::wasm_externkind_enum_WASM_EXTERN_GLOBAL => {
                    let globaltype = wasm_externtype_as_globaltype_const(self);
                    let valtype = wasm_globaltype_content(globaltype);
                    let mutability = if wasm_globaltype_mutability(globaltype)
                        == bindings::wasm_mutability_enum_WASM_CONST as u8
                    {
                        Mutability::Const
                    } else {
                        Mutability::Var
                    };
                    ExternType::Global(wasmer_types::GlobalType {
                        ty: valtype.into_wt(),
                        mutability,
                    })
                }
                bindings::wasm_externkind_enum_WASM_EXTERN_TABLE => {
                    let tabletype = wasm_externtype_as_tabletype_const(self);
                    let valtype = wasm_tabletype_element(tabletype);
                    let limits = *wasm_tabletype_limits(tabletype);
                    ExternType::Table(wasmer_types::TableType {
                        ty: valtype.into_wt(),
                        minimum: limits.min,
                        maximum: if limits.max == 0 || limits.max == u32::MAX {
                            None
                        } else {
                            Some(limits.max)
                        },
                    })
                }
                bindings::wasm_externkind_enum_WASM_EXTERN_MEMORY => {
                    let memorytype = wasm_externtype_as_memorytype_const(self);
                    let limits = *wasm_memorytype_limits(memorytype);
                    ExternType::Memory(wasmer_types::MemoryType {
                        minimum: wasmer_types::Pages(limits.min),
                        maximum: if limits.max == 0 || limits.max == u32::MAX {
                            None
                        } else {
                            Some(wasmer_types::Pages(limits.max))
                        },
                        shared: limits.shared,
                    })
                }
                bindings::wasm_externkind_enum_WASM_EXTERN_TAG => {
                    todo!()
                }
                _ => return Err(String::from("Unsupported extern kind!")),
            }
        };

        Ok(ret)
    }
}
