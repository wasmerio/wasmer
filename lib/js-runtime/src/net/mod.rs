use std::sync::Arc;

use deno_core::OpState;
use deno_runtime::FeatureChecker;
use deno_runtime::deno_tls::RootCertStoreProvider;
use deno_runtime::deno_web;
use virtual_net::VirtualNetworking;

mod io;
mod ops;
mod ops_tls;
mod stream;

pub(crate) type SharedNet = Arc<dyn VirtualNetworking>;

#[derive(Clone)]
pub(crate) struct NetState {
    net: SharedNet,
}

impl NetState {
    pub(crate) fn new(net: SharedNet) -> Self {
        Self { net }
    }

    pub(crate) fn net(&self) -> SharedNet {
        self.net.clone()
    }
}

pub(crate) fn net_from_state(state: &OpState) -> SharedNet {
    state.borrow::<NetState>().net()
}

pub const UNSTABLE_FEATURE_NAME: &str = "net";

pub(crate) fn check_unstable(state: &OpState, api_name: &str) {
    state
        .borrow::<Arc<FeatureChecker>>()
        .check_or_exit(UNSTABLE_FEATURE_NAME, api_name);
}

#[derive(Clone)]
pub struct DefaultTlsOptions {
    pub root_cert_store_provider: Option<Arc<dyn RootCertStoreProvider>>,
}

impl DefaultTlsOptions {
    pub fn root_cert_store(
        &self,
    ) -> Result<Option<deno_runtime::deno_tls::rustls::RootCertStore>, deno_error::JsErrorBox> {
        Ok(match &self.root_cert_store_provider {
            Some(provider) => Some(provider.get_or_try_init()?.clone()),
            None => None,
        })
    }
}

pub struct UnsafelyIgnoreCertificateErrors(pub Option<Vec<String>>);

deno_core::extension!(deno_net,
    deps = [ deno_web ],
    ops = [
        ops::op_net_accept_tcp,
        ops::op_net_get_ips_from_perm_token,
        ops::op_net_connect_tcp,
        ops::op_net_listen_tcp,
        ops::op_net_listen_udp,
        ops::op_node_unstable_net_listen_udp,
        ops::op_net_recv_udp,
        ops::op_net_send_udp,
        ops::op_net_join_multi_v4_udp,
        ops::op_net_join_multi_v6_udp,
        ops::op_net_leave_multi_v4_udp,
        ops::op_net_leave_multi_v6_udp,
        ops::op_net_set_multi_loopback_udp,
        ops::op_net_set_multi_ttl_udp,
        ops::op_net_set_broadcast_udp,
        ops::op_net_validate_multicast,
        ops::op_dns_resolve,
        ops::op_set_nodelay,
        ops::op_set_keepalive,
        ops::op_net_listen_vsock,
        ops::op_net_accept_vsock,
        ops::op_net_connect_vsock,
        ops::op_net_listen_tunnel,
        ops::op_net_accept_tunnel,
        ops::op_net_accept_unix,
        ops::op_net_connect_unix,
        ops::op_net_listen_unix,
        ops::op_net_listen_unixpacket,
        ops::op_node_unstable_net_listen_unixpacket,
        ops::op_net_recv_unixpacket,
        ops::op_net_send_unixpacket,
        ops::op_net_unix_stream_from_fd,
        ops_tls::op_tls_key_null,
        ops_tls::op_tls_key_static,
        ops_tls::op_tls_cert_resolver_create,
        ops_tls::op_tls_cert_resolver_poll,
        ops_tls::op_tls_cert_resolver_resolve,
        ops_tls::op_tls_cert_resolver_resolve_error,
        ops_tls::op_tls_start,
        ops_tls::op_net_connect_tls,
        ops_tls::op_net_listen_tls,
        ops_tls::op_net_accept_tls,
        ops_tls::op_tls_handshake,
    ],
    esm = [
        "01_net.js" = { source = include_str!("js/01_net.js") },
        "02_tls.js" = { source = include_str!("js/02_tls.js") },
    ],
    lazy_loaded_esm = [
        "03_quic.js" = { source = include_str!("js/03_quic.js") },
    ],
    options = {
        net: SharedNet,
        root_cert_store_provider: Option<Arc<dyn RootCertStoreProvider>>,
        unsafely_ignore_certificate_errors: Option<Vec<String>>,
    },
    state = |state, options| {
        state.put(NetState::new(options.net));
        state.put(DefaultTlsOptions {
            root_cert_store_provider: options.root_cert_store_provider,
        });
        state.put(UnsafelyIgnoreCertificateErrors(
            options.unsafely_ignore_certificate_errors,
        ));
    },
);
