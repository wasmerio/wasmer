use std::path::Path;

use wasmer_config::package::ModuleReference;

use webc::Container;

use super::ConversionError;

/// Convert a webc image into a directory with a wasmer.toml file that can
/// be used for generating a new package.
pub fn webc_to_package_dir(webc: &Container, target_dir: &Path) -> Result<(), ConversionError> {
    let mut pkg_manifest = wasmer_config::package::Manifest::new_empty();

    let manifest = webc.manifest();
    // Convert the package annotation.

    let pkg_annotation = manifest
        .wapm()
        .map_err(|err| ConversionError::msg(format!("could not read package annotation: {err}")))?;
    if let Some(ann) = pkg_annotation {
        let mut pkg = wasmer_config::package::Package::new_empty();

        pkg.name = ann.name;
        pkg.version = if let Some(raw) = ann.version {
            let v = raw
                .parse()
                .map_err(|e| ConversionError::with_cause("invalid package version", e))?;
            Some(v)
        } else {
            None
        };

        pkg.description = ann.description;
        pkg.license = ann.license;

        // TODO: map license_file and README (paths!)

        pkg.homepage = ann.homepage;
        pkg.repository = ann.repository;
        pkg.private = ann.private;
        pkg.entrypoint = manifest.entrypoint.clone();

        pkg_manifest.package = Some(pkg);
    }

    // Map dependencies.
    for (_name, target) in &manifest.use_map {
        match target {
            webc::metadata::UrlOrManifest::Url(_url) => {
                // Not supported.
            }
            webc::metadata::UrlOrManifest::Manifest(_) => {
                // Not supported.
            }
            webc::metadata::UrlOrManifest::RegistryDependentUrl(raw) => {
                let (name, version) = if let Some((name, version_raw)) = raw.split_once('@') {
                    let version = version_raw.parse().map_err(|err| {
                        ConversionError::with_cause(
                            format!("Could not parse version of dependency: '{raw}'"),
                            err,
                        )
                    })?;
                    (name.to_string(), version)
                } else {
                    (raw.to_string(), "*".parse().unwrap())
                };

                pkg_manifest.dependencies.insert(name, version);
            }
        }
    }

    // Convert filesystem mappings.

    let fs_annotation = manifest
        .filesystem()
        .map_err(|err| ConversionError::msg(format!("could not read fs annotation: {err}")))?;
    if let Some(ann) = fs_annotation {
        for mapping in ann.0 {
            if mapping.from.is_some() {
                // wasmer.toml does not allow specifying dependency mounts.
                continue;
            }

            // Extract the volume to "<target-dir>/<volume-name>".
            let volume = webc.get_volume(&mapping.volume_name).ok_or_else(|| {
                ConversionError::msg(format!(
                    "Package annotations specify a volume that does not exist: '{}'",
                    mapping.volume_name
                ))
            })?;

            // The volume name is taken verbatim from the (untrusted) webc and
            // is used to build an on-disk path. Without normalization a name
            // such as "../x" would unpack outside target_dir, so route it
            // through the same sanitizer used elsewhere in the crate.
            let volume_subpath = webc::sanitize_path(&mapping.volume_name);
            let volume_path = target_dir.join(volume_subpath.trim_start_matches('/'));

            std::fs::create_dir_all(&volume_path).map_err(|err| {
                ConversionError::with_cause(
                    format!(
                        "could not create volume directory '{}'",
                        volume_path.display()
                    ),
                    err,
                )
            })?;

            volume.unpack("/", &volume_path).map_err(|err| {
                ConversionError::with_cause("could not unpack volume to filesystemt", err)
            })?;

            let mut source_path = mapping
                .volume_name
                .trim_start_matches('/')
                .trim_end_matches('/')
                .to_string();
            if let Some(subpath) = mapping.host_path {
                if !source_path.ends_with('/') {
                    source_path.push('/');
                }
                source_path.push_str(&subpath);
            }
            source_path.insert_str(0, "./");

            pkg_manifest
                .fs
                .insert(mapping.mount_path, source_path.into());
        }
    }

    // Convert modules.

    let module_dir_name = "modules";
    let module_dir = target_dir.join(module_dir_name);

    let atoms = webc.atoms();
    if !atoms.is_empty() {
        std::fs::create_dir_all(&module_dir).map_err(|err| {
            ConversionError::with_cause(
                format!("Could not create directory '{}'", module_dir.display()),
                err,
            )
        })?;
        for (atom_name, data) in atoms {
            // Atom names also come from the container, so sanitize before
            // joining to keep the write inside module_dir.
            let atom_subpath = webc::sanitize_path(&atom_name);
            let atom_path = module_dir.join(atom_subpath.trim_start_matches('/'));

            std::fs::write(&atom_path, &data).map_err(|err| {
                ConversionError::with_cause(
                    format!("Could not write atom to path '{}'", atom_path.display()),
                    err,
                )
            })?;

            let relative_path = format!("./{module_dir_name}/{atom_name}");

            pkg_manifest.modules.push(wasmer_config::package::Module {
                name: atom_name,
                source: relative_path.into(),
                abi: wasmer_config::package::Abi::None,
                kind: None,
                interfaces: None,
                bindings: None,
                annotations: None,
            });
        }
    }

    // Convert commands.
    for (name, spec) in &manifest.commands {
        let mut annotations = toml::Table::new();
        for (key, value) in &spec.annotations {
            if key == webc::metadata::annotations::Atom::KEY {
                continue;
            }

            let raw_toml = toml::to_string(&value).unwrap();
            let toml_value = toml::from_str::<toml::Value>(&raw_toml).unwrap();
            annotations.insert(key.into(), toml_value);
        }

        let atom_annotation = spec
            .annotation::<webc::metadata::annotations::Atom>(webc::metadata::annotations::Atom::KEY)
            .map_err(|err| {
                ConversionError::msg(format!(
                    "could not read atom annotation for command '{name}': {err}"
                ))
            })?
            .ok_or_else(|| {
                ConversionError::msg(format!(
                    "Command '{name}' is missing the required atom annotation"
                ))
            })?;

        let module = if let Some(dep) = atom_annotation.dependency {
            ModuleReference::Dependency {
                dependency: dep,
                module: atom_annotation.name,
            }
        } else {
            ModuleReference::CurrentPackage {
                module: atom_annotation.name,
            }
        };

        let cmd = wasmer_config::package::Command::V2(wasmer_config::package::CommandV2 {
            name: name.clone(),
            module,
            runner: spec.runner.clone(),
            annotations: Some(wasmer_config::package::CommandAnnotations::Raw(
                annotations.into(),
            )),
        });

        pkg_manifest.commands.push(cmd);
    }

    // Write out the manifest.
    let manifest_toml = toml::to_string(&pkg_manifest)
        .map_err(|err| ConversionError::with_cause("could not serialize package manifest", err))?;
    std::fs::write(target_dir.join("wasmer.toml"), manifest_toml)
        .map_err(|err| ConversionError::with_cause("could not write wasmer.toml", err))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir_all;

    use pretty_assertions::assert_eq;

    use crate::{package::Package, utils::from_bytes};

    use super::*;

    // A hostile webc can name a volume "../something". Extracting it must not
    // escape the requested output directory.
    #[test]
    fn webc_to_package_dir_contains_volume_name_traversal() {
        use std::collections::BTreeMap;

        use webc::{
            PathSegment,
            metadata::{
                Manifest,
                annotations::{FileSystemMapping, FileSystemMappings},
            },
            v3::{
                ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
                write::{DirEntry, Directory, FileEntry, Writer},
            },
        };

        let tmpdir = tempfile::tempdir().unwrap();
        let root = tmpdir.path();
        // The caller creates the output directory before invoking the converter.
        let target_dir = root.join("out");
        create_dir_all(&target_dir).unwrap();

        let escaping_name = "../escaped";

        let mut manifest = Manifest::default();
        let mappings = FileSystemMappings(vec![FileSystemMapping {
            from: None,
            volume_name: escaping_name.to_string(),
            host_path: None,
            mount_path: "/data".to_string(),
        }]);
        manifest.package.insert(
            FileSystemMappings::KEY.to_string(),
            ciborium::Value::serialized(&mappings).unwrap(),
        );

        let mut children: BTreeMap<PathSegment, DirEntry> = BTreeMap::new();
        children.insert(
            "secret".parse().unwrap(),
            DirEntry::File(FileEntry::owned(b"PWNED".to_vec(), Timestamps::default())),
        );
        let volume = Directory::new(children, Timestamps::default());

        let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::new())
            .unwrap();
        writer.write_volume(escaping_name, volume).unwrap();
        let bytes = writer.finish(SignatureAlgorithm::None).unwrap();

        let container = from_bytes(bytes).unwrap();

        webc_to_package_dir(&container, &target_dir).unwrap();

        // The volume contents must stay under target_dir, never in a sibling.
        let escaped = root.join("escaped").join("secret");
        assert!(
            !escaped.exists(),
            "volume escaped the output directory to {}",
            escaped.display()
        );
        assert!(target_dir.join("escaped").join("secret").exists());
    }

    // Build a webc from a package directory, and then restore the directory
    // from the webc.
    #[test]
    fn test_wasmer_package_webc_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();

        let webc = {
            let dir_input = dir.join("input");
            let dir_public = dir_input.join("public");

            create_dir_all(&dir_public).unwrap();

            std::fs::write(dir_public.join("index.html"), "INDEX").unwrap();

            std::fs::write(dir_input.join("mywasm.wasm"), "()").unwrap();

            std::fs::write(
                dir_input.join("wasmer.toml"),
                r#"
[package]
name = "testns/testpkg"
version = "0.0.1"
description = "descr1"
license = "MIT"

[dependencies]
"wasmer/python" = "8.12.0"

[fs]
public = "./public"

[[module]]
name = "mywasm"
source = "./mywasm.wasm"

[[command]]
name = "run"
module = "mywasm"
runner = "wasi"

[command.annotations.wasi]
env =  ["A=B"]
main-args = ["/mounted/script.py"]
"#,
            )
            .unwrap();

            let pkg = Package::from_manifest(dir_input.join("wasmer.toml")).unwrap();
            let raw = pkg.serialize().unwrap();
            from_bytes(raw).unwrap()
        };

        let dir_output = dir.join("output");
        webc_to_package_dir(&webc, &dir_output).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir_output.join("public/index.html")).unwrap(),
            "INDEX",
        );

        assert_eq!(
            std::fs::read_to_string(dir_output.join("modules/mywasm")).unwrap(),
            "()",
        );

        assert_eq!(
            std::fs::read_to_string(dir_output.join("wasmer.toml"))
                .unwrap()
                .trim(),
            r#"

[package]
license = "MIT"
entrypoint = "run"

[dependencies]
"wasmer/python" = "^8.12.0"

[fs]
"/public" = "./public"

[[module]]
name = "mywasm"
source = "./modules/mywasm"

[[command]]
name = "run"
module = "mywasm"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
atom = "mywasm"
env = ["A=B"]
main-args = ["/mounted/script.py"]
            "#
            .trim(),
        );
    }
}
