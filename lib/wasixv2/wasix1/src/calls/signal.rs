//! Signals.

use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::shim::Env;

/// `callback_signal(name, name_len)`: libc names its handler export at startup.
///
/// Placeholder until signal delivery to v1 guests exists (DESIGN.md, "WASIX v1
/// compatibility"): nothing is registered, so signals take their default actions, as under v1
/// before the first registration.
pub(crate) async fn callback_signal<S: Syscalls + Control + 'static>(
    _env: Env<S>,
    _name: i32,
    _name_len: i32,
) -> Result<(), RuntimeError> {
    Ok(())
}
