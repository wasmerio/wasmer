use std::str::FromStr;

use super::{
    NamedPackageId, NamedPackageIdent, PackageHash, PackageId, PackageIdent, PackageParseError,
};

/// Source location of a package.
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum PackageSource {
    /// An identifier in the format prescribed by [`WebcIdent`].
    Ident(PackageIdent),
    /// An absolute or relative (dot-leading) path.
    Path(String),
    Url(url::Url),
}

impl PackageSource {
    pub fn as_ident(&self) -> Option<&PackageIdent> {
        if let Self::Ident(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_hash(&self) -> Option<&PackageHash> {
        self.as_ident().and_then(|x| x.as_hash())
    }

    pub fn as_named(&self) -> Option<&NamedPackageIdent> {
        self.as_ident().and_then(|x| x.as_named())
    }

    pub fn as_path(&self) -> Option<&String> {
        if let Self::Path(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_url(&self) -> Option<&url::Url> {
        if let Self::Url(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl From<PackageIdent> for PackageSource {
    fn from(id: PackageIdent) -> Self {
        Self::Ident(id)
    }
}

impl From<NamedPackageIdent> for PackageSource {
    fn from(value: NamedPackageIdent) -> Self {
        Self::Ident(PackageIdent::Named(value))
    }
}

impl From<NamedPackageId> for PackageSource {
    fn from(value: NamedPackageId) -> Self {
        Self::Ident(PackageIdent::Named(NamedPackageIdent::from(value)))
    }
}

impl From<PackageHash> for PackageSource {
    fn from(value: PackageHash) -> Self {
        Self::Ident(PackageIdent::Hash(value))
    }
}

impl From<PackageId> for PackageSource {
    fn from(value: PackageId) -> Self {
        match value {
            PackageId::Hash(hash) => Self::from(hash),
            PackageId::Named(named) => Self::Ident(PackageIdent::Named(named.into())),
        }
    }
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ident(id) => id.fmt(f),
            Self::Path(path) => path.fmt(f),
            Self::Url(url) => url.fmt(f),
        }
    }
}

impl std::str::FromStr for PackageSource {
    type Err = PackageParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(first_char) = value.chars().next() else {
            return Err(PackageParseError::new(
                value,
                "An empty string is not a valid package source",
            ));
        };

        if value.contains("://") {
            let url = value
                .parse::<url::Url>()
                .map_err(|e| PackageParseError::new(value, e.to_string()))?;
            return Ok(Self::Url(url));
        }

        #[cfg(windows)]
        // Detect windows absolute paths
        if value.contains('\\') {
            return Ok(Self::Path(value.to_string()));
        }

        match first_char {
            '.' | '/' => Ok(Self::Path(value.to_string())),
            _ => PackageIdent::from_str(value).map(Self::Ident),
        }
    }
}

impl serde::Serialize for PackageSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Ident(id) => id.serialize(serializer),
            Self::Path(path) => path.serialize(serializer),
            Self::Url(url) => url.serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for PackageSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PackageSource::from_str(&s).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl schemars::JsonSchema for PackageSource {
    fn schema_name() -> String {
        "PackageSource".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
mod tests {
    use crate::package::Tag;

    use super::*;

    #[test]
    fn test_parse_package_specifier() {
        // Parse as WebcIdent
        assert_eq!(
            PackageSource::from_str("ns/name").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            })
        );

        assert_eq!(
            PackageSource::from_str("ns/name@").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }),
            "empty tag should be parsed as None"
        );

        assert_eq!(
            PackageSource::from_str("ns/name@tag").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name@tag").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name@tag").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            })
        );

        assert_eq!(
            PackageSource::from_str("reg.com:ns/name@tag").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            })
        );

        // Failure cases.
        assert_eq!(
            PackageSource::from_str("alpha"),
            Ok(PackageSource::from(NamedPackageIdent {
                registry: None,
                namespace: None,
                name: "alpha".to_string(),
                tag: None,
            }))
        );

        assert_eq!(
            PackageSource::from_str(""),
            Err(PackageParseError::new(
                "",
                "An empty string is not a valid package source"
            ))
        );
        assert_eq!(
            PackageSource::from_str("ns/name").unwrap(),
            PackageSource::from(NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            })
        );

        assert_eq!(
            PackageSource::from_str(
                "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
            )
            .unwrap(),
            PackageSource::from(
                PackageHash::from_str(
                    "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
                )
                .unwrap()
            )
        );

        let wants = vec![
            "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
            "./dir",
            "ns/name",
            "ns/name@",
            "ns/name@tag",
        ];
        for want in wants {
            let spec = PackageSource::from_str(want).unwrap();
            assert_eq!(spec, PackageSource::from_str(&spec.to_string()).unwrap());
        }
    }

    #[test]
    fn parse_package_sources() {
        let inputs = [
            (
                "first",
                PackageSource::from(NamedPackageIdent {
                    registry: None,
                    namespace: None,
                    name: "first".to_string(),
                    tag: None,
                }),
            ),
            (
                "namespace/package",
                PackageSource::from(NamedPackageIdent {
                    registry: None,
                    namespace: Some("namespace".to_string()),
                    name: "package".to_string(),
                    tag: None,
                }),
            ),
            (
                "namespace/package@1.0.0",
                PackageSource::from(NamedPackageIdent {
                    registry: None,
                    namespace: Some("namespace".to_string()),
                    name: "package".to_string(),
                    tag: Some(Tag::VersionReq("1.0.0".parse().unwrap())),
                }),
            ),
            (
                "namespace/package@latest",
                PackageSource::from(NamedPackageIdent {
                    registry: None,
                    namespace: Some("namespace".to_string()),
                    name: "package".to_string(),
                    tag: Some(Tag::VersionReq(semver::VersionReq::STAR)),
                }),
            ),
            (
                "https://wapm/io/namespace/package@1.0.0",
                PackageSource::Url("https://wapm/io/namespace/package@1.0.0".parse().unwrap()),
            ),
            (
                "/path/to/some/file.webc",
                PackageSource::Path("/path/to/some/file.webc".into()),
            ),
            ("./file.webc", PackageSource::Path("./file.webc".into())),
            #[cfg(windows)]
            (
                r"C:\Path\to\some\file.webc",
                PackageSource::Path(r"C:\Path\to\some\file.webc".into()),
            ),
        ];

        for (index, (src, expected)) in inputs.into_iter().enumerate() {
            eprintln!("testing pattern {}", index + 1);
            let parsed = PackageSource::from_str(src).unwrap();
            assert_eq!(parsed, expected);
        }
    }
}
