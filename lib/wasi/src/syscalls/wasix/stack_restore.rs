use super::*;
use crate::syscalls::*;

/// ### `stack_restore()`
/// Restores the current stack to a previous stack described by its
/// stack hash.
///
/// ## Parameters
///
/// * `snapshot_ptr` - Contains a previously made snapshot
#[instrument(level = "trace", skip_all, ret, err)]
pub fn stack_restore<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    mut val: Longsize,
) -> Result<(), WasiError> {
    // Read the snapshot from the stack
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let snapshot = match snapshot_ptr.read(&memory) {
        Ok(a) => {
            trace!("with_ret={}, hash={}, user={}", val, a.hash, a.user);
            a
        }
        Err(err) => {
            warn!("failed to read stack snapshot - {}", err);
            return Err(WasiError::Exit(mem_error_to_wasi(err).into()));
        }
    };

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, _, _| {
        // Let the stack (or fail trying!)
        let env = ctx.data();
        if let Some((mut memory_stack, rewind_stack, store_data)) =
            env.thread.get_snapshot(snapshot.hash)
        {
            let env = ctx.data();
            let memory = env.memory_view(&ctx);

            // If the return value offset is within the memory stack then we need
            // to update it here rather than in the real memory
            let ret_val_offset = snapshot.user;
            if ret_val_offset >= env.stack_start && ret_val_offset < env.stack_end {
                // Make sure its within the "active" part of the memory stack
                let val_bytes = val.to_ne_bytes();
                let offset = env.stack_end - ret_val_offset;
                let end = offset + (val_bytes.len() as u64);
                if end as usize > memory_stack.len() {
                    warn!(
                        "snapshot stack restore failed - the return value is outside of the active part of the memory stack ({} vs {}) - {} - {}",
                        offset,
                        memory_stack.len(),
                        ret_val_offset,
                        end
                    );
                    return OnCalledAction::Trap(Box::new(WasiError::Exit(
                        Errno::Memviolation.into(),
                    )));
                } else {
                    // Update the memory stack with the new return value
                    let pstart = memory_stack.len() - offset as usize;
                    let pend = pstart + val_bytes.len();
                    let pbytes = &mut memory_stack[pstart..pend];
                    pbytes.clone_from_slice(&val_bytes);
                }
            } else {
                let err = snapshot
                    .user
                    .try_into()
                    .map_err(|_| Errno::Overflow)
                    .map(|a| WasmPtr::<Longsize, M>::new(a))
                    .map(|a| {
                        a.write(&memory, val)
                            .map(|_| Errno::Success)
                            .map_err(mem_error_to_wasi)
                            .unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|a| a);
                if err != Errno::Success {
                    warn!(
                        "snapshot stack restore failed - the return value can not be written too - {}",
                        err
                    );
                    return OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())));
                }
            }

            // Rewind the stack - after this point we must immediately return
            // so that the execution can end here and continue elsewhere.
            let pid = ctx.data().pid();
            let tid = ctx.data().tid();
            match rewind::<M>(ctx, memory_stack.freeze(), rewind_stack, store_data) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("failed to rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                }
            }
        } else {
            warn!(
                "snapshot stack restore failed - the snapshot can not be found and hence restored (hash={})",
                snapshot.hash
            );
            OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Unknown.into())))
        }
    });

    // Return so the stack can be unwound (which will then
    // be rewound again but with a different location)
    Ok(())
}
