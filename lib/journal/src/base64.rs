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
    let bytes = base64::decode(base64).map_err(serde::de::Error::custom)?;
    decompress_size_prepended(&bytes)
        .map_err(serde::de::Error::custom)
        .map(|d| d.into())
}
