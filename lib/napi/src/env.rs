use std::collections::HashMap;

use wasmer::{Memory, Table, TypedFunction};

use crate::snapi::SnapiEnv;

pub(crate) struct HostBufferCopy {
    pub(crate) handle_id: u32,
    pub(crate) backing_store_token: u64,
    pub(crate) guest_ptr: u32,
    pub(crate) byte_len: usize,
}

pub(crate) struct GuestBackingStoreMapping {
    pub(crate) host_addr: u64,
    pub(crate) guest_ptr: u32,
    pub(crate) byte_len: usize,
}

#[derive(Default)]
pub(crate) struct RuntimeEnv {
    pub(crate) memory: Option<Memory>,
    pub(crate) malloc_fn: Option<TypedFunction<i32, i32>>,
    pub(crate) table: Option<Table>,
    /// Maps value handle IDs to their guest-memory data pointers.
    /// Used for buffers/arraybuffers backed by guest linear memory.
    pub(crate) guest_data_ptrs: HashMap<u32, u32>,
    /// Maps stable host backing-store tokens to guest-memory data pointers.
    /// This keeps external Buffer/ArrayBuffer aliases stable even when V8/N-API
    /// surfaces the same backing store through a different value handle.
    pub(crate) guest_data_backing_stores: HashMap<u64, GuestBackingStoreMapping>,
    /// Host-owned buffer/arraybuffer mappings copied into guest memory for the
    /// duration of an active callback. These are written back on callback exit.
    pub(crate) host_buffer_copies: Vec<HostBufferCopy>,
    pub(crate) host_buffer_copy_frames: Vec<usize>,
    /// Host-owned buffer copies created while servicing a single guest-side
    /// native binding invocation (typically bracketed by napi_get_cb_info and a
    /// return-value creation call).
    pub(crate) host_buffer_method_frames: Vec<usize>,
    pub(crate) next_napi_env_id: u32,
    pub(crate) next_napi_scope_id: u32,
    pub(crate) napi_envs: HashMap<u32, usize>,
    pub(crate) napi_state_to_guest_env: HashMap<usize, u32>,
    pub(crate) napi_scopes: HashMap<u32, u32>,
}

impl RuntimeEnv {
    pub(crate) fn register_napi_env(&mut self, env: SnapiEnv) -> (u32, u32) {
        let env_id = self.next_napi_env_id.max(1);
        self.next_napi_env_id = env_id.saturating_add(1);

        let scope_id = self.next_napi_scope_id.max(1);
        self.next_napi_scope_id = scope_id.saturating_add(1);

        self.napi_envs.insert(env_id, env as usize);
        self.napi_state_to_guest_env.insert(env as usize, env_id);
        self.napi_scopes.insert(scope_id, env_id);
        (env_id, scope_id)
    }

    pub(crate) fn unregister_napi_scope(&mut self, scope_id: u32) -> Option<SnapiEnv> {
        let env_id = self.napi_scopes.remove(&scope_id)?;
        let env = self.napi_envs.remove(&env_id)?;
        self.napi_state_to_guest_env.remove(&env);
        Some(env as SnapiEnv)
    }

    pub(crate) fn resolve_napi_env(&self, guest_env: i32) -> SnapiEnv {
        let env_id = if guest_env > 0 {
            guest_env as u32
        } else {
            return std::ptr::null_mut();
        };
        self.napi_envs
            .get(&env_id)
            .map(|env| *env as SnapiEnv)
            .unwrap_or(std::ptr::null_mut())
    }
}
