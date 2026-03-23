// ============================================================
// Guest memory helpers
// ============================================================

use wasmer::FunctionEnvMut;

use crate::{GuestBackingStoreMapping, HostBufferCopy, RuntimeEnv};

pub fn write_guest_bytes(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    guest_ptr: u32,
    data: &[u8],
) -> bool {
    let (state, store) = env.data_and_store_mut();
    let Some(memory) = state.memory.clone() else {
        return false;
    };
    let view = memory.view(&store);
    view.write(guest_ptr as u64, data).is_ok()
}

pub fn write_guest_u32(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: u32) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

pub fn write_guest_i32(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: i32) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

pub fn write_guest_u64(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: u64) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

pub fn write_guest_i64(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: i64) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

pub fn write_guest_f64(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: f64) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

pub fn write_guest_u8(env: &mut FunctionEnvMut<RuntimeEnv>, guest_ptr: u32, val: u8) -> bool {
    write_guest_bytes(env, guest_ptr, &[val])
}

pub fn read_guest_bytes(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    guest_ptr: i32,
    len: usize,
) -> Option<Vec<u8>> {
    if guest_ptr < 0 {
        return None;
    }
    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);
    let mut out = vec![0u8; len];
    view.read(guest_ptr as u64, &mut out).ok()?;
    Some(out)
}

pub fn allocate_guest_bytes(env: &mut FunctionEnvMut<RuntimeEnv>, data: &[u8]) -> Option<u32> {
    let malloc_fn = env.data().malloc_fn.clone()?;
    let len = i32::try_from(data.len()).ok()?;
    let guest_ptr: i32 = {
        let (_, mut store_ref) = env.data_and_store_mut();
        malloc_fn.call(&mut store_ref, len).ok()?
    };
    if guest_ptr <= 0 {
        return None;
    }
    if !write_guest_bytes(env, guest_ptr as u32, data) {
        return None;
    }
    Some(guest_ptr as u32)
}

pub fn host_ptr_to_guest_ptr(env: &mut FunctionEnvMut<RuntimeEnv>, host_addr: u64) -> Option<u32> {
    let memory = env.data().memory.clone()?;
    let (_, store_ref) = env.data_and_store_mut();
    let view = memory.view(&store_ref);
    let host_base = view.data_ptr() as u64;
    let memory_len = view.data_size();
    if host_addr < host_base || host_addr >= host_base + memory_len {
        return None;
    }
    u32::try_from(host_addr - host_base).ok()
}

pub fn resolve_guest_backing_store_mapping(
    mapping: &GuestBackingStoreMapping,
    host_addr: u64,
    byte_len: usize,
) -> Option<u32> {
    let delta = usize::try_from(host_addr.checked_sub(mapping.host_addr)?).ok()?;
    let end = delta.checked_add(byte_len)?;
    if end > mapping.byte_len {
        return None;
    }
    let guest_delta = u32::try_from(delta).ok()?;
    mapping.guest_ptr.checked_add(guest_delta)
}

pub fn resolve_or_copy_host_data_to_guest(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    handle_id: u32,
    backing_store_token: u64,
    host_addr: u64,
    byte_len: usize,
) -> Option<u32> {
    if backing_store_token != 0
        && let Some(mapping) = env
            .data()
            .guest_data_backing_stores
            .get(&backing_store_token)
        && let Some(guest_data_ptr) =
            resolve_guest_backing_store_mapping(mapping, host_addr, byte_len)
    {
        env.data_mut()
            .guest_data_ptrs
            .insert(handle_id, guest_data_ptr);
        return Some(guest_data_ptr);
    }
    if let Some(&guest_data_ptr) = env.data().guest_data_ptrs.get(&handle_id) {
        return Some(guest_data_ptr);
    }
    if host_addr == 0 {
        return Some(0);
    }
    if let Some(guest_ptr) = host_ptr_to_guest_ptr(env, host_addr) {
        return Some(guest_ptr);
    }
    if byte_len == 0 {
        return Some(0);
    }
    let host_slice = unsafe { std::slice::from_raw_parts(host_addr as *const u8, byte_len) };
    let guest_ptr = allocate_guest_bytes(env, host_slice)?;
    if backing_store_token != 0 {
        env.data_mut().guest_data_backing_stores.insert(
            backing_store_token,
            GuestBackingStoreMapping {
                host_addr,
                guest_ptr,
                byte_len,
            },
        );
    }
    env.data_mut().guest_data_ptrs.insert(handle_id, guest_ptr);
    env.data_mut().host_buffer_copies.push(HostBufferCopy {
        handle_id,
        backing_store_token,
        guest_ptr,
        byte_len,
    });
    Some(guest_ptr)
}

pub fn read_guest_u32_array(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    guest_ptr: i32,
    count: usize,
) -> Option<Vec<u32>> {
    let bytes = read_guest_bytes(env, guest_ptr, count * 4)?;
    let mut result = Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(4) {
        result.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(result)
}

pub fn read_guest_c_string(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    guest_ptr: i32,
) -> Option<Vec<u8>> {
    if guest_ptr < 0 {
        return None;
    }
    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);
    let mut out = Vec::new();
    let mut offset = guest_ptr as u64;
    for _ in 0..super::MAX_GUEST_CSTRING_SCAN {
        let mut b = [0u8; 1];
        view.read(offset, &mut b).ok()?;
        if b[0] == 0 {
            return Some(out);
        }
        out.push(b[0]);
        offset += 1;
    }
    None
}
