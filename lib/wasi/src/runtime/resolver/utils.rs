use std::path::Path;

use anyhow::{Context, Error};
use url::Url;
use webc::{
    metadata::{annotations::Wapm, Manifest, UrlOrManifest},
    Container,
};

use crate::runtime::resolver::{Dependency, PackageSpecifier, SourceId, Summary, WebcHash};

pub(crate) fn extract_summary_from_webc(path: &Path, source: SourceId) -> Result<Summary, Error> {
    let path = path.canonicalize()?;
    let container = Container::from_disk(&path)?;
    let webc_sha256 = WebcHash::for_file(&path)?;
    let url = Url::from_file_path(&path)
        .map_err(|_| anyhow::anyhow!("Unable to turn \"{}\" into a file:// URL", path.display()))?;

    extract_summary_from_manifest(container.manifest(), source, url, webc_sha256)
}

pub(crate) fn extract_summary_from_manifest(
    manifest: &Manifest,
    source: SourceId,
    url: Url,
    webc_sha256: WebcHash,
) -> Result<Summary, Error> {
    let Wapm { name, version, .. } = manifest
        .package_annotation("wapm")?
        .context("Unable to find the \"wapm\" annotations")?;

    let dependencies = manifest
        .use_map
        .iter()
        .map(|(alias, value)| {
            Ok(Dependency {
                alias: alias.clone(),
                pkg: url_or_manifest_to_specifier(value)?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let commands = manifest
        .commands
        .iter()
        .map(|(name, _value)| crate::runtime::resolver::Command {
            name: name.to_string(),
        })
        .collect();

    Ok(Summary {
        package_name: name,
        version: version.parse()?,
        webc: url,
        webc_sha256,
        dependencies,
        commands,
        source,
        entrypoint: manifest.entrypoint.clone(),
    })
}

fn url_or_manifest_to_specifier(value: &UrlOrManifest) -> Result<PackageSpecifier, Error> {
    match value {
        UrlOrManifest::Url(url) => Ok(PackageSpecifier::Url(url.clone())),
        UrlOrManifest::Manifest(manifest) => {
            if let Ok(Some(Wapm { name, version, .. })) = manifest.package_annotation("wapm") {
                let version = version.parse()?;
                return Ok(PackageSpecifier::Registry {
                    full_name: name,
                    version,
                });
            }

            if let Some(origin) = manifest
                .origin
                .as_deref()
                .and_then(|origin| Url::parse(origin).ok())
            {
                return Ok(PackageSpecifier::Url(origin));
            }

            Err(Error::msg(
                "Unable to determine a package specifier for a vendored dependency",
            ))
        }
        UrlOrManifest::RegistryDependentUrl(specifier) => specifier.parse(),
    }
}
