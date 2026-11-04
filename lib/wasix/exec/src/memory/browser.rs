//! C1 synchronous foreign copies. Linux authority: mm/memory.c:__access_remote_vm
//! and mm/process_vm_access.c:process_vm_rw_core. Validation and retention stay
//! on the caller; only the byte operation may run in the independent JS service.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use wasixv2_syscalls::Fault;
use wasm_bindgen::prelude::*;
use wasmer::SharedMemory;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const WORDS: usize = 12;
thread_local! {
    // One slot per runtime worker, in its shared runtime TLS, never in guest memory.
    static SLOT: [AtomicU32; WORDS] = const { [const { AtomicU32::new(0) }; WORDS] };
}

#[wasm_bindgen(inline_js = "
export function registerMemory(id, memory) {
    if (typeof document !== 'undefined' || !globalThis.__wasixMemory)
        throw new Error('foreign memory requires the browser copy-worker bootstrap');
    globalThis.__wasixMemory.register(id, memory);
}
export function unregisterMemory(id) { globalThis.__wasixMemory.unregister(id); }
export function copyMemory(memory, slot) { return globalThis.__wasixMemory.request(memory, slot); }
")]
extern "C" {
    #[wasm_bindgen(js_name = registerMemory)]
    fn register_memory(id: u64, memory: &js_sys::WebAssembly::Memory);
    #[wasm_bindgen(js_name = unregisterMemory)]
    fn unregister_memory(id: u64);
    #[wasm_bindgen(js_name = copyMemory)]
    fn copy_memory(memory: &JsValue, slot: u32) -> bool;
}

pub(super) struct Registration(u64);
impl Registration {
    pub(super) fn new(memory: &SharedMemory) -> Self {
        let id = NEXT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("foreign memory IDs exhausted");
        let local = memory
            .local_js_memory()
            .expect("register shared guest memory on its owning worker");
        // No acknowledgement. The service retains requests from other workers
        // until this registration arrives, even if its owner immediately blocks.
        register_memory(id, &local);
        Self(id)
    }

    fn request(
        &self,
        memory: &SharedMemory,
        op: u32,
        addr: u32,
        buffer: (u32, u32),
        cas: (u32, u32),
    ) -> Result<u64, Fault> {
        SLOT.with(|slot| {
            let values = [
                op,
                self.0 as u32,
                (self.0 >> 32) as u32,
                addr,
                buffer.0,
                buffer.1,
                cas.0,
                cas.1,
            ];
            for (word, value) in slot[1..].iter().zip(values) {
                word.store(value, Ordering::Relaxed);
            }
            let local = memory
                .local_js_memory()
                .map_or(JsValue::UNDEFINED, Into::into);
            // JS publishes the request, waits with a deadline, and acquires the
            // completion before returning. Transport failure never returns here:
            // returning would free/reuse Rust's still-borrowed buffer after timeout.
            if !copy_memory(&local, slot.as_ptr() as usize as u32) {
                return Err(Fault);
            }
            Ok(u64::from(slot[9].load(Ordering::Relaxed))
                | (u64::from(slot[10].load(Ordering::Relaxed)) << 32))
        })
    }

    pub(super) fn read(
        &self,
        memory: &SharedMemory,
        addr: u32,
        bytes: &mut [u8],
    ) -> Result<(), Fault> {
        let len = u32::try_from(bytes.len()).map_err(|_| Fault)?;
        self.request(memory, 0, addr, (bytes.as_mut_ptr() as u32, len), (0, 0))?;
        Ok(())
    }

    pub(super) fn write(
        &self,
        memory: &SharedMemory,
        addr: u32,
        bytes: &[u8],
    ) -> Result<(), Fault> {
        let len = u32::try_from(bytes.len()).map_err(|_| Fault)?;
        self.request(memory, 1, addr, (bytes.as_ptr() as u32, len), (0, 0))?;
        Ok(())
    }

    pub(super) fn size(&self, memory: &SharedMemory) -> u64 {
        self.request(memory, 2, 0, (0, 0), (0, 0))
            .expect("a memory size request cannot fault")
    }

    pub(super) fn compare_exchange(
        &self,
        memory: &SharedMemory,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> Result<u32, Fault> {
        self.request(memory, 3, addr, (0, 4), (expected, new))
            .map(|value| value as u32)
    }

    pub(super) fn zero(&self, memory: &SharedMemory, addr: u32, len: u64) -> Result<(), Fault> {
        let len = u32::try_from(len).map_err(|_| Fault)?;
        self.request(memory, 4, addr, (0, len), (0, 0))?;
        Ok(())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        unregister_memory(self.0);
    }
}
