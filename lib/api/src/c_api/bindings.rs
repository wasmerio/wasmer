#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

// This matches bindgen::Builder output
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(feature = "wasmi")]
#[allow(unused_imports)]
// This is here to force its linking.
use wasmi_c_api::*;
