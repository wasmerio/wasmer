//! The WebAssembly adapter a guest imports in place of an async host function.
//!
//! JSPI suspends a guest when it calls a `WebAssembly.Suspending` import. That
//! call is the only thing the engine tells us about: there is no hook when it
//! refuses the suspension, and none when the guest is back on the stack. So
//! the guest does not import a suspending function. It imports this, one
//! module per signature, whose hooks are ordinary imports that run on the
//! guest's own stack, right before and right after the suspension:
//!
//! ```text
//! (func (export "f") (param P..) (result R..) (local $r R..) (local $t i32)
//!   (local.set $r.. (call $start (local.get P..)))        ;; switch out
//!   (block $ready
//!     (loop $again
//!       (br_if $ready (i32.eqz (local.tee $t (global.get $status))))
//!       (global.set $status (i32.const 0))
//!       (if (i32.eq (local.get $t) (i32.const 1)) (then unreachable))
//!       (block $refused (result exnref)
//!         (try_table (catch_all_ref $refused)
//!           (call $wait (local.get $t)))                  ;; the only suspending import
//!         (local.set $r.. (call $resume (local.get $t)))  ;; switch in
//!         (br $again))
//!       (call $unwait (local.get $t))                     ;; not suspended after all
//!       (throw_ref)))
//!   (local.get $r..))
//! ```
//!
//! `start` and `resume` return the import's results, and say what those are
//! worth through `$status`, a global the host writes: [`READY`], [`TRAP`], or
//! the token of the activation that has to wait. The token stays in a local,
//! which the engine saves with the stack, so `resume` is told which activation
//! it resumes instead of having to guess. Results that are not ready are a
//! [`placeholder`], which the guest never sees.
//!
//! The `unreachable` is a genuine trap: no exception handler of the guest
//! catches it, so a cancelled call or a failed host function unwinds the
//! guest without running any more of it.
//!
//! `wait` throws when the engine refuses to suspend the guest, because the
//! guest was entered with a synchronous call or through a JavaScript frame.
//! Some engines run the body of a suspending import before they refuse and
//! some do not, so nothing is concluded from the body: the adapter catches
//! whatever is thrown, has `unwait` undo `start`, and throws on.
//!
//! The handler is a `try_table`, which every engine that has JSPI also has:
//! they all shipped exception references before it.

use std::{cell::RefCell, collections::HashMap};

use js_sys::{Array, Function, Object, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{FunctionType, Type};

/// `$status`: the values `start` or `resume` returned are the import's results.
pub(super) const READY: i32 = 0;
/// `$status`: the guest must not run again.
pub(super) const TRAP: i32 = 1;
/// `$status`: the first value that is the token of an activation.
pub(super) const FIRST_TOKEN: i32 = 2;

// Function indices, in import order.
const START: u8 = 0;
const WAIT: u8 = 1;
const RESUME: u8 = 2;
const UNWAIT: u8 = 3;

const I32: u8 = 0x7f;
const EXNREF: u8 = 0x69;

fn leb(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            return out.push(byte);
        }
        out.push(byte | 0x80);
    }
}

fn section(out: &mut Vec<u8>, id: u8, body: &[u8]) {
    out.push(id);
    leb(out, body.len() as u32);
    out.extend_from_slice(body);
}

fn functype(out: &mut Vec<u8>, params: &[u8], results: &[u8]) {
    out.push(0x60);
    for types in [params, results] {
        leb(out, types.len() as u32);
        out.extend_from_slice(types);
    }
}

fn import(out: &mut Vec<u8>, name: &str, descriptor: &[u8]) {
    out.extend_from_slice(&[1, b'w']);
    leb(out, name.len() as u32);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(descriptor);
}

/// The binary encoding of a type an async host function can take or return.
///
/// JavaScript has no value for a `v128` or an `exnref`: the engine throws
/// when such a value reaches a host function, whatever the function does.
fn valtype(ty: &Type) -> Result<u8, String> {
    match ty {
        Type::I32 => Ok(I32),
        Type::I64 => Ok(0x7e),
        Type::F32 => Ok(0x7d),
        Type::F64 => Ok(0x7c),
        Type::FuncRef => Ok(0x70),
        Type::ExternRef => Ok(0x6f),
        Type::V128 | Type::ExceptionRef => Err(format!(
            "an async host function cannot take or return a `{ty}` on the JavaScript backend: \
             JavaScript has no value of that type"
        )),
    }
}

/// The binary of the adapter module for `ty`.
fn encode(ty: &FunctionType) -> Result<Vec<u8>, String> {
    let params = ty.params().iter().map(valtype).collect::<Result<Vec<_>, _>>()?;
    let results = ty.results().iter().map(valtype).collect::<Result<Vec<_>, _>>()?;
    // The locals: the parameters, then one per result, then the token.
    let first_result = params.len() as u32;
    let token = first_result + results.len() as u32;

    let mut module = b"\0asm\x01\0\0\0".to_vec();

    let mut types = vec![3];
    functype(&mut types, &params, &results);
    functype(&mut types, &[I32], &[]);
    functype(&mut types, &[I32], &results);
    section(&mut module, 1, &types);

    let mut imports = vec![5];
    import(&mut imports, "start", &[0, 0]);
    import(&mut imports, "wait", &[0, 1]);
    import(&mut imports, "resume", &[0, 2]);
    import(&mut imports, "unwait", &[0, 1]);
    import(&mut imports, "status", &[3, I32, 1]);
    section(&mut module, 2, &imports);

    section(&mut module, 3, &[1, 0]);
    section(&mut module, 7, &[1, 1, b'f', 0, 4]);

    let mut body = Vec::new();
    leb(&mut body, results.len() as u32 + 1);
    for ty in results.iter().chain(&[I32]) {
        body.extend_from_slice(&[1, *ty]);
    }
    let local_get = |body: &mut Vec<u8>, index: u32| {
        body.push(0x20);
        leb(body, index);
    };
    // Results come off the stack last one first.
    let set_results = |body: &mut Vec<u8>| {
        for index in (first_result..token).rev() {
            body.push(0x21);
            leb(body, index);
        }
    };

    for index in 0..first_result {
        local_get(&mut body, index);
    }
    body.extend_from_slice(&[0x10, START]);
    set_results(&mut body);
    body.extend_from_slice(&[0x02, 0x40, 0x03, 0x40]); // block $ready, loop $again
    body.extend_from_slice(&[0x23, 0, 0x22]); // global.get $status, local.tee $t
    leb(&mut body, token);
    body.extend_from_slice(&[0x45, 0x0d, 1]); // i32.eqz, br_if $ready
    body.extend_from_slice(&[0x41, READY as u8, 0x24, 0]); // global.set $status (i32.const 0)
    local_get(&mut body, token);
    // if ($t == 1) unreachable
    body.extend_from_slice(&[0x41, TRAP as u8, 0x46, 0x04, 0x40, 0x00, 0x0b]);
    // block $refused (result exnref), try_table (catch_all_ref $refused)
    body.extend_from_slice(&[0x02, EXNREF, 0x1f, 0x40, 1, 3, 0]);
    local_get(&mut body, token);
    body.extend_from_slice(&[0x10, WAIT, 0x0b]); // end try_table
    local_get(&mut body, token);
    body.extend_from_slice(&[0x10, RESUME]);
    set_results(&mut body);
    body.extend_from_slice(&[0x0c, 1, 0x0b]); // br $again, end $refused
    local_get(&mut body, token);
    body.extend_from_slice(&[0x10, UNWAIT, 0x0a, 0x0b, 0x0b]); // throw_ref, end $again, end $ready
    for index in first_result..token {
        local_get(&mut body, index);
    }
    body.push(0x0b);

    let mut code = vec![1];
    leb(&mut code, body.len() as u32);
    code.extend_from_slice(&body);
    section(&mut module, 10, &code);
    Ok(module)
}

/// What `start` and `resume` return while the results are not ready, or will
/// never be: the guest does not see it, but the engine converts it to the
/// result types before the adapter looks at `$status`, and a value it cannot
/// convert would be thrown into the guest as a `TypeError`.
///
/// `null` converts to a zero and to a null reference; an `i64` needs a BigInt.
pub(super) fn placeholder(ty: &FunctionType) -> JsValue {
    let placeholder = |ty: &Type| match ty {
        Type::I64 => JsValue::from(0i64),
        _ => JsValue::NULL,
    };
    match ty.results() {
        [] => JsValue::UNDEFINED,
        [ty] => placeholder(ty),
        types => Array::from_iter(types.iter().map(placeholder)).into(),
    }
}

thread_local! {
    static MODULES: RefCell<HashMap<FunctionType, WebAssembly::Module>> =
        RefCell::new(HashMap::new());
}

/// The adapter module for `ty`, compiled once per signature and thread.
///
/// Fails with a message when the signature has a type no JavaScript function
/// can take or return.
pub(super) fn module(ty: &FunctionType) -> Result<WebAssembly::Module, JsValue> {
    MODULES.with(|modules| {
        if let Some(module) = modules.borrow().get(ty) {
            return Ok(module.clone());
        }
        let bytes = encode(ty).map_err(|message| JsValue::from_str(&message))?;
        let module = WebAssembly::Module::new(&Uint8Array::from(&bytes[..]).into())?;
        modules.borrow_mut().insert(ty.clone(), module.clone());
        Ok(module)
    })
}

/// Instantiates `module` around the hooks of one host function, and returns
/// the function a guest imports.
pub(super) fn instantiate(
    module: &WebAssembly::Module,
    [start, wait, resume, unwait]: [&JsValue; 4],
    status: &JsValue,
) -> Result<Function, JsValue> {
    let hooks = Object::new();
    for (name, value) in [
        ("start", start),
        ("wait", wait),
        ("resume", resume),
        ("unwait", unwait),
        ("status", status),
    ] {
        Reflect::set(&hooks, &name.into(), value)?;
    }
    let imports = Object::new();
    Reflect::set(&imports, &"w".into(), &hooks)?;
    let instance = WebAssembly::Instance::new(module, &imports)?;
    Ok(Reflect::get(&instance.exports(), &"f".into())?.unchecked_into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// The engine accepts the adapter of every shape of signature, also when
    /// the local indices no longer fit in one byte.
    #[wasm_bindgen_test]
    fn every_signature_compiles() {
        let numbers = [Type::I32, Type::I64, Type::F32, Type::F64];
        let references = [Type::ExternRef, Type::FuncRef];
        let many = [Type::I64; 200];
        let signatures: [(&[Type], &[Type]); 7] = [
            (&[], &[]),
            (&numbers, &[]),
            (&[], &numbers),
            (&numbers, &numbers),
            (&references, &references),
            (&many, &[Type::I32]),
            (&many, &many),
        ];
        for (params, results) in signatures {
            let ty = FunctionType::new(params.to_vec(), results.to_vec());
            if let Err(error) = module(&ty) {
                panic!("the adapter for {ty} does not compile: {error:?}");
            }
        }
    }

    #[wasm_bindgen_test]
    fn signatures_javascript_cannot_call_are_refused() {
        for ty in [Type::V128, Type::ExceptionRef] {
            for signature in [
                FunctionType::new(vec![Type::I32, ty], vec![]),
                FunctionType::new(vec![], vec![Type::I32, ty]),
            ] {
                let error = module(&signature).unwrap_err().as_string().unwrap();
                assert!(error.contains(&format!("`{ty}`")), "{error}");
            }
        }
    }
}
