use crate::{hash::Sha256Hash, package::PackageParseError};

/// Hash for a package.
///
/// Currently only supports the format: `sha256:<hash>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageHash(Sha256Hash);

impl PackageHash {
    const STR_PREFIX: &'static str = "sha256:";

    pub fn as_sha256(&self) -> Option<&Sha256Hash> {
        Some(&self.0)
    }

    pub fn from_sha256_bytes(bytes: [u8; 32]) -> Self {
        Self(Sha256Hash(bytes))
    }
}

impl From<Sha256Hash> for PackageHash {
    fn from(value: Sha256Hash) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for PackageHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sha256:{}", self.0)
    }
}

impl std::str::FromStr for PackageHash {
    type Err = PackageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(Self::STR_PREFIX) {
            return Err(PackageParseError::new(
                s,
                "package hashes must start with 'sha256:'",
            ));
        }
        let hash = Sha256Hash::from_str(&s[Self::STR_PREFIX.len()..])
            .map_err(|e| PackageParseError::new(s, e.to_string()))?;

        Ok(PackageHash(hash))
    }
}

impl serde::Serialize for PackageHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for PackageHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>()
            .map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl schemars::JsonSchema for PackageHash {
    fn schema_name() -> String {
        "PackageHash".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_package_hash_roundtrip() {
        let input = "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c";
        let h1 = input
            .parse::<PackageHash>()
            .expect("string should parse to hash");

        assert_eq!(
            h1.as_sha256().unwrap().as_bytes(),
            &[
                195, 85, 205, 83, 121, 91, 155, 72, 31, 126, 178, 181, 244, 246, 200, 207, 115, 99,
                27, 220, 52, 55, 35, 165, 121, 214, 113, 227, 45, 183, 11, 60
            ],
        );

        assert_eq!(h1.to_string(), input);
    }

    #[test]
    fn package_hash_serde_roundtrip() {
        let input = "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c";
        let h1 = input
            .parse::<PackageHash>()
            .expect("string should parse to hash");

        // Test serialization.
        assert_eq!(
            serde_json::to_value(&h1).unwrap(),
            serde_json::Value::String(input.to_owned()),
        );

        // Test deserialize.
        let v = serde_json::to_string(&h1).unwrap();
        let h2 = serde_json::from_str::<PackageHash>(&v).unwrap();

        assert_eq!(h1, h2);
    }
}
