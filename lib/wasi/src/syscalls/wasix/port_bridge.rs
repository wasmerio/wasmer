use super::*;
use crate::syscalls::*;

/// ### `port_bridge()`
/// Securely connects to a particular remote network
///
/// ## Parameters
///
/// * `network` - Fully qualified identifier for the network
/// * `token` - Access token used to authenticate with the network
/// * `security` - Level of encryption to encapsulate the network connection with
pub fn port_bridge<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    network: WasmPtr<u8, M>,
    network_len: M::Offset,
    token: WasmPtr<u8, M>,
    token_len: M::Offset,
    security: Streamsecurity,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::port_bridge",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let network = unsafe { get_input_str_ok!(&memory, network, network_len) };
    let token = unsafe { get_input_str_ok!(&memory, token, token_len) };
    let security = match security {
        Streamsecurity::Unencrypted => StreamSecurity::Unencrypted,
        Streamsecurity::AnyEncryption => StreamSecurity::AnyEncyption,
        Streamsecurity::ClassicEncryption => StreamSecurity::ClassicEncryption,
        Streamsecurity::DoubleEncryption => StreamSecurity::DoubleEncryption,
        _ => return Ok(Errno::Inval),
    };

    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        net.bridge(network.as_str(), token.as_str(), security)
            .await
            .map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
