use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering},
};

use js_sys::Symbol;
use wasm_bindgen::JsValue;

use self::integrity_check::IntegrityCheck;

/// A handle that lets you detect thread-safety issues when passing a
/// [`JsValue`] (or derived type) around.
#[derive(Debug, Clone)]
pub(crate) struct JsHandle<T> {
    value: T,
    integrity: IntegrityCheck,
}

impl<T> JsHandle<T> {
    #[track_caller]
    pub fn new(value: T) -> Self {
        Self {
            value,
            integrity: IntegrityCheck::new(std::any::type_name::<T>()),
        }
    }

    #[track_caller]
    pub fn into_inner(self) -> T {
        self.integrity.check();
        self.value
    }
}

impl<T: PartialEq> PartialEq for JsHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            value,
            integrity: _,
        } = self;

        *value == other.value
    }
}

impl<T: Eq> Eq for JsHandle<T> {}

impl<T> From<T> for JsHandle<T> {
    #[track_caller]
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Into<Self>> From<JsHandle<T>> for JsValue {
    fn from(value: JsHandle<T>) -> Self {
        value.into_inner().into()
    }
}

impl<A, T> AsRef<A> for JsHandle<T>
where
    T: AsRef<A>,
{
    #[track_caller]
    fn as_ref(&self) -> &A {
        self.integrity.check();
        self.value.as_ref()
    }
}

impl<T> Deref for JsHandle<T> {
    type Target = T;

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        self.integrity.check();
        &self.value
    }
}

impl<T> DerefMut for JsHandle<T> {
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.integrity.check();
        &mut self.value
    }
}

#[cfg(not(debug_assertions))]
mod integrity_check {

    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct IntegrityCheck;

    impl IntegrityCheck {
        #[track_caller]
        pub(crate) fn new(_type_name: &'static str) -> Self {
            IntegrityCheck
        }

        pub(crate) fn check(&self) {}
    }
}

#[cfg(debug_assertions)]
mod integrity_check {
    use std::{fmt::Write as _, panic::Location, sync::Arc};

    use js_sys::JsString;
    use wasm_bindgen::JsCast;

    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct IntegrityCheck {
        original_thread: u32,
        created: &'static Location<'static>,
        type_name: &'static str,
        // Export snapshots clone handles; keep their immutable diagnostic shared.
        backtrace: Option<Arc<String>>,
    }

    impl IntegrityCheck {
        #[track_caller]
        pub(crate) fn new(type_name: &'static str) -> Self {
            Self {
                original_thread: super::current_thread_id(),
                created: Location::caller(),
                type_name,
                backtrace: record_backtrace(),
            }
        }

        #[track_caller]
        pub(crate) fn check(&self) {
            let current_thread = super::current_thread_id();

            if current_thread != self.original_thread {
                let Self {
                    original_thread,
                    created,
                    type_name,
                    backtrace,
                } = self;
                let mut error_message = String::new();

                writeln!(
                    error_message,
                    "Thread-safety integrity check for {type_name} failed."
                )
                .unwrap();

                writeln!(
                    error_message,
                    "Created at {created} on thread #{original_thread}"
                )
                .unwrap();

                if let Some(bt) = backtrace {
                    writeln!(error_message, "{bt}").unwrap();
                    writeln!(error_message).unwrap();
                }

                let caller = Location::caller();

                writeln!(
                    error_message,
                    "Accessed from {caller} on thread #{current_thread}"
                )
                .unwrap();

                if let Some(bt) = record_backtrace() {
                    writeln!(error_message, "{bt}").unwrap();
                    writeln!(error_message).unwrap();
                }

                panic!("{error_message}");
            }
        }
    }

    fn record_backtrace() -> Option<Arc<String>> {
        let err = js_sys::Error::new("");
        let stack = JsString::from(wasm_bindgen::intern("stack"));

        let stack = js_sys::Reflect::get(&err, &stack)
            .ok()?
            .dyn_into::<JsString>()
            .ok()?;
        copy_backtrace(&stack)
    }

    fn copy_backtrace(stack: &JsString) -> Option<Arc<String>> {
        // Firefox can include the entire very large Wasm/Rust stack. Bound this
        // optional diagnostic in JS before copying it into the fixed Wasm heap.
        const MAX_STACK_UNITS: u32 = 8192;
        let mut trace = stack.slice(0, MAX_STACK_UNITS).as_string()?;
        if stack.length() > MAX_STACK_UNITS {
            const MARKER: &str = "\n[stack truncated]";
            // Reserve the marker inside the copied text: appending past its
            // capacity otherwise doubles every export's diagnostic allocation.
            let mut end = trace.len().saturating_sub(MARKER.len());
            while !trace.is_char_boundary(end) {
                end -= 1;
            }
            trace.truncate(end);
            trace.push_str(MARKER);
        }
        Some(Arc::new(trace))
    }

    #[cfg(all(test, target_arch = "wasm32"))]
    mod tests {
        use super::IntegrityCheck;

        #[wasm_bindgen_test::wasm_bindgen_test]
        fn truncation_marker_keeps_export_diagnostics_within_the_bounded_copy() {
            for stack in [
                js_sys::JsString::from("frame\n".repeat(20_000)),
                js_sys::JsString::from("é🦀".repeat(20_000)),
            ] {
                let bounded = stack.slice(0, 8192).as_string().unwrap();
                let diagnostic = super::copy_backtrace(&stack).unwrap();
                assert!(diagnostic.capacity() <= bounded.capacity(),
                    "a marker must not double the allocation for every export");
                assert!(diagnostic.len() <= bounded.len());
                assert!(diagnostic.ends_with("\n[stack truncated]"));
            }
        }

        #[wasm_bindgen_test::wasm_bindgen_test]
        fn many_large_export_diagnostics_fit_the_runtime_heap() {
            let stack = js_sys::JsString::from("frame\n".repeat(20_000));
            let diagnostics = (0..5_000)
                .map(|_| super::copy_backtrace(&stack).unwrap())
                .collect::<Vec<_>>();
            assert!(diagnostics.first().unwrap().starts_with("frame\n"));
            assert!(diagnostics.last().unwrap().starts_with("frame\n"));
        }

        #[wasm_bindgen_test::wasm_bindgen_test]
        fn many_export_handle_clones_preserve_debug_affinity_within_the_runtime_heap() {
            // Large PIC mains expose thousands of functions; snapshots/ABI handles
            // clone these diagnostics repeatedly under the same fixed runtime heap.
            let original = IntegrityCheck::new("export handle");
            let copies = vec![original.clone(); 250_000];
            copies.first().unwrap().check();
            copies.last().unwrap().check();
            assert_eq!(copies.first(), Some(&original));
            assert_eq!(copies.last(), Some(&original));
        }
    }
}

/// A browser polyfill for [`std::thread::ThreadId`] [`std::thread::current()`].
///
/// This works by creating a `$WASMER_THREAD_ID` symbol and setting it on
/// the global object. As long as they use the same `SharedArrayBuffer` for
/// their linear memory, each thread (i.e. web worker or the UI thread) is
/// guaranteed to get a unique ID.
///
/// This is mainly intended for use in `wasmer-wasix` and `wasmer-js`, and may
/// go away in the future.
#[doc(hidden)]
pub fn current_thread_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(0);

    let global = js_sys::global();
    let thread_id_symbol = Symbol::for_("$WASMER_THREAD_ID");

    if let Some(v) = js_sys::Reflect::get(&global, &thread_id_symbol)
        .ok()
        .and_then(|v| v.as_f64())
    {
        // Note: we use a symbol so we know for sure that nobody else created
        // this field.
        return v as u32;
    }

    // Looks like we haven't set the thread ID yet.
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    js_sys::Reflect::set(&global, &thread_id_symbol, &JsValue::from(id))
        .expect("Setting a field on the global object should never fail");

    id
}
