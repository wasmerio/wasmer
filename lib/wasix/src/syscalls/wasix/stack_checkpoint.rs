use super::*;
use crate::syscalls::*;

/// ### `stack_checkpoint()`
/// Creates a snapshot of the current stack which allows it to be restored
/// later using its stack hash.
#[instrument(level = "trace", skip_all, ret)]
pub fn stack_checkpoint<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    ret_val: WasmPtr<Longsize, M>,
) -> Result<Errno, WasiError> {
    // If we were just restored then we need to return the value instead
    if let Some(val) = unsafe { handle_rewind::<M, Longsize>(&mut ctx) } {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        wasi_try_mem_ok!(ret_val.write(&memory, val));
        trace!("restored - (ret={})", val);
        return Ok(Errno::Success);
    }
    trace!("capturing",);

    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // Set the return value that we will give back to
    // indicate we are a normal function call that has not yet
    // been restored
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(ret_val.write(&memory, 0));

    // Pass some offsets to the unwind function
    let ret_offset = ret_val.offset();
    let snapshot_offset = snapshot_ptr.offset();
    let secret = env.state().secret;

    // We clear the target memory location before we grab the stack so that
    // it correctly hashes
    if let Err(err) = snapshot_ptr.write(&memory, StackSnapshot { hash: 0, user: 0 }) {
        warn!(
            %err
        );
    }

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
        // Grab all the globals and serialize them
        let store_data = crate::utils::store::capture_store_snapshot(&mut ctx.as_store_mut())
            .serialize()
            .unwrap();
        let env = ctx.data();
        let store_data = Bytes::from(store_data);

        // We compute the hash again for two reasons... integrity so if there
        // is a long jump that goes to the wrong place it will fail gracefully.
        // and security so that the stack can not be used to attempt to break
        // out of the sandbox
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&secret[..]);
            hasher.update(&memory_stack[..]);
            hasher.update(&rewind_stack[..]);
            hasher.update(&store_data[..]);
            let hash: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
            u128::from_le_bytes(hash)
        };

        // Build a stack snapshot
        let snapshot = StackSnapshot {
            hash,
            user: ret_offset.into(),
        };

        // Get a reference directly to the bytes of snapshot
        let val_bytes = unsafe {
            let p = &snapshot;
            ::std::slice::from_raw_parts(
                (p as *const StackSnapshot) as *const u8,
                ::std::mem::size_of::<StackSnapshot>(),
            )
        };

        // The snapshot may itself reside on the stack (which means we
        // need to update the memory stack rather than write to the memory
        // as otherwise the rewind will wipe out the structure)
        // This correct memory stack is stored as well for validation purposes
        let mut memory_stack_corrected = memory_stack.clone();
        {
            let snapshot_offset: u64 = snapshot_offset.into();
            if snapshot_offset >= env.layout.stack_lower
                && (snapshot_offset + val_bytes.len() as u64) <= env.layout.stack_upper
            {
                // Make sure its within the "active" part of the memory stack
                // (note - the area being written to might not go past the memory pointer)
                let offset = env.layout.stack_upper - snapshot_offset;
                if (offset as usize) < memory_stack_corrected.len() {
                    let left = memory_stack_corrected.len() - (offset as usize);
                    let end = offset + (val_bytes.len().min(left) as u64);
                    if end as usize <= memory_stack_corrected.len() {
                        let pstart = memory_stack_corrected.len() - offset as usize;
                        let pend = pstart + val_bytes.len();
                        let pbytes = &mut memory_stack_corrected[pstart..pend];
                        pbytes.clone_from_slice(val_bytes);
                    }
                }
            }
        }

        /// Add a snapshot to the stack
        ctx.data().thread.add_snapshot(
            &memory_stack[..],
            &memory_stack_corrected[..],
            hash,
            &rewind_stack[..],
            &store_data[..],
        );
        trace!(hash = snapshot.hash, user = snapshot.user);

        // Save the stack snapshot
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        let snapshot_ptr: WasmPtr<StackSnapshot, M> = WasmPtr::new(snapshot_offset);
        if let Err(err) = snapshot_ptr.write(&memory, snapshot) {
            warn!("could not save stack snapshot - {}", err);
            return OnCalledAction::Trap(Box::new(WasiError::Exit(mem_error_to_wasi(err).into())));
        }

        // Rewind the stack and carry on
        let pid = ctx.data().pid();
        let tid = ctx.data().tid();
        match rewind::<M, _>(
            ctx,
            memory_stack_corrected.freeze(),
            rewind_stack.freeze(),
            store_data,
            0 as Longsize,
        ) {
            Errno::Success => OnCalledAction::InvokeAgain,
            err => {
                warn!(
                    "failed checkpoint - could not rewind the stack - errno={}",
                    err
                );
                OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
            }
        }
    })
}
