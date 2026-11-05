use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    sync::{
        Arc, Weak,
        atomic::{AtomicU32, Ordering},
    },
};

static NEXT_ERROR: AtomicU32 = AtomicU32::new(1);
thread_local! {
    static LOCAL_ERRORS: RefCell<HashMap<u32, (Weak<JsErrorOwner>, JsValue)>> = RefCell::default();
}

#[derive(Debug)]
struct JsErrorOwner(u32);
impl Drop for JsErrorOwner {
    fn drop(&mut self) {
        // A foreign final drop cannot touch the originating worker's JS heap. Its weak
        // entry is collected by that worker's next error conversion instead.
        let _ = LOCAL_ERRORS.try_with(|errors| {
            errors.borrow_mut().remove(&self.0);
        });
    }
}

#[derive(Debug)]
struct ThrownValue {
    owner: Arc<JsErrorOwner>,
    message: JsTrap,
}
impl ThrownValue {
    fn new(value: JsValue) -> Self {
        let message = JsTrap::from(value.clone());
        let id = NEXT_ERROR
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("JavaScript error IDs exhausted");
        let owner = Arc::new(JsErrorOwner(id));
        LOCAL_ERRORS.with_borrow_mut(|errors| {
            errors.retain(|_, (owner, _)| owner.strong_count() != 0);
            errors.insert(id, (Arc::downgrade(&owner), value));
        });
        Self { owner, message }
    }
    fn local_value(&self) -> Option<JsValue> {
        LOCAL_ERRORS.with_borrow(|errors| errors.get(&self.owner.0).map(|(_, value)| value.clone()))
    }
}

use js_sys::Reflect;
use wasm_bindgen::{JsValue, prelude::*};

use crate::{
    RuntimeError,
    js::{exception::Exception, vm::VMExceptionRef},
};

// A freshly raised engine trap carries an internal brand which Wasm catch_all
// cannot catch. Constructing WebAssembly.RuntimeError in JavaScript does not.
// The symbol owns the Rust payload on that exact error object; there is no
// registry or reverse edge keeping the JavaScript error alive.
#[wasm_bindgen(inline_js = r#"
const hostAbortPayload = Symbol('wasmer.host-abort');
const defineHostAbortPayload = Object.defineProperty;
export function wasmerHostAbort(trap, payload) {
    let error;
    try { trap(); } catch (raised) { error = raised; }
    defineHostAbortPayload(error, hostAbortPayload, {value: payload});
    return error;
}
export function wasmerHostAbortPayload(value) {
    if ((typeof value !== 'object' || value === null) && typeof value !== 'function') return undefined;
    const payload = value[hostAbortPayload];
    if (payload === undefined) return undefined;
    try { return payload.__wasmer_clone_error(); } catch (_) { return null; }
}
export function wasmerDescribeError(value) {
    try {
        if (value instanceof Error) {
            const message = String(value.message);
            const stack = Reflect.get(value, "stack");
            return typeof stack === "string" && stack.length && stack !== message
                ? `${message}\n${stack}` : message;
        }
        if (typeof value === "string") return value;
        if (value !== null && (typeof value === "object" || typeof value === "function")) {
            return String(value.toString());
        }
    } catch (_) {}
    return undefined;
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = wasmerHostAbort)]
    fn host_abort_value(trap: &js_sys::Function, payload: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = wasmerHostAbortPayload)]
    fn host_abort_payload(value: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = wasmerDescribeError)]
    fn describe_error(value: &JsValue) -> JsValue;
}

/// An actual runtime instruction supplies the engine's uncatchable trap brand.
/// Calling its raw export avoids compiling another module or retaining a store.
#[wasm_bindgen(skip_typescript)]
pub fn __wasmer_host_abort_trap() {
    abort_instruction();
}

fn abort_instruction() -> ! {
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    panic!("JavaScript host abort requires a WebAssembly runtime");
}

/// Owned by one JavaScript error, with wasm-bindgen's ordinary finalizer custody.
#[wasm_bindgen(skip_typescript)]
pub struct HostAbortPayload {
    error: RuntimeError,
}

#[wasm_bindgen]
impl HostAbortPayload {
    /// Each conversion gets its own wrapper around the same Rust error Arc.
    /// Diagnostics and repeated rethrows never consume the error's payload.
    pub fn __wasmer_clone_error(&self) -> JsValue {
        Trap::user(Box::new(self.error.clone())).into()
    }
}

fn host_abort_to_js(error: RuntimeError) -> JsValue {
    let exports = wasm_bindgen::exports();
    let trap = Reflect::get(&exports, &JsValue::from_str("__wasmer_host_abort_trap"))
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
        .unwrap_or_else(|| {
            // Broken runtime glue must never resume the canceled guest.
            abort_instruction()
        });
    host_abort_value(&trap, HostAbortPayload { error }.into()).unwrap_or_else(|_| {
        // Fresh built-in trap objects are extensible. Fail closed if the host
        // cannot attach custody; a catchable replacement would resume the guest.
        abort_instruction()
    })
}

#[derive(Debug)]
enum InnerTrap {
    User(Box<dyn Error + Send + Sync>),
    Js(ThrownValue),
}

/// A struct representing a Trap
#[wasm_bindgen(skip_typescript)]
#[derive(Debug)]
pub struct Trap {
    inner: InnerTrap,
}

impl Trap {
    pub fn user(error: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            inner: InnerTrap::User(error),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            _ => Err(self),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match &self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => err.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Returns true if the `Trap` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match &self.inner {
            InnerTrap::User(err) => err.is::<T>(),
            _ => false,
        }
    }

    /// Returns true if the `Trap` is an exception
    pub fn is_exception(&self) -> bool {
        false
    }

    /// If the `Trap` is an uncaught exception, returns it.
    pub fn to_exception_ref(&self) -> Option<VMExceptionRef> {
        None
    }
}

#[wasm_bindgen]
impl Trap {
    /// A marker method to indicate that an object is an instance of the `Trap`
    /// class.
    pub fn __wbg_wasmer_trap() {}
}

impl std::error::Error for Trap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner {
            InnerTrap::User(err) => Some(&**err),
            _ => None,
        }
    }
}

impl std::fmt::Display for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            InnerTrap::User(e) => write!(f, "user: {e}"),
            InnerTrap::Js(value) => write!(f, "js: {}", value.message),
        }
    }
}

impl From<JsValue> for RuntimeError {
    fn from(value: JsValue) -> Self {
        if let Ok(payload) = host_abort_payload(&value)
            && !payload.is_undefined()
        {
            return match downcast_from_ptr(&payload) {
                Some(trap) => trap.into(),
                // A recognized abort with broken payload glue must not turn into
                // a catchable replacement or be mistaken for normal completion.
                None => abort_instruction(),
            };
        }
        // We try to downcast the error and see if it's an instance of Trap
        // instead, so we don't need to re-wrap it.
        if let Some(trap) = downcast_from_ptr(&value) {
            return trap.into();
        }

        Self::from(Trap {
            inner: InnerTrap::Js(ThrownValue::new(value)),
        })
    }
}

/// This whole mechanism works because the JavaScript wrapper class has a static
/// `__wbg_wasmer_trap()` method which marks that it is a [`Trap`].
///
/// If that method exists, we assume the pointer is valid and safe to cast back
/// to our type.
fn downcast_from_ptr(value: &JsValue) -> Option<Trap> {
    if !value.is_object() {
        return None;
    }

    let prototype = &Reflect::get_prototype_of(value).ok()?;
    let class = Reflect::get(prototype, &JsValue::from_str("constructor")).ok()?;
    let key = JsValue::from_str("__wbg_wasmer_trap");

    if !Reflect::get(&class, &key).ok()?.is_function() {
        return None;
    }

    // Safety: The marker function exists, therefore it's safe to convert back
    // to a Trap.
    unsafe {
        // Note: this assumes the wrapper class generated by #[wasm_bindgen] will
        // always have a `__destroy_into_raw()` method which consumes the `Trap`
        // wrapper and returns a pointer.
        //
        // This is valid as of wasm-bindgen version 0.2.87
        let key = JsValue::from_str("__destroy_into_raw");
        let ptr = Reflect::get(value, &key)
            .ok()
            .filter(JsValue::is_function)
            .map(JsCast::unchecked_into::<js_sys::Function>)
            .and_then(|destroy_into_raw| destroy_into_raw.call0(value).ok())
            .and_then(|ret| ret.as_f64())
            .and_then(wasm32_pointer_from_js_number)?;

        Some(<Trap as wasm_bindgen::convert::FromWasmAbi>::from_abi(
            wasm_bindgen::__rt::WasmPtr::from_usize(ptr as usize),
        ))
    }
}

/// JavaScript exposes a WebAssembly `i32` result as a signed number, while
/// newer glue may explicitly coerce pointer results to an unsigned number.
/// Accept both representations and preserve the original wasm32 bit pattern.
fn wasm32_pointer_from_js_number(value: f64) -> Option<u32> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }

    if (0.0..=u32::MAX as f64).contains(&value) {
        Some(value as u32)
    } else if ((i32::MIN as f64)..0.0).contains(&value) {
        Some(value as i32 as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::wasm32_pointer_from_js_number;

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn thrown_values_stay_rooted_only_while_the_error_is_live() {
        use super::{JsValue, LOCAL_ERRORS, RuntimeError};
        let before = LOCAL_ERRORS.with_borrow(|errors| errors.len());
        let error = RuntimeError::from(JsValue::from_str("original thrown value"));
        assert_eq!(LOCAL_ERRORS.with_borrow(|errors| errors.len()), before + 1);
        let rethrown: JsValue = error.clone().into();
        assert_eq!(
            rethrown.as_string().as_deref(),
            Some("original thrown value")
        );
        drop(error);
        assert_eq!(LOCAL_ERRORS.with_borrow(|errors| errors.len()), before);
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn throwing_error_properties_preserve_the_original_rejection() {
        use super::{JsValue, LOCAL_ERRORS, RuntimeError};
        for source in [
            "return { toString() { throw new Error('diagnostic failed'); } };",
            "return Object.create(Object.defineProperty({}, 'constructor', { get() { throw new Error('constructor failed'); } }));",
            "return new Proxy({}, { getPrototypeOf() { throw new Error('prototype failed'); }, get() { throw new Error('property failed'); } });",
            "return Object.defineProperty(new Error(), 'message', { get() { throw new Error('message failed'); } });",
            "return Object.defineProperty(new Error(), 'stack', { get() { throw new Error('stack failed'); } });",
        ] {
            let before = LOCAL_ERRORS.with_borrow(|errors| errors.len());
            let value = js_sys::Function::new_no_args(source)
                .call0(&JsValue::UNDEFINED)
                .unwrap();
            let error = RuntimeError::from(value.clone());
            let rethrown: JsValue = error.clone().into();
            assert!(js_sys::Object::is(&value, &rethrown));
            assert_eq!(LOCAL_ERRORS.with_borrow(|errors| errors.len()), before + 1);
            drop(error);
            assert_eq!(LOCAL_ERRORS.with_borrow(|errors| errors.len()), before);
        }
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn host_abort_diagnostics_clone_the_exact_error_without_consuming_custody() {
        use super::{JsValue, RuntimeError};
        #[derive(Debug, thiserror::Error)]
        #[error("host abort identity")]
        struct Aborted;
        let original = RuntimeError::host_abort(Box::new(Aborted));
        let js: JsValue = original.clone().into();
        let first = RuntimeError::from(js.clone());
        let second = RuntimeError::from(js);
        assert!(std::sync::Arc::ptr_eq(&original.inner, &first.inner));
        assert!(std::sync::Arc::ptr_eq(&original.inner, &second.inner));
        assert!(first.is_host_abort());
        assert!(second.downcast_ref::<Aborted>().is_some());
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn host_abort_of_a_javascript_error_keeps_the_original_value_separate() {
        use super::{InnerTrap, JsValue, RuntimeError};
        let original_value: JsValue = js_sys::Object::new().into();
        let original = RuntimeError::from(original_value.clone());
        let abort = RuntimeError::host_abort(Box::new(original));
        let first: JsValue = abort.clone().into();
        let second: JsValue = RuntimeError::from(first.clone()).into();
        assert!(!js_sys::Object::is(&first, &original_value));
        assert!(!js_sys::Object::is(&first, &second));
        let restored = RuntimeError::from(second);
        assert!(std::sync::Arc::ptr_eq(&abort.inner, &restored.inner));
        #[allow(clippy::infallible_destructuring_match)]
        // The fallback applies with other backends enabled.
        let trap = match &restored.inner.source {
            crate::BackendTrap::Js(trap) => trap,
            #[cfg(any(feature = "sys", feature = "v8"))]
            _ => panic!("expected the original JavaScript error"),
        };
        let InnerTrap::Js(thrown) = &trap.inner else {
            panic!("expected the original thrown value");
        };
        assert!(js_sys::Object::is(
            &thrown.local_value().unwrap(),
            &original_value
        ));
    }

    #[test]
    fn converts_signed_and_unsigned_wasm32_pointers() {
        assert_eq!(wasm32_pointer_from_js_number(42.0), Some(42));
        assert_eq!(
            wasm32_pointer_from_js_number(i32::MIN as f64),
            Some(0x8000_0000),
        );
        assert_eq!(wasm32_pointer_from_js_number(-1.0), Some(u32::MAX),);
        assert_eq!(
            wasm32_pointer_from_js_number(u32::MAX as f64),
            Some(u32::MAX),
        );
    }

    #[test]
    fn rejects_numbers_that_cannot_be_wasm32_pointers() {
        assert_eq!(wasm32_pointer_from_js_number(f64::NAN), None);
        assert_eq!(wasm32_pointer_from_js_number(1.5), None);
        assert_eq!(wasm32_pointer_from_js_number(i32::MIN as f64 - 1.0), None);
        assert_eq!(wasm32_pointer_from_js_number(u32::MAX as f64 + 1.0), None);
    }
}

/// A `Send+Sync` version of a JavaScript error.
#[derive(Debug)]
enum JsTrap {
    /// An error message.
    Message(String),
    /// Unable to determine the underlying error.
    Unknown,
}

impl From<JsValue> for JsTrap {
    fn from(value: JsValue) -> Self {
        describe_error(&value)
            .as_string()
            .map_or(Self::Unknown, Self::Message)
    }
}

impl std::fmt::Display for JsTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(m) => write!(f, "{m}"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl From<JsTrap> for Trap {
    fn from(value: JsTrap) -> Self {
        Self {
            inner: InnerTrap::Js(ThrownValue::new(JsValue::from_str(&value.to_string()))),
        }
    }
}

impl From<JsTrap> for RuntimeError {
    fn from(value: JsTrap) -> Self {
        Into::<Trap>::into(value).into()
    }
}

impl From<Trap> for RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<Self>() {
            return trap.downcast::<Self>().unwrap();
        }

        Self::new_from_source(crate::BackendTrap::Js(trap), vec![], None)
    }
}

impl From<RuntimeError> for wasm_bindgen::JsValue {
    fn from(value: RuntimeError) -> Self {
        if value.is_host_abort() {
            return host_abort_to_js(value);
        }
        if let crate::BackendTrap::Js(trap) = &value.inner.source
            && let InnerTrap::Js(thrown) = &trap.inner
            && let Some(original) = thrown.local_value()
        {
            return original;
        }
        // Keep exec's typed Exit/Restore errors intact across JS host frames.
        Trap::user(Box::new(value)).into()
    }
}

pub(crate) fn raise(error: Box<dyn std::error::Error + Send + Sync>) -> ! {
    if let Some(runtime_error) = error.downcast_ref::<RuntimeError>()
        && runtime_error.is_host_abort()
    {
        let runtime_error = *error.downcast::<RuntimeError>().unwrap();
        wasm_bindgen::throw_val(host_abort_to_js(runtime_error));
    }
    let error = Trap::user(error);
    let js_error: JsValue = error.into();
    wasm_bindgen::throw_val(js_error)
}
