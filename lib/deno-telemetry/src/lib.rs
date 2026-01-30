use deno_core::Extension;
use serde::{Deserialize, Serialize};

deno_core::extension!(deno_telemetry, esm = ["telemetry.ts", "util.ts"],);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OtelConfig {}

pub use deno_telemetry::{init, lazy_init};
