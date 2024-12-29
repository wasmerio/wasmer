use std::str::FromStr;

use super::{NamedPackageIdent, PackageHash, PackageId, PackageParseError};

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum PackageIdent {
    Named(NamedPackageIdent),
    Hash(PackageHash),
}

impl PackageIdent {
    pub fn as_named(&self) -> Option<&NamedPackageIdent> {
        if let Self::Named(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_hash(&self) -> Option<&PackageHash> {
        if let Self::Hash(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns true if this ident matches the given package id.
    ///
    /// Semver constraints are matched against the package id's version.
    pub fn matches_id(&self, id: &PackageId) -> bool {
        match (self, id) {
            (Self::Named(a), PackageId::Named(b)) => a.matches_id(b),
            (Self::Hash(a), PackageId::Hash(b)) => a == b,
            _ => false,
        }
    }
}

impl From<NamedPackageIdent> for PackageIdent {
    fn from(value: NamedPackageIdent) -> Self {
        Self::Named(value)
    }
}

impl From<PackageHash> for PackageIdent {
    fn from(value: PackageHash) -> Self {
        Self::Hash(value)
    }
}

impl std::str::FromStr for PackageIdent {
    type Err = PackageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(hash) = PackageHash::from_str(s) {
            Ok(Self::Hash(hash))
        } else if let Ok(named) = NamedPackageIdent::from_str(s) {
            Ok(Self::Named(named))
        } else {
            Err(PackageParseError::new(
                s,
                "invalid package ident: expected a hash or a valid named package identifier",
            ))
        }
    }
}

impl std::fmt::Display for PackageIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(n) => n.fmt(f),
            Self::Hash(h) => h.fmt(f),
        }
    }
}

impl serde::Serialize for PackageIdent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PackageIdent {
    fn deserialize<D>(deserializer: D) -> Result<PackageIdent, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for PackageIdent {
    fn schema_name() -> String {
        "PackageIdent".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_ident_matches_id() {
        assert!(PackageIdent::from_str("ns/pkg")
            .unwrap()
            .matches_id(&PackageId::new_named("ns/pkg", "1.0.0".parse().unwrap())));

        assert!(PackageIdent::from_str("ns/pkg@2")
            .unwrap()
            .matches_id(&PackageId::new_named("ns/pkg", "2.3.7".parse().unwrap())));

        assert!(!PackageIdent::from_str("ns/pkg@3")
            .unwrap()
            .matches_id(&PackageId::new_named("ns/pkg", "2.3.7".parse().unwrap())));
    }
}
