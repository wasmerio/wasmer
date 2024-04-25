use std::{fmt::Write, str::FromStr};

use semver::VersionReq;

use super::{NamedPackageId, PackageParseError};

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Tag {
    Named(String),
    VersionReq(semver::VersionReq),
}

impl Tag {
    pub fn as_named(&self) -> Option<&String> {
        if let Self::Named(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_version_req(&self) -> Option<&semver::VersionReq> {
        if let Self::VersionReq(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tag::Named(n) => n.fmt(f),
            Tag::VersionReq(v) => v.fmt(f),
        }
    }
}

impl std::str::FromStr for Tag {
    type Err = PackageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "latest" {
            Ok(Self::VersionReq(semver::VersionReq::STAR))
        } else {
            match semver::VersionReq::from_str(s) {
                Ok(v) => Ok(Self::VersionReq(v)),
                Err(_) => Ok(Self::Named(s.to_string())),
            }
        }
    }
}

/// Parsed representation of a package identifier.
///
/// Format:
/// [https?://<domain>/][namespace/]name[@version]
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct NamedPackageIdent {
    pub registry: Option<String>,
    pub namespace: Option<String>,
    pub name: String,
    pub tag: Option<Tag>,
}

impl NamedPackageIdent {
    pub fn try_from_full_name_and_version(
        full_name: &str,
        version: &str,
    ) -> Result<Self, PackageParseError> {
        let (namespace, name) = match full_name.split_once('/') {
            Some((ns, name)) => (Some(ns.to_owned()), name.to_owned()),
            None => (None, full_name.to_owned()),
        };

        let version = version
            .parse::<VersionReq>()
            .map_err(|e| PackageParseError::new(version, e.to_string()))?;

        Ok(Self {
            registry: None,
            namespace,
            name,
            tag: Some(Tag::VersionReq(version)),
        })
    }

    pub fn tag_str(&self) -> Option<String> {
        self.tag.as_ref().map(|x| x.to_string())
    }

    /// Namespaced name.
    ///
    /// Eg: "namespace/name"
    pub fn full_name(&self) -> String {
        if let Some(ns) = &self.namespace {
            format!("{}/{}", ns, self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn version_opt(&self) -> Option<&VersionReq> {
        match &self.tag {
            Some(Tag::VersionReq(v)) => Some(v),
            Some(Tag::Named(_)) | None => None,
        }
    }

    pub fn version_or_default(&self) -> VersionReq {
        match &self.tag {
            Some(Tag::VersionReq(v)) => v.clone(),
            Some(Tag::Named(_)) | None => semver::VersionReq::STAR,
        }
    }

    pub fn registry_url(&self) -> Result<Option<url::Url>, PackageParseError> {
        let Some(reg) = &self.registry else {
            return Ok(None);
        };

        let reg = if !reg.starts_with("http://") && !reg.starts_with("https://") {
            format!("https://{}", reg)
        } else {
            reg.clone()
        };

        url::Url::parse(&reg)
            .map_err(|e| PackageParseError::new(reg, e.to_string()))
            .map(Some)
    }

    /// Build the ident for a package.
    ///
    /// Format: [NAMESPACE/]NAME[@tag]
    pub fn build_identifier(&self) -> String {
        let mut ident = if let Some(ns) = &self.namespace {
            format!("{}/{}", ns, self.name)
        } else {
            self.name.to_string()
        };

        if let Some(tag) = &self.tag {
            ident.push('@');
            // Writing to a string only fails on memory allocation errors.
            write!(&mut ident, "{}", tag).unwrap();
        }
        ident
    }

    pub fn build(&self) -> String {
        let mut out = String::new();
        if let Some(url) = &self.registry {
            // NOTE: writing to a String can only fail on allocation errors.
            write!(&mut out, "{}", url).unwrap();

            if !out.ends_with('/') {
                out.push(':');
            }
        }
        if let Some(ns) = &self.namespace {
            out.push_str(ns);
            out.push('/');
        }
        out.push_str(&self.name);
        if let Some(tag) = &self.tag {
            out.push('@');
            // Writing to a string only fails on memory allocation errors.
            write!(&mut out, "{}", tag).unwrap();
        }

        out
    }
}

impl From<NamedPackageId> for NamedPackageIdent {
    fn from(value: NamedPackageId) -> Self {
        let (namespace, name) = match value.full_name.split_once('/') {
            Some((ns, name)) => (Some(ns.to_owned()), name.to_owned()),
            None => (None, value.full_name),
        };

        Self {
            registry: None,
            namespace,
            name,
            tag: Some(Tag::VersionReq(semver::VersionReq {
                comparators: vec![semver::Comparator {
                    op: semver::Op::Exact,
                    major: value.version.major,
                    minor: Some(value.version.minor),
                    patch: Some(value.version.patch),
                    pre: value.version.pre,
                }],
            })),
        }
    }
}

impl std::str::FromStr for NamedPackageIdent {
    type Err = PackageParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (rest, tag_opt) = value
            .trim()
            .rsplit_once('@')
            .map(|(x, y)| (x, if y.is_empty() { None } else { Some(y) }))
            .unwrap_or((value, None));

        let tag = if let Some(v) = tag_opt.filter(|x| !x.is_empty()) {
            Some(Tag::from_str(v)?)
        } else {
            None
        };

        let (rest, name) = if let Some((r, n)) = rest.rsplit_once('/') {
            (r, n)
        } else {
            ("", rest)
        };

        let name = name.trim();
        if name.is_empty() {
            return Err(PackageParseError::new(value, "package name is required"));
        }

        let (rest, namespace) = if rest.is_empty() {
            ("", None)
        } else {
            let (rest, ns) = rest.rsplit_once(':').unwrap_or(("", rest));

            let ns = ns.trim();

            if ns.is_empty() {
                return Err(PackageParseError::new(value, "namespace can not be empty"));
            }
            (rest, Some(ns.to_string()))
        };

        let rest = rest.trim();
        let registry = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };

        Ok(Self {
            registry,
            namespace,
            name: name.to_string(),
            tag,
        })
    }
}

impl std::fmt::Display for NamedPackageIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.build())
    }
}

impl serde::Serialize for NamedPackageIdent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for NamedPackageIdent {
    fn deserialize<D>(deserializer: D) -> Result<NamedPackageIdent, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for NamedPackageIdent {
    fn schema_name() -> String {
        "NamedPackageIdent".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::package::PackageParseError;

    use super::*;

    #[test]
    fn test_parse_webc_ident() {
        // Success cases.

        assert_eq!(
            NamedPackageIdent::from_str("ns/name").unwrap(),
            NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("ns/name@").unwrap(),
            NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            },
            "empty tag should be parsed as None"
        );

        assert_eq!(
            NamedPackageIdent::from_str("ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: None,
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com:ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some("reg.com".to_string()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some(Tag::Named("tag".to_string())),
            }
        );

        // Failure cases.

        assert_eq!(
            NamedPackageIdent::from_str("alpha").unwrap(),
            NamedPackageIdent {
                registry: None,
                namespace: None,
                name: "alpha".to_string(),
                tag: None,
            },
        );

        assert_eq!(
            NamedPackageIdent::from_str(""),
            Err(PackageParseError::new("", "package name is required"))
        );
    }

    #[test]
    fn test_serde_serialize_package_ident_with_repo() {
        // Serialize
        let ident = NamedPackageIdent {
            registry: Some("wapm.io".to_string()),
            namespace: Some("ns".to_string()),
            name: "name".to_string(),
            tag: None,
        };

        let raw = serde_json::to_string(&ident).unwrap();
        assert_eq!(raw, "\"wapm.io:ns/name\"");

        let ident2 = serde_json::from_str::<NamedPackageIdent>(&raw).unwrap();
        assert_eq!(ident, ident2);
    }

    #[test]
    fn test_serde_serialize_webc_str_ident_without_repo() {
        // Serialize
        let ident = NamedPackageIdent {
            registry: None,
            namespace: Some("ns".to_string()),
            name: "name".to_string(),
            tag: None,
        };

        let raw = serde_json::to_string(&ident).unwrap();
        assert_eq!(raw, "\"ns/name\"");

        let ident2 = serde_json::from_str::<NamedPackageIdent>(&raw).unwrap();
        assert_eq!(ident, ident2);
    }
}
