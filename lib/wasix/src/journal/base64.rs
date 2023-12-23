use std::borrow::Cow;

use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};

use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};

pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
    #[allow(deprecated)]
    let base64 = base64::encode(compress_prepend_size(v));
    String::serialize(&base64, s)
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Cow<'static, [u8]>, D::Error> {
    let base64 = String::deserialize(d)?;
    #[allow(deprecated)]
    base64::decode(decompress_size_prepended(base64.as_bytes()).map_err(serde::de::Error::custom)?)
        .map_err(serde::de::Error::custom)
        .map(|d| d.into())
}
