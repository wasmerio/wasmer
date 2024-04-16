use std::{fmt::Write, str::FromStr};

use super::PackageParseError;

/// Parsed representation of a package identifier.
///
/// Format:
/// [https?://<domain>/][namespace/]name[@version]
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct NamedPackageIdent {
    pub registry: Option<url::Url>,
    pub namespace: Option<String>,
    pub name: String,
    pub tag: Option<String>,
}

impl NamedPackageIdent {
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
            ident.push_str(tag);
        }
        ident
    }

    pub fn build(&self) -> String {
        let mut out = String::new();
        if let Some(url) = &self.registry {
            // NOTE: writing to a String can only fail on allocation errors.
            write!(&mut out, "{}", url).unwrap();

            if !out.ends_with('/') {
                out.push('/');
            }
        }
        if let Some(ns) = &self.namespace {
            out.push_str(&ns);
            out.push('/');
        }
        out.push_str(&self.name);
        if let Some(tag) = &self.tag {
            out.push('@');
            out.push_str(tag);
        }

        out
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
            let (rest, ns) = rest.rsplit_once('/').unwrap_or(("", rest));

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
            let registry = rest;
            let full_registry =
                if registry.starts_with("http://") || registry.starts_with("https://") {
                    registry.to_string()
                } else {
                    format!("https://{}", registry)
                };

            let registry_url = url::Url::parse(&full_registry).map_err(|e| {
                PackageParseError::new(value, format!("invalid registry url: {}", e))
            })?;
            Some(registry_url)
        };

        Ok(Self {
            registry,
            namespace,
            name: name.to_string(),
            tag: tag_opt.map(|x| x.to_string()),
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
                tag: Some("tag".to_string()),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com/ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("https://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("reg.com/ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("https://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some("tag".to_string()),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("https://reg.com/ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("https://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("https://reg.com/ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("https://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some("tag".to_string()),
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("http://reg.com/ns/name").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("http://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: None,
            }
        );

        assert_eq!(
            NamedPackageIdent::from_str("http://reg.com/ns/name@tag").unwrap(),
            NamedPackageIdent {
                registry: Some(url::Url::parse("http://reg.com").unwrap()),
                namespace: Some("ns".to_string()),
                name: "name".to_string(),
                tag: Some("tag".to_string()),
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
            registry: Some(url::Url::parse("https://wapm.io").unwrap()),
            namespace: Some("ns".to_string()),
            name: "name".to_string(),
            tag: None,
        };

        let raw = serde_json::to_string(&ident).unwrap();
        assert_eq!(raw, "\"https://wapm.io/ns/name\"");

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
