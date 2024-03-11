//! Module for parsing and installing packages

use std::str::FromStr;

use url::Url;

/// Source of a package
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageSource {
    /// Download from a URL
    Url(Url),
    /// Run a local file
    File(String),
    /// Download from a package
    Package(wasmer_registry::Package),
}

impl Default for PackageSource {
    fn default() -> Self {
        PackageSource::File(String::new())
    }
}

impl FromStr for PackageSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl PackageSource {
    /// Parses a package source and transforms it to a URL or a File
    pub fn parse(s: &str) -> Result<Self, String> {
        // If the file is a http:// URL, run the URL
        if let Ok(url) = url::Url::parse(s) {
            if url.scheme() == "http" || url.scheme() == "https" {
                return Ok(Self::Url(url));
            }
        }

        Ok(match wasmer_registry::Package::from_str(s) {
            Ok(o) => Self::Package(o),
            Err(_) => Self::File(s.to_string()),
        })
    }
}

#[test]
fn test_package_source() {
    assert_eq!(
        PackageSource::parse("registry.wasmer.io/graphql/python/python").unwrap(),
        PackageSource::File("registry.wasmer.io/graphql/python/python".to_string()),
    );

    assert_eq!(
        PackageSource::parse("/absolute/path/test.wasm").unwrap(),
        PackageSource::File("/absolute/path/test.wasm".to_string()),
    );

    assert_eq!(
        PackageSource::parse("C://absolute/path/test.wasm").unwrap(),
        PackageSource::File("C://absolute/path/test.wasm".to_string()),
    );

    assert_eq!(
        PackageSource::parse("namespace/name@latest").unwrap(),
        PackageSource::Package(wasmer_registry::Package {
            namespace: "namespace".to_string(),
            name: "name".to_string(),
            version: Some("latest".to_string()),
        })
    );

    assert_eq!(
        PackageSource::parse("namespace/name@latest:command").unwrap(),
        PackageSource::File("namespace/name@latest:command".to_string()),
    );

    assert_eq!(
        PackageSource::parse("namespace/name@1.0.2").unwrap(),
        PackageSource::Package(wasmer_registry::Package {
            namespace: "namespace".to_string(),
            name: "name".to_string(),
            version: Some("1.0.2".to_string()),
        })
    );

    assert_eq!(
        PackageSource::parse("namespace/name@1.0.2-rc.2").unwrap(),
        PackageSource::Package(wasmer_registry::Package {
            namespace: "namespace".to_string(),
            name: "name".to_string(),
            version: Some("1.0.2-rc.2".to_string()),
        })
    );

    assert_eq!(
        PackageSource::parse("namespace/name").unwrap(),
        PackageSource::Package(wasmer_registry::Package {
            namespace: "namespace".to_string(),
            name: "name".to_string(),
            version: None,
        })
    );

    assert_eq!(
        PackageSource::parse("https://wasmer.io/syrusakbary/python").unwrap(),
        PackageSource::Url(url::Url::parse("https://wasmer.io/syrusakbary/python").unwrap()),
    );

    assert_eq!(
        PackageSource::parse("command").unwrap(),
        PackageSource::File("command".to_string()),
    );

    assert_eq!(
        PackageSource::parse("python@latest").unwrap(),
        PackageSource::File("python@latest".to_string()),
    );
}
