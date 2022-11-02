//! The publish command uploads the package specified in the Manifest (`wapm.toml`)
//! to the wapm registry.

use std::fs;
use std::path::{Path, PathBuf};
use clap::Parser;
use log::{info, warn, error};
use wasmer_registry::MANIFEST_FILE_NAME;

#[derive(Parser, Debug)]
pub struct Publish {
    /// Run the publish logic without sending anything to the registry server
    #[clap(long = "dry-run")]
    pub dry_run: bool,
    /// Run without printing progress bars to stdout (useful for unit testing)
    #[clap(long = "silent")]
    pub silent: bool,
    /// Which registry to publish to
    #[clap(name = "REGISTRY")]
    pub registry: Option<String>,
    /// Path to the directory to package, e.g. `wasmer publish /my/package`
    #[clap(name = "PACKAGE_DIR")]
    pub directory: Option<String>,
}

impl Publish {
    pub fn execute(&self) -> anyhow::Result<()> {
        let mut builder = Builder::new(Vec::new());
        let cwd = match self.directory.as_ref() {
            Some(s) => std::path::Path::new(s).to_path_buf(),
            None => wasmer_registry::PartialWapmConfig::get_current_dir()?,
        };
    
        wasmer_registry::validate::validate_directory(cwd.clone())?;
    
        let manifest = Manifest::find_in_directory(&cwd)?;
    
        let manifest_path_buf = cwd.join(MANIFEST_FILE_NAME);
        builder.append_path_with_name(&manifest_path_buf, MANIFEST_FILE_NAME)?;
        let package = &manifest.package;
        let modules = manifest.module.as_ref().ok_or(PublishError::NoModule)?;
        let manifest_string = toml::to_string(&manifest)?;
    
        let readme = package.readme.as_ref().and_then(|readme_path| {
            let normalized_path = normalize_path(&manifest.base_directory_path, readme_path);
            if builder.append_path(&normalized_path).is_err() {
                // TODO: Maybe do something here
            }
            fs::read_to_string(normalized_path).ok()
        });
        let license_file = package.license_file.as_ref().and_then(|license_file_path| {
            let normalized_path = normalize_path(&manifest.base_directory_path, license_file_path);
            if builder.append_path(&normalized_path).is_err() {
                // TODO: Maybe do something here
            }
            fs::read_to_string(normalized_path).ok()
        });
    
        for module in modules {
            let normalized_path = normalize_path(&manifest.base_directory_path, &module.source);
            normalized_path
                .metadata()
                .map_err(|_| PublishError::SourceMustBeFile {
                    module: module.name.clone(),
                    path: normalized_path.clone(),
                })?;
            builder
                .append_path(normalized_path)
                .map_err(|_| PublishError::ErrorBuildingPackage(module.name.clone()))?;
    
            if let Some(bindings) = &module.bindings {
                for path in bindings.referenced_files(&manifest.base_directory_path) {
                    let normalized_path = normalize_path(&manifest.base_directory_path, &path);
                    normalized_path
                        .metadata()
                        .map_err(|_| PublishError::MissingBindings {
                            module: module.name.clone(),
                            path: normalized_path.clone(),
                        })?;
                    builder
                        .append_path(normalized_path)
                        .map_err(|_| PublishError::ErrorBuildingPackage(module.name.clone()))?;
                }
            }
        }
    
        // bundle the package filesystem
        for (_alias, path) in manifest.fs.unwrap_or_default().iter() {
            let normalized_path = normalize_path(&cwd, path);
            let path_metadata = normalized_path.metadata().map_err(|_| {
                PublishError::MissingManifestFsPath(normalized_path.to_string_lossy().to_string())
            })?;
            if path_metadata.is_dir() {
                builder.append_dir_all(path, &normalized_path)
            } else {
                return Err(PublishError::PackageFileSystemEntryMustBeDirectory(
                    path.to_string_lossy().to_string(),
                )
                .into());
            }
            .map_err(|_| {
                PublishError::MissingManifestFsPath(normalized_path.to_string_lossy().to_string())
            })?;
        }
    
        builder.finish().ok();
        let tar_archive_data = builder.into_inner().map_err(|_|
                                                            // TODO:
                                                            PublishError::NoModule)?;
        let archive_name = "package.tar.gz".to_string();
        let archive_dir = create_temp_dir()?;
        let archive_dir_path: &std::path::Path = archive_dir.as_ref();
        fs::create_dir(archive_dir_path.join("wapm_package"))?;
        let archive_path = archive_dir_path.join("wapm_package").join(&archive_name);
        let mut compressed_archive = fs::File::create(&archive_path).unwrap();
        let mut gz_enc = GzEncoder::new(&mut compressed_archive, Compression::default());
    
        gz_enc.write_all(&tar_archive_data).unwrap();
        let _compressed_archive = gz_enc.finish().unwrap();
        let mut compressed_archive_reader = fs::File::open(&archive_path)?;
    
        let maybe_signature_data = sign_compressed_archive(&mut compressed_archive_reader)?;
        let archived_data_size = archive_path.metadata()?.len();
        let use_chunked_uploads = archived_data_size > 1242880;
    
        assert!(archive_path.exists());
        assert!(archive_path.is_file());
    
        if publish_opts.dry_run {
            // dry run: publish is done here
    
            println!(
                "Successfully published package `{}@{}`",
                package.name, package.version
            );
    
            info!(
                "Publish succeeded, but package was not published because it was run in dry-run mode"
            );
    
            return Ok(());
        }
    
        // file is larger than 1MB, use chunked uploads
        if std::env::var("FORCE_WAPM_USE_CHUNKED_UPLOAD").is_ok()
            || (std::env::var("WAPM_USE_CHUNKED_UPLOAD").is_ok() && use_chunked_uploads)
        {
            wasmer_registry::publish::try_chunked_uploading(
                package,
                &manifest_string,
                &license_file,
                &readme,
                &archive_name,
                &archive_path,
                &maybe_signature_data,
                archived_data_size,
            )
            .or_else(|_| {
                wasmer_registry::publish::try_default_uploading(
                    package,
                    &manifest_string,
                    &license_file,
                    &readme,
                    &archive_name,
                    &archive_path,
                    &maybe_signature_data,
                    &publish_opts,
                )
            })
        } else {
            wasmer_registry::publish::try_default_uploading(
                package,
                &manifest_string,
                &license_file,
                &readme,
                &archive_name,
                &archive_path,
                &maybe_signature_data,
                &publish_opts,
            )
        }
    }    
}

fn normalize_path(cwd: &Path, path: &Path) -> PathBuf {
    let mut out = PathBuf::from(cwd);
    let mut components = path.components();
    if path.is_absolute() {
        warn!(
            "Interpreting absolute path {} as a relative path",
            path.to_string_lossy()
        );
        components.next();
    }
    for comp in components {
        out.push(comp);
    }
    out
}

fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}

#[derive(Debug, Error)]
enum PublishError {
    #[error("Cannot publish without a module.")]
    NoModule,
    #[error("Unable to publish the \"{module}\" module because \"{}\" is not a file", path.display())]
    SourceMustBeFile { module: String, path: PathBuf },
    #[error("Unable to load the bindings for \"{module}\" because \"{}\" doesn't exist", path.display())]
    MissingBindings { module: String, path: PathBuf },
    #[error("Error building package when parsing module \"{0}\".")]
    ErrorBuildingPackage(String),
    #[error(
        "Path \"{0}\", specified in the manifest as part of the package file system does not exist.",
    )]
    MissingManifestFsPath(String),
    #[error("When processing the package filesystem, found path \"{0}\" which is not a directory")]
    PackageFileSystemEntryMustBeDirectory(String),
}

/// Takes the package archive as a File and attempts to sign it using the active key
/// returns the public key id used to sign it and the signature string itself
pub fn sign_compressed_archive(
    compressed_archive: &mut fs::File,
) -> anyhow::Result<SignArchiveResult> {
    let key_db = database::open_db()?;
    let personal_key = if let Ok(v) = keys::get_active_personal_key(&key_db) {
        v
    } else {
        return Ok(SignArchiveResult::NoKeyRegistered);
    };
    let password = rpassword::prompt_password(&format!(
        "Please enter your password for the key pair {}:",
        &personal_key.public_key_id
    ))
    .ok();
    let private_key = if let Some(priv_key_location) = personal_key.private_key_location {
        match minisign::SecretKey::from_file(&priv_key_location, password) {
            Ok(priv_key_data) => priv_key_data,
            Err(e) => {
                error!(
                    "Could not read private key from location {}: {}",
                    priv_key_location, e
                );
                return Err(e.into());
            }
        }
    } else {
        // TODO: add more info about why this might have happened and what the user can do about it
        warn!("Active key does not have a private key location registered with it!");
        return Err(anyhow!("Cannot sign package, no private key"));
    };
    Ok(SignArchiveResult::Ok {
        public_key_id: personal_key.public_key_id,
        signature: (minisign::sign(
            Some(&minisign::PublicKey::from_base64(
                &personal_key.public_key_value,
            )?),
            &private_key,
            compressed_archive,
            false,
            None,
            None,
        )?
        .to_string()),
    })
}
