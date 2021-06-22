#![allow(non_camel_case_types)]

use crate::polling::types::*;
use wasmer_wasi_types::*;

#[link(wasm_import_module = "wasi_experimental_network_ext_unstable")]
extern "C" {
    pub fn poll_create(poll_id: *mut __wasi_poll_t) -> __wasi_errno_t;
    pub fn poll_register(
        poll_id: __wasi_poll_t,
        server: __wasi_fd_t,
        token: __wasi_poll_token_t,
        interest: __wasi_poll_interest_t,
    ) -> __wasi_errno_t;
    /*
    pub fn poll_reregister(
        poll: __wasi_poll_t,
        server: __wasi_fd_t,
        token: __wasi_poll_token_t,
        interest: __wasi_poll_interest_t,
    ) -> __wasi_errno_t;
    pub fn poll_deregister(poll: __wasi_poll_t, server: __wasi_fd_t) -> __wasi_errno_t;
    */
    pub fn poll(
        poll_id: __wasi_poll_t,
        events_id: __wasi_poll_events_t,
        events_size: *mut u32,
    ) -> __wasi_errno_t;

    pub fn events_create(capacity: u32, events_id: *mut __wasi_poll_events_t) -> __wasi_errno_t;
    pub fn event_token(
        events_id: __wasi_poll_events_t,
        event_nth: __wasi_poll_event_t,
        token: *mut __wasi_poll_token_t,
    ) -> __wasi_errno_t;
    /*
    pub fn event_types(
        event: __wasi_poll_event_t,
        types: *mut __wasi_poll_event_type_t,
    ) -> __wasi_errno_t;
    */
}
