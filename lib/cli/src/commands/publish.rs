use anyhow::Context;
use clap::Parser;
use flate2::{write::GzEncoder, Compression};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tar::Builder;
use thiserror::Error;
use time::{self, OffsetDateTime};
use wasmer_registry::publish::SignArchiveResult;
use wasmer_registry::PartialWapmConfig;

const CURRENT_DATA_VERSION: i32 = 3;

/// CLI options for the `wasmer publish` command
#[derive(Debug, Parser)]
pub struct Publish {
    /// Registry to publish to
    #[clap(long)]
    pub registry: Option<String>,
    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,
    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,
    /// Override the namespace of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub namespace: Option<String>,
    /// Override the token (by default, it will use the current logged in user)
    #[clap(long)]
    pub token: Option<String>,
    /// Directory containing the `wasmer.toml` (defaults to current root dir)
    #[clap(name = "PACKAGE_PATH")]
    pub package_path: Option<String>,
}

#[derive(Debug, Error)]
enum PublishError {
    #[error("Cannot publish without a module.")]
    NoModule,
    #[error("Unable to publish the \"{module}\" module because \"{}\" is not a file", path.display())]
    SourceMustBeFile { module: String, path: PathBuf },
    #[error("Unable to load the bindings for \"{module}\" because \"{}\" doesn't exist", path.display())]
    MissingBindings { module: String, path: PathBuf },
    #[error("Error building package when parsing module \"{0}\": {1}.")]
    ErrorBuildingPackage(String, io::Error),
    #[error(
        "Path \"{0}\", specified in the manifest as part of the package file system does not exist.",
    )]
    MissingManifestFsPath(String),
    #[error("When processing the package filesystem, found path \"{0}\" which is not a directory")]
    PackageFileSystemEntryMustBeDirectory(String),
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let mut builder = Builder::new(Vec::new());

        let cwd = match self.package_path.as_ref() {
            Some(s) => std::env::current_dir()?.join(s),
            None => std::env::current_dir()?,
        };

        // TODO: implement validation
        // validate::validate_directory(cwd.clone())?;

        let manifest_path_buf = cwd.join("wasmer.toml");
        let manifest = std::fs::read_to_string(&manifest_path_buf)
            .map_err(|e| anyhow::anyhow!("could not find manifest: {e}"))
            .with_context(|| anyhow::anyhow!("{}", manifest_path_buf.display()))?;
        let mut manifest = wapm_toml::Manifest::parse(&manifest)?;
        manifest.base_directory_path = cwd.clone();

        builder.append_path_with_name(&manifest_path_buf, "wapm.toml")?;

        let package = &manifest.package;
        let modules = manifest.module.as_ref().ok_or(PublishError::NoModule)?;
        let manifest_string = toml::to_string(&manifest)?;

        let readme = package.readme.as_ref().and_then(|readme_path| {
            let normalized_path = normalize_path(&manifest.base_directory_path, readme_path);
            if builder
                .append_path_with_name(&normalized_path, readme_path)
                .is_err()
            {
                // TODO: Maybe do something here
            }
            fs::read_to_string(normalized_path).ok()
        });

        let license_file = package.license_file.as_ref().and_then(|license_file_path| {
            let normalized_path = normalize_path(&manifest.base_directory_path, license_file_path);
            if builder
                .append_path_with_name(&normalized_path, license_file_path)
                .is_err()
            {
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
                .append_path_with_name(&normalized_path, &module.source)
                .map_err(|e| {
                    PublishError::ErrorBuildingPackage(format!("{}", normalized_path.display()), e)
                })?;

            if let Some(bindings) = &module.bindings {
                for path in bindings.referenced_files(&manifest.base_directory_path)? {
                    let normalized_path = normalize_path(&manifest.base_directory_path, &path);
                    normalized_path
                        .metadata()
                        .map_err(|_| PublishError::MissingBindings {
                            module: module.name.clone(),
                            path: normalized_path.clone(),
                        })?;
                    builder
                        .append_path_with_name(&normalized_path, &module.source)
                        .map_err(|e| PublishError::ErrorBuildingPackage(module.name.clone(), e))?;
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
        let tar_archive_data = builder.into_inner().map_err(|_| PublishError::NoModule)?;
        let archive_name = "package.tar.gz".to_string();
        let archive_dir = tempfile::TempDir::new()?;
        let archive_dir_path: &std::path::Path = archive_dir.as_ref();
        fs::create_dir(archive_dir_path.join("wapm_package"))?;
        let archive_path = archive_dir_path.join("wapm_package").join(&archive_name);
        let mut compressed_archive = fs::File::create(&archive_path).unwrap();
        let mut gz_enc = GzEncoder::new(&mut compressed_archive, Compression::best());

        gz_enc.write_all(&tar_archive_data).unwrap();
        let _compressed_archive = gz_enc.finish().unwrap();
        let mut compressed_archive_reader = fs::File::open(&archive_path)?;

        let maybe_signature_data = sign_compressed_archive(&mut compressed_archive_reader)?;
        let archived_data_size = archive_path.metadata()?.len();

        assert!(archive_path.exists());
        assert!(archive_path.is_file());

        if self.dry_run {
            // dry run: publish is done here

            println!(
                "Successfully published package `{}@{}`",
                package.name, package.version
            );

            log::info!(
                "Publish succeeded, but package was not published because it was run in dry-run mode"
            );

            return Ok(());
        }

        // See if the user is logged in and has authorization to publish the package
        // under the correct namespace before trying to upload.
        let (registry, username) =
            wasmer_registry::whoami(self.registry.as_deref(), self.token.as_deref()).with_context(
                || {
                    anyhow::anyhow!(
                        "could not find username / registry for registry = {:?}, token = {}",
                        self.registry,
                        self.token.as_deref().unwrap_or_default()
                    )
                },
            )?;

        let registry_present =
            wasmer_registry::test_if_registry_present(&registry).unwrap_or(false);
        if !registry_present {
            return Err(anyhow::anyhow!(
                "registry {} is currently unavailable",
                registry
            ));
        }

        let namespace = self
            .namespace
            .as_deref()
            .or_else(|| package.name.split('/').next())
            .unwrap_or("")
            .to_string();
        if username != namespace {
            return Err(anyhow::anyhow!("trying to publish package under the namespace {namespace:?}, but logged in as user {username:?}"));
        }

        wasmer_registry::publish::try_chunked_uploading(
            self.registry.clone(),
            self.token.clone(),
            package,
            &manifest_string,
            &license_file,
            &readme,
            &archive_name,
            &archive_path,
            &maybe_signature_data,
            archived_data_size,
            self.quiet,
        )
        .map_err(on_error)
    }
}

fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}

fn normalize_path(cwd: &Path, path: &Path) -> PathBuf {
    let mut out = PathBuf::from(cwd);
    let mut components = path.components();
    if path.is_absolute() {
        log::warn!(
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

/// Takes the package archive as a File and attempts to sign it using the active key
/// returns the public key id used to sign it and the signature string itself
pub fn sign_compressed_archive(
    compressed_archive: &mut fs::File,
) -> anyhow::Result<SignArchiveResult> {
    let key_db = open_db()?;
    let personal_key = if let Ok(v) = get_active_personal_key(&key_db) {
        v
    } else {
        return Ok(SignArchiveResult::NoKeyRegistered);
    };
    let password = rpassword::prompt_password(format!(
        "Please enter your password for the key pair {}:",
        &personal_key.public_key_id
    ))
    .ok();
    let private_key = if let Some(priv_key_location) = personal_key.private_key_location {
        match minisign::SecretKey::from_file(&priv_key_location, password) {
            Ok(priv_key_data) => priv_key_data,
            Err(e) => {
                log::error!(
                    "Could not read private key from location {}: {}",
                    priv_key_location,
                    e
                );
                return Err(e.into());
            }
        }
    } else {
        // TODO: add more info about why this might have happened and what the user can do about it
        log::warn!("Active key does not have a private key location registered with it!");
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
            None,
            None,
        )?
        .to_string()),
    })
}

/// Opens an exclusive read/write connection to the database, creating it if it does not exist
pub fn open_db() -> anyhow::Result<Connection> {
    let db_path =
        PartialWapmConfig::get_database_file_path().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;

    apply_migrations(&mut conn)?;
    Ok(conn)
}

/// Applies migrations to the database
pub fn apply_migrations(conn: &mut Connection) -> anyhow::Result<()> {
    let user_version = conn.pragma_query_value(None, "user_version", |val| val.get(0))?;
    for data_version in user_version..CURRENT_DATA_VERSION {
        log::debug!("Applying migration {}", data_version);
        apply_migration(conn, data_version)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
enum MigrationError {
    #[error(
        "Critical internal error: the data version {0} is not handleded; current data version: {1}"
    )]
    MigrationNumberDoesNotExist(i32, i32),
    #[error("Critical internal error: failed to commit trasaction migrating to data version {0}")]
    CommitFailed(i32),
    #[error("Critical internal error: transaction failed on migration number {0}: {1}")]
    TransactionFailed(i32, String),
}

/// Applies migrations to the database and updates the `user_version` pragma.
/// Every migration must leave the database in a valid state.
fn apply_migration(conn: &mut Connection, migration_number: i32) -> Result<(), MigrationError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| MigrationError::TransactionFailed(migration_number, format!("{}", e)))?;
    match migration_number {
        0 => {
            tx.execute_batch(include_str!("../../sql/migrations/0000.sql"))
                .map_err(|e| {
                    MigrationError::TransactionFailed(migration_number, format!("{}", e))
                })?;
        }
        1 => {
            tx.execute_batch(include_str!("../../sql/migrations/0001.sql"))
                .map_err(|e| {
                    MigrationError::TransactionFailed(migration_number, format!("{}", e))
                })?;
        }
        2 => {
            tx.execute_batch(include_str!("../../sql/migrations/0002.sql"))
                .map_err(|e| {
                    MigrationError::TransactionFailed(migration_number, format!("{}", e))
                })?;
        }
        _ => {
            return Err(MigrationError::MigrationNumberDoesNotExist(
                migration_number,
                CURRENT_DATA_VERSION,
            ));
        }
    }
    tx.pragma_update(None, "user_version", &(migration_number + 1))
        .map_err(|e| MigrationError::TransactionFailed(migration_number, format!("{}", e)))?;
    tx.commit()
        .map_err(|_| MigrationError::CommitFailed(migration_number))
}

/// Information about one of the user's keys
#[derive(Debug)]
pub struct PersonalKey {
    /// Flag saying if the key will be used (there can only be one active key at a time)
    pub active: bool,
    /// The public key's tag. Used to identify the key pair
    pub public_key_id: String,
    /// The raw value of the public key in base64
    pub public_key_value: String,
    /// The location in the file system of the private key
    pub private_key_location: Option<String>,
    /// The type of private/public key this is
    pub key_type_identifier: String,
    /// The time at which the key was registered with wapm
    pub date_created: OffsetDateTime,
}

fn get_active_personal_key(conn: &Connection) -> anyhow::Result<PersonalKey> {
    let mut stmt = conn.prepare(
        "SELECT active, public_key_value, private_key_location, date_added, key_type_identifier, public_key_id FROM personal_keys 
         where active = 1",
    )?;

    let result = stmt
        .query_map(params![], |row| {
            Ok(PersonalKey {
                active: row.get(0)?,
                public_key_value: row.get(1)?,
                private_key_location: row.get(2)?,
                date_created: {
                    use time::format_description::well_known::Rfc3339;
                    let time_str: String = row.get(3)?;
                    OffsetDateTime::parse(&time_str, &Rfc3339)
                        .unwrap_or_else(|_| panic!("Failed to parse time string {}", &time_str))
                },
                key_type_identifier: row.get(4)?,
                public_key_id: row.get(5)?,
            })
        })?
        .next();

    if let Some(res) = result {
        Ok(res?)
    } else {
        Err(anyhow!("No active key found"))
    }
}
