/// Sha256 hash, represented as bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Hash(pub [u8; 32]);

impl Sha256Hash {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl std::str::FromStr for Sha256Hash {
    type Err = Sha256HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(Sha256HashParseError {
                value: s.to_string(),
                message: "invalid hash length - hash must have 64 hex-encoded characters "
                    .to_string(),
            });
        }

        let bytes = hex::decode(s).map_err(|e| Sha256HashParseError {
            value: s.to_string(),
            message: e.to_string(),
        })?;

        Ok(Sha256Hash(bytes.try_into().unwrap()))
    }
}

impl std::fmt::Debug for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sha256({})", hex::encode(self.0))
    }
}

impl std::fmt::Display for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl schemars::JsonSchema for Sha256Hash {
    fn schema_name() -> String {
        "Sha256Hash".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[derive(Clone, Debug)]
pub struct Sha256HashParseError {
    value: String,
    message: String,
}

impl std::fmt::Display for Sha256HashParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not parse value as sha256 hash: {} (value: '{}')",
            self.message, self.value
        )
    }
}

impl std::error::Error for Sha256HashParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_sha256_parse_roundtrip() {
        let input = "c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c";
        let h1 = input
            .parse::<Sha256Hash>()
            .expect("string should parse to hash");

        assert_eq!(
            h1.0,
            [
                195, 85, 205, 83, 121, 91, 155, 72, 31, 126, 178, 181, 244, 246, 200, 207, 115, 99,
                27, 220, 52, 55, 35, 165, 121, 214, 113, 227, 45, 183, 11, 60
            ],
        );

        assert_eq!(h1.to_string(), input);
    }

    #[test]
    fn hash_sha256_parse_fails() {
        let res1 =
            "c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3".parse::<Sha256Hash>();
        assert!(res1.is_err());

        let res2 = "".parse::<Sha256Hash>();
        assert!(res2.is_err());

        let res3 = "öööööööööööööööööööööööööööööööööööööööööööööööööööööööööööööööö"
            .parse::<Sha256Hash>();
        assert!(res3.is_err());
    }
}
