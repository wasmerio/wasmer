use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};

pub fn serialize<S: Serializer>(v: &Cow<'_, [u8]>, s: S) -> Result<S::Ok, S::Error> {
    #[allow(deprecated)]
    let base64 = base64::encode(v);
    String::serialize(&base64, s)
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Cow<'static, [u8]>, D::Error> {
    let base64 = String::deserialize(d)?;
    #[allow(deprecated)]
    base64::decode(base64.as_bytes())
        .map_err(|e| serde::de::Error::custom(e))
        .map(|d| d.into())
}
