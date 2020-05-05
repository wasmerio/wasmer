use serde::de::{Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;
use wasmer_compiler::CompiledFunctionFrameInfo;

/// This is the unserialized verison of `CompiledFunctionFrameInfo`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct UnprocessedFunctionFrameInfo {
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

impl UnprocessedFunctionFrameInfo {
    /// Converts the `UnprocessedFunctionFrameInfo` to a `CompiledFunctionFrameInfo`
    pub fn deserialize(&self) -> CompiledFunctionFrameInfo {
        // let r = flexbuffers::Reader::get_root(&self.bytes).expect("Can't deserialize the info");
        // CompiledFunctionFrameInfo::deserialize(r).expect("Can't deserialize the info")
        bincode::deserialize(&self.bytes).expect("Can't deserialize the info")
    }

    /// Converts the `CompiledFunctionFrameInfo` to a `UnprocessedFunctionFrameInfo`
    pub fn serialize(processed: &CompiledFunctionFrameInfo) -> Self {
        // let mut s = flexbuffers::FlexbufferSerializer::new();
        // processed
        //     .serialize(&mut s)
        //     .expect("Can't serialize the info");
        // let bytes = s.take_buffer();
        let bytes = bincode::serialize(&processed).expect("Can't serialize the info");
        Self { bytes }
    }
}

/// We hold the frame info in two states, mainly because we want to
/// process it lazily to speed up execution.
///
/// When a Trap occurs, we process the frame info lazily for each
/// function in the frame. That way we minimize as much as we can
/// the upfront effort.
///
/// The data can also be processed upfront. This will happen in the case
/// of compiling at the same time that emiting the JIT.
/// In that case, we don't need to deserialize/process anything
/// as the data is already in memory.
#[derive(Clone)]
pub enum SerializableFunctionFrameInfo {
    /// The unprocessed frame info (binary)
    Unprocessed(UnprocessedFunctionFrameInfo),
    /// The processed frame info (memory struct)
    Processed(CompiledFunctionFrameInfo),
}

impl SerializableFunctionFrameInfo {
    /// Returns true if the extra function info is not yet
    /// processed
    pub fn is_unprocessed(&self) -> bool {
        match self {
            Self::Unprocessed(_) => true,
            _ => false,
        }
    }
}

// Below:
// The custom ser/de for `SerializableFunctionFrameInfo`.

impl Serialize for SerializableFunctionFrameInfo {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let unprocessed = match self {
            Self::Processed(processed) => UnprocessedFunctionFrameInfo::serialize(processed),
            Self::Unprocessed(unprocessed) => unprocessed.clone(),
        };
        s.serialize_bytes(&unprocessed.bytes)
    }
}

struct FunctionFrameInfoVisitor;

impl<'de> Visitor<'de> for FunctionFrameInfoVisitor {
    type Value = UnprocessedFunctionFrameInfo;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes")
    }
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(UnprocessedFunctionFrameInfo { bytes: v })
    }
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> {
        Ok(UnprocessedFunctionFrameInfo { bytes: v.to_vec() })
    }
}

impl<'de> Deserialize<'de> for SerializableFunctionFrameInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(SerializableFunctionFrameInfo::Unprocessed(
            deserializer.deserialize_byte_buf(FunctionFrameInfoVisitor)?,
        ))
    }
}
