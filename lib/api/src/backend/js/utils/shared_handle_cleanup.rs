//! Coalesced collection requests; object values never go over this channel.
use wasm_bindgen::{JsCast, JsValue, prelude::*};

#[wasm_bindgen(inline_js = r#"
export function createSharedHandleCleanup(name, collect) {
    let channel, timer, queued = false;
    const close = () => {
        channel?.close();
        channel = undefined;
        if (timer !== undefined) clearInterval(timer);
        timer = undefined;
    };
    const sweep = () => { if (!collect()) close(); };
    const listen = () => {
        if (timer !== undefined) return;
        try {
            if (typeof BroadcastChannel === "function") {
                channel = new BroadcastChannel(name);
                channel.onmessage = sweep;
                channel.unref?.();
            }
        } catch (_) {}
        // Backstop for unavailable/lost notifications, including a final drop
        // in a worker without BroadcastChannel. Stop when no local objects remain.
        timer = setInterval(sweep, 1000);
        timer.unref?.();
    };
    return {
        listen,
        notify() {
            if (queued) return;
            queued = true;
            listen();
            queueMicrotask(() => {
                queued = false;
                try { channel?.postMessage(null); } finally { sweep(); }
            });
        }
    };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = createSharedHandleCleanup)]
    fn create_cleanup(name: &str, callback: &js_sys::Function) -> js_sys::Object;
}

struct Cleanup {
    bridge: js_sys::Object,
    _callback: Closure<dyn FnMut() -> bool>,
}
thread_local! {
    static CLEANUP: std::cell::OnceCell<Cleanup> = const { std::cell::OnceCell::new() };
}

fn invoke(namespace: &str, method: &str) {
    let _ = CLEANUP.try_with(|slot| {
        let cleanup = slot.get_or_init(|| {
            let callback = Closure::new(|| {
                super::shared_handle::collect_shared_objects();
                super::shared_handle::has_local_objects()
            });
            let bridge = create_cleanup(namespace, callback.as_ref().unchecked_ref());
            Cleanup {
                bridge,
                _callback: callback,
            }
        });
        if let Ok(function) = js_sys::Reflect::get(&cleanup.bridge, &JsValue::from_str(method))
            && let Some(function) = function.dyn_ref::<js_sys::Function>()
        {
            // A torn-down host must not turn final Rust ownership drop into a panic.
            let _ = function.call0(&cleanup.bridge);
        }
    });
}

pub(super) fn listen(namespace: &str) {
    invoke(namespace, "listen");
}
pub(super) fn notify(namespace: &str) {
    invoke(namespace, "notify");
}
