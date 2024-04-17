use super::PackageHash;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamedPackageId {
    pub full_name: String,
    pub version: semver::Version,
}

impl NamedPackageId {
    pub fn try_new(
        name: impl Into<String>,
        version: impl AsRef<str>,
    ) -> Result<Self, semver::Error> {
        Ok(Self {
            full_name: name.into(),
            version: version.as_ref().parse()?,
        })
    }
}

impl std::fmt::Display for NamedPackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.full_name, self.version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageId {
    Hash(PackageHash),
    Named(NamedPackageId),
}

impl PackageId {
    pub fn new_named(name: impl Into<String>, version: semver::Version) -> Self {
        Self::Named(NamedPackageId {
            full_name: name.into(),
            version,
        })
    }

    pub fn as_named(&self) -> Option<&NamedPackageId> {
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
}

impl From<NamedPackageId> for PackageId {
    fn from(value: NamedPackageId) -> Self {
        Self::Named(value)
    }
}

impl From<PackageHash> for PackageId {
    fn from(value: PackageHash) -> Self {
        Self::Hash(value)
    }
}

// impl std::str::FromStr for PackageId {
//     type Err = PackageParseError;
//
//     fn from_str(s: &str) -> Result<Self, Self::Err> {
//         if let Ok(hash) = PackageHash::from_str(s) {
//             Ok(Self::Hash(hash))
//         } else if let Ok(named) = NamedPackageId::from_str(s) {
//             Ok(Self::Named(named))
//         } else {
//             Err(PackageParseError::new(
//                 s,
//                 "invalid package ident: expected a hash or a valid named package identifier",
//             ))
//         }
//     }
// }

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(n) => n.fmt(f),
            Self::Hash(h) => h.fmt(f),
        }
    }
}

// impl serde::Serialize for PackageId {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::ser::Serializer,
//     {
//         self.to_string().serialize(serializer)
//     }
// }
//
// impl<'de> serde::Deserialize<'de> for PackageId {
//     fn deserialize<D>(deserializer: D) -> Result<PackageId, D::Error>
//     where
//         D: serde::de::Deserializer<'de>,
//     {
//         let s = String::deserialize(deserializer)?;
//         Self::from_str(&s).map_err(serde::de::Error::custom)
//     }
// }

impl schemars::JsonSchema for PackageId {
    fn schema_name() -> String {
        "PackageIdent".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}
