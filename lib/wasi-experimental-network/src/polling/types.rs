#![allow(non_camel_case_types)]

pub type __wasi_poll_t = u32;

pub type __wasi_poll_interest_t = u32;
pub const READABLE_INTEREST: __wasi_poll_interest_t = 1;
pub const WRITABLE_INTEREST: __wasi_poll_interest_t = 2;
pub const AIO_INTEREST: __wasi_poll_interest_t = 3;
pub const LIO_INTEREST: __wasi_poll_interest_t = 4;

pub type __wasi_poll_events_t = u32;

pub type __wasi_poll_event_t = u32;

pub type __wasi_poll_event_type_t = u32;
pub const READABLE_EVENT: __wasi_poll_event_type_t = 1;
pub const WRITABLE_EVENT: __wasi_poll_event_type_t = 2;
pub const ERROR_EVENT: __wasi_poll_event_type_t = 3;
pub const READ_CLOSED_EVENT: __wasi_poll_event_type_t = 4;
pub const WRITE_CLOSED_EVENT: __wasi_poll_event_type_t = 5;
pub const PRIORITY_EVENT: __wasi_poll_event_type_t = 6;
pub const AIO_EVENT: __wasi_poll_event_type_t = 7;
pub const LIO_EVENT: __wasi_poll_event_type_t = 8;

pub type __wasi_poll_token_t = u32;
