use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io::IsTerminal};

use anyhow::{anyhow, bail, Context};
use flate2::{write::GzEncoder, Compression};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use tar::Builder;
use thiserror::Error;
use time::{self, OffsetDateTime};
use wasmer_config::package::{PackageIdent, MANIFEST_FILE_NAME};

use crate::publish::PublishWait;
use crate::WasmerConfig;
use crate::{package::builder::validate::ValidationPolicy, publish::SignArchiveResult};

const MIGRATIONS: &[(i32, &str)] = &[
    (0, include_str!("./sql/migrations/0000.sql")),
    (1, include_str!("./sql/migrations/0001.sql")),
    (2, include_str!("./sql/migrations/0002.sql")),
];

const CURRENT_DATA_VERSION: usize = MIGRATIONS.len();

/// An abstraction for the action of publishing a named or unnamed package.
#[derive(Debug)]
pub struct Publish {
    /// Registry to publish to
    pub registry: Option<String>,
    /// Run the publish logic without sending anything to the registry server
    pub dry_run: bool,
    /// Run the publish command without any output
    pub quiet: bool,
    /// Override the namespace of the package to upload
    pub package_namespace: Option<String>,
    /// Override the name of the package to upload
    pub package_name: Option<String>,
    /// Override the package version of the uploaded package in the wasmer.toml
    pub version: Option<semver::Version>,
    /// The auth token to use.
    pub token: String,
    /// Skip validation of the uploaded package
    pub no_validate: bool,
    /// Directory containing the `wasmer.toml` (defaults to current root dir)
    pub package_path: Option<String>,
    /// Wait for package to be available on the registry before exiting
    pub wait: PublishWait,
    /// Timeout (in seconds) for the publish query to the registry
    pub timeout: Duration,
}

#[derive(Debug, Error)]
enum PackageBuildError {
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
    /// Publish the package to the selected (or default) registry.
    pub async fn execute(&self) -> Result<Option<PackageIdent>, anyhow::Error> {
        let input_path = match self.package_path.as_ref() {
            Some(s) => std::env::current_dir()?.join(s),
            None => std::env::current_dir()?,
        };

        let manifest_path = match input_path.metadata() {
            Ok(metadata) => {
                if metadata.is_dir() {
                    let p = input_path.join("wasmer.toml");
                    if !p.is_file() {
                        bail!(
                            "directory does not contain a 'wasmer.toml' manifest - use 'wasmer init' to initialize a new packagae, or specify a valid package directory or manifest file instead. (path: {})",
                            input_path.display()
                        );
                    }

                    p
                } else if metadata.is_file() {
                    if input_path.extension().and_then(|x| x.to_str()) != Some("toml") {
                        bail!(
                            "The specified file path is not a .toml file: '{}'",
                            input_path.display()
                        );
                    }
                    input_path
                } else {
                    bail!("Invalid path specified: '{}'", input_path.display());
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                bail!("Specified path does not exist: '{}'", input_path.display());
            }
            Err(err) => {
                bail!("Could not read path '{}': {}", input_path.display(), err);
            }
        };

        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|e| anyhow::anyhow!("could not find manifest: {e}"))
            .with_context(|| anyhow::anyhow!("{}", manifest_path.display()))?;
        let mut manifest = wasmer_config::package::Manifest::parse(&manifest)?;

        let manifest_path_canon = manifest_path.canonicalize()?;
        let manifest_dir = manifest_path_canon
            .parent()
            .context("could not determine manifest parent directory")?
            .to_owned();

        if let Some(package_name) = self.package_name.as_ref() {
            if let Some(ref mut package) = manifest.package {
                package.name = package_name.clone();
            }
        }

        if let Some(version) = self.version.as_ref() {
            if let Some(ref mut package) = manifest.package {
                package.version = version.clone();
            }
        }

        let archive_dir = tempfile::TempDir::new()?;
        let archive_meta = construct_tar_gz(archive_dir.path(), &manifest, &manifest_path)?;

        let registry = match self.registry.as_deref() {
            Some(s) => crate::format_graphql(s),
            None => {
                let wasmer_dir = WasmerConfig::get_wasmer_dir()
                    .map_err(|e| anyhow::anyhow!("no wasmer dir: {e}"))?;
                let config = WasmerConfig::from_file(&wasmer_dir)
                    .map_err(|e| anyhow::anyhow!("could not load config {e}"))?;
                config.registry.get_current_registry()
            }
        };

        let mut policy = self.validation_policy();

        if !policy.skip_validation() {
            validate::validate_directory(
                &manifest,
                &registry,
                manifest_dir,
                &mut *policy,
                &self.token,
            )?;
        }

        let archive_path = &archive_meta.archive_path;
        let mut compressed_archive_reader = fs::File::open(archive_path)?;

        let maybe_signature_data = sign_compressed_archive(&mut compressed_archive_reader)?;
        let archived_data_size = archive_path.metadata()?.len();

        assert!(archive_path.exists());
        assert!(archive_path.is_file());

        if self.dry_run {
            // dry run: publish is done here
            println!("🚀 Package published successfully!");

            let path = archive_dir.into_path();
            eprintln!("Archive persisted at: {}", path.display());

            log::info!(
                "Publish succeeded, but package was not published because it was run in dry-run mode"
            );

            return Ok(None);
        }

        crate::publish::try_chunked_uploading(
            Some(registry),
            Some(self.token.clone()),
            &manifest.package,
            &archive_meta.manifest_toml,
            &archive_meta.license,
            &archive_meta.readme,
            &archive_meta
                .archive_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            archive_path,
            &maybe_signature_data,
            archived_data_size,
            self.quiet,
            self.wait,
            self.timeout,
            self.package_namespace.clone(),
        )
        .await
    }

    fn validation_policy(&self) -> Box<dyn ValidationPolicy> {
        if self.no_validate {
            Box::<validate::Skip>::default()
        } else if std::io::stdin().is_terminal() {
            Box::<validate::Interactive>::default()
        } else {
            Box::<validate::NonInteractive>::default()
        }
    }
}

struct ConstructedPackageArchive {
    manifest_toml: String,
    readme: Option<String>,
    license: Option<String>,
    archive_path: PathBuf,
}

fn construct_tar_gz(
    archive_dir: &Path,
    manifest: &wasmer_config::package::Manifest,
    manifest_path: &Path,
) -> Result<ConstructedPackageArchive, anyhow::Error> {
    // This is an assert instead of returned error because this is a programmer error.
    debug_assert!(manifest_path.is_file(), "manifest path is not a file");

    let manifest_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;

    let mut builder = Builder::new(Vec::new());
    builder.append_path_with_name(
        manifest_path,
        manifest_path
            .file_name()
            .map(|s| s.to_str().unwrap_or_default())
            .unwrap_or(MANIFEST_FILE_NAME),
    )?;

    let manifest_string = toml::to_string(&manifest)?;

    let modules = &manifest.modules;

    let readme = if let Some(ref package) = manifest.package {
        match package.readme.as_ref() {
            None => None,
            Some(s) => {
                let path =
                    append_path_to_tar_gz(&mut builder, manifest_dir, s).map_err(|(p, e)| {
                        PackageBuildError::ErrorBuildingPackage(format!("{}", p.display()), e)
                    })?;
                Some(std::fs::read_to_string(path)?)
            }
        }
    } else {
        None
    };

    let license = if let Some(ref package) = manifest.package {
        match package.license_file.as_ref() {
            None => None,
            Some(s) => {
                let path =
                    append_path_to_tar_gz(&mut builder, manifest_dir, s).map_err(|(p, e)| {
                        PackageBuildError::ErrorBuildingPackage(format!("{}", p.display()), e)
                    })?;
                Some(std::fs::read_to_string(path)?)
            }
        }
    } else {
        None
    };

    for module in modules {
        append_path_to_tar_gz(&mut builder, manifest_dir, &module.source).map_err(
            |(normalized_path, _)| PackageBuildError::SourceMustBeFile {
                module: module.name.clone(),
                path: normalized_path,
            },
        )?;

        if let Some(bindings) = &module.bindings {
            for path in bindings.referenced_files(manifest_dir)? {
                let relative_path = path.strip_prefix(manifest_dir).with_context(|| {
                    format!(
                        "\"{}\" should be inside \"{}\"",
                        path.display(),
                        manifest_dir.display(),
                    )
                })?;

                append_path_to_tar_gz(&mut builder, manifest_dir, relative_path).map_err(
                    |(normalized_path, _)| PackageBuildError::MissingBindings {
                        module: module.name.clone(),
                        path: normalized_path,
                    },
                )?;
            }
        }
    }

    // bundle the package filesystem
    for (_alias, path) in &manifest.fs {
        let normalized_path = normalize_path(manifest_dir, path);
        let path_metadata = normalized_path.metadata().map_err(|_| {
            PackageBuildError::MissingManifestFsPath(normalized_path.to_string_lossy().to_string())
        })?;
        if path_metadata.is_dir() {
            builder.append_dir_all(path, &normalized_path)
        } else {
            return Err(PackageBuildError::PackageFileSystemEntryMustBeDirectory(
                path.to_string_lossy().to_string(),
            )
            .into());
        }
        .map_err(|_| {
            PackageBuildError::MissingManifestFsPath(normalized_path.to_string_lossy().to_string())
        })?;
    }

    builder
        .finish()
        .map_err(|e| anyhow::anyhow!("failed to finish .tar.gz builder: {e}"))?;
    let tar_archive_data = builder.into_inner().expect("tar archive was not finalized");
    let archive_name = "package.tar.gz".to_string();
    fs::create_dir(archive_dir.join("wapm_package"))?;
    let archive_path = archive_dir.join("wapm_package").join(archive_name);
    let mut compressed_archive = fs::File::create(&archive_path).unwrap();
    let mut gz_enc = GzEncoder::new(&mut compressed_archive, Compression::best());

    gz_enc.write_all(&tar_archive_data).unwrap();
    let _compressed_archive = gz_enc.finish().unwrap();

    Ok(ConstructedPackageArchive {
        manifest_toml: manifest_string,
        archive_path,
        readme,
        license,
    })
}

fn append_path_to_tar_gz(
    builder: &mut tar::Builder<Vec<u8>>,
    base_path: &Path,
    target_path: &Path,
) -> Result<PathBuf, (PathBuf, io::Error)> {
    let normalized_path = normalize_path(base_path, target_path);
    normalized_path
        .metadata()
        .map_err(|e| (normalized_path.clone(), e))?;
    builder
        .append_path_with_name(&normalized_path, target_path)
        .map_err(|e| (normalized_path.clone(), e))?;
    Ok(normalized_path)
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
    let public_key = minisign::PublicKey::from_base64(&personal_key.public_key_value)?;
    let signature = minisign::sign(
        Some(&public_key),
        &private_key,
        compressed_archive,
        None,
        None,
    )?;
    Ok(SignArchiveResult::Ok {
        public_key_id: personal_key.public_key_id,
        signature: signature.to_string(),
    })
}

/// Opens an exclusive read/write connection to the database, creating it if it does not exist
pub fn open_db() -> anyhow::Result<Connection> {
    let wasmer_dir =
        WasmerConfig::get_wasmer_dir().map_err(|e| anyhow::anyhow!("no wasmer dir: {e}"))?;
    let db_path = WasmerConfig::get_database_file_path(&wasmer_dir);
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
        apply_migration(conn, data_version as i32)?;
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

    let migration_to_apply = MIGRATIONS
        .iter()
        .find_map(|(number, sql)| {
            if *number == migration_number {
                Some(sql)
            } else {
                None
            }
        })
        .ok_or({
            MigrationError::MigrationNumberDoesNotExist(
                migration_number,
                CURRENT_DATA_VERSION as i32,
            )
        })?;

    tx.execute_batch(migration_to_apply)
        .map_err(|e| MigrationError::TransactionFailed(migration_number, format!("{}", e)))?;

    tx.pragma_update(None, "user_version", migration_number + 1)
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
    /// The time at which the key was registered with wasmer
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

mod interfaces {
    use anyhow::anyhow;
    use rusqlite::{params, Connection, TransactionBehavior};

    pub const WASM_INTERFACE_EXISTENCE_CHECK: &str =
        include_str!("./sql/queries/wasm_interface_existence_check.sql");
    pub const INSERT_WASM_INTERFACE: &str = include_str!("./sql/queries/insert_interface.sql");
    pub const GET_WASM_INTERFACE: &str = include_str!("./sql/queries/get_interface.sql");

    pub fn interface_exists(
        conn: &mut Connection,
        interface_name: &str,
        version: &str,
    ) -> anyhow::Result<bool> {
        let mut stmt = conn.prepare(WASM_INTERFACE_EXISTENCE_CHECK)?;
        Ok(stmt.exists(params![interface_name, version])?)
    }

    pub fn load_interface_from_db(
        conn: &mut Connection,
        interface_name: &str,
        version: &str,
    ) -> anyhow::Result<wasmer_wasm_interface::Interface> {
        let mut stmt = conn.prepare(GET_WASM_INTERFACE)?;
        let interface_string: String =
            stmt.query_row(params![interface_name, version], |row| row.get(0))?;

        wasmer_wasm_interface::parser::parse_interface(&interface_string).map_err(|e| {
            anyhow!(
                "Failed to parse interface {} version {} in database: {}",
                interface_name,
                version,
                e
            )
        })
    }

    pub fn import_interface(
        conn: &mut Connection,
        interface_name: &str,
        version: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        // fail if we already have this interface
        {
            let mut key_check = conn.prepare(WASM_INTERFACE_EXISTENCE_CHECK)?;
            let result = key_check.exists(params![interface_name, version])?;

            if result {
                return Err(anyhow!(
                    "Interface {}, version {} already exists",
                    interface_name,
                    version
                ));
            }
        }

        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let time_string = get_current_time_in_format().expect("Could not get current time");

        log::debug!("Adding interface {:?} {:?}", interface_name, version);
        tx.execute(
            INSERT_WASM_INTERFACE,
            params![interface_name, version, time_string, content],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Gets the current time in our standard format
    pub fn get_current_time_in_format() -> Option<String> {
        use time::format_description::well_known::Rfc3339;
        let cur_time = time::OffsetDateTime::now_utc();
        cur_time.format(&Rfc3339).ok()
    }
}

mod validate {
    use anyhow::anyhow;
    use thiserror::Error;
    use wasmer_wasm_interface::{validate, Interface};

    use super::interfaces;
    use crate::{interface::InterfaceFromServer, QueryPackageError};
    use std::{
        fs,
        io::Read,
        ops::ControlFlow,
        path::{Path, PathBuf},
    };

    pub(crate) fn validate_directory(
        manifest: &wasmer_config::package::Manifest,
        registry: &str,
        pkg_path: PathBuf,
        callbacks: &mut dyn ValidationPolicy,
        auth_token: &str,
    ) -> anyhow::Result<()> {
        // validate as dir
        for module in manifest.modules.iter() {
            if let Err(e) = validate_module(module, registry, &pkg_path) {
                if callbacks.on_invalid_module(module, &e).is_break() {
                    return Err(e.into());
                }
            }
        }

        if would_change_package_privacy(manifest, registry, auth_token)?
            && callbacks.on_package_privacy_changed(manifest).is_break()
        {
            if let Some(package) = &manifest.package {
                if package.private {
                    return Err(ValidationError::WouldBecomePrivate.into());
                } else {
                    return Err(ValidationError::WouldBecomePublic.into());
                }
            }
        }

        log::debug!("package at path {:#?} validated", &pkg_path);

        Ok(())
    }

    /// Check if publishing this manifest would change the package's privacy.
    fn would_change_package_privacy(
        manifest: &wasmer_config::package::Manifest,
        registry: &str,
        auth_token: &str,
    ) -> Result<bool, ValidationError> {
        match &manifest.package {
            Some(pkg) => {
                let result =
                    crate::query_package_from_registry(registry, &pkg.name, None, Some(auth_token));

                match result {
                    Ok(package_version) => Ok(package_version.is_private != pkg.private),
                    Err(QueryPackageError::NoPackageFound { .. }) => {
                        // The package hasn't been published yet
                        Ok(false)
                    }
                    Err(e) => Err(e.into()),
                }
            }

            // This manifest refers to an unnamed package:
            // as of now, unnamed packages are private by default.
            None => Ok(false),
        }
    }

    fn validate_module(
        module: &wasmer_config::package::Module,
        registry: &str,
        pkg_path: &Path,
    ) -> Result<(), ValidationError> {
        let source_path = if module.source.is_relative() {
            pkg_path.join(&module.source)
        } else {
            module.source.clone()
        };
        let source_path_string = source_path.to_string_lossy().to_string();
        let mut wasm_file =
            fs::File::open(&source_path).map_err(|_| ValidationError::MissingFile {
                file: source_path_string.clone(),
            })?;
        let mut wasm_buffer = Vec::new();
        wasm_file
            .read_to_end(&mut wasm_buffer)
            .map_err(|err| ValidationError::MiscCannotRead {
                file: source_path_string.clone(),
                error: format!("{}", err),
            })?;

        if let Some(bindings) = &module.bindings {
            validate_bindings(bindings, pkg_path)?;
        }

        // hack, short circuit if no interface for now
        if module.interfaces.is_none() {
            return validate_wasm_and_report_errors_old(&wasm_buffer[..], source_path_string);
        }

        let mut conn = super::open_db().map_err(ValidationError::UpdatingInterfaces)?;
        let mut interface: Interface = Default::default();
        for (interface_name, interface_version) in
            module.interfaces.clone().unwrap_or_default().into_iter()
        {
            add_module_interface(
                &mut conn,
                interface_name,
                interface_version,
                registry,
                &mut interface,
            )
            .map_err(ValidationError::UpdatingInterfaces)?;
        }
        validate::validate_wasm_and_report_errors(&wasm_buffer, &interface).map_err(|e| {
            ValidationError::InvalidWasm {
                file: source_path_string,
                error: format!("{:?}", e),
            }
        })?;

        Ok(())
    }

    fn add_module_interface(
        conn: &mut rusqlite::Connection,
        interface_name: String,
        interface_version: String,
        registry: &str,
        interface: &mut Interface,
    ) -> anyhow::Result<()> {
        if !interfaces::interface_exists(conn, &interface_name, &interface_version)? {
            // download interface and store it if we don't have it locally
            let interface_data_from_server = InterfaceFromServer::get(
                registry,
                interface_name.clone(),
                interface_version.clone(),
            )?;
            interfaces::import_interface(
                conn,
                &interface_name,
                &interface_version,
                &interface_data_from_server.content,
            )?;
        }
        let sub_interface =
            interfaces::load_interface_from_db(conn, &interface_name, &interface_version)?;
        *interface = interface
            .merge(sub_interface)
            .map_err(|e| anyhow!("Failed to merge interface {}: {}", &interface_name, e))?;
        Ok(())
    }

    fn validate_bindings(
        bindings: &wasmer_config::package::Bindings,
        base_directory_path: &Path,
    ) -> Result<(), ValidationError> {
        // Note: checking for referenced files will make sure they all exist.
        let _ = bindings.referenced_files(base_directory_path)?;

        Ok(())
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ValidationError {
        #[error("WASM file \"{file}\" detected as invalid because {error}")]
        InvalidWasm { file: String, error: String },
        #[error("Could not find file {file}")]
        MissingFile { file: String },
        #[error("Failed to read file {file}; {error}")]
        MiscCannotRead { file: String, error: String },
        #[error(transparent)]
        Imports(#[from] wasmer_config::package::ImportsError),
        #[error("Unable to update the interfaces database")]
        UpdatingInterfaces(#[source] anyhow::Error),
        #[error("Aborting because publishing the package would make it public")]
        WouldBecomePublic,
        #[error("Aborting because publishing the package would make it private")]
        WouldBecomePrivate,
        #[error("Unable to look up package information")]
        Registry(#[from] QueryPackageError),
    }

    // legacy function, validates wasm.  TODO: clean up
    pub fn validate_wasm_and_report_errors_old(
        wasm: &[u8],
        file_name: String,
    ) -> Result<(), ValidationError> {
        let mut val = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures {
            threads: true,
            reference_types: true,
            simd: true,
            bulk_memory: true,
            multi_value: true,
            ..Default::default()
        });

        val.validate_all(wasm)
            .map_err(|e| ValidationError::InvalidWasm {
                file: file_name.clone(),
                error: format!("{}", e),
            })?;

        Ok(())
    }

    /// How should validation be treated by the publishing process?
    pub(crate) trait ValidationPolicy: Send + Sync {
        /// Should validation be skipped entirely?
        fn skip_validation(&mut self) -> bool;

        /// How should publishing proceed when a module is invalid?
        fn on_invalid_module(
            &mut self,
            module: &wasmer_config::package::Module,
            error: &ValidationError,
        ) -> ControlFlow<(), ()>;

        /// How should publishing proceed when it might change a package's
        /// privacy? (i.e. by making a private package publicly available).
        fn on_package_privacy_changed(
            &mut self,
            manifest: &wasmer_config::package::Manifest,
        ) -> ControlFlow<(), ()>;
    }

    #[derive(Debug, Default, Copy, Clone, PartialEq)]
    pub(crate) struct Skip;

    impl ValidationPolicy for Skip {
        fn skip_validation(&mut self) -> bool {
            true
        }

        fn on_invalid_module(
            &mut self,
            _module: &wasmer_config::package::Module,
            _error: &ValidationError,
        ) -> ControlFlow<(), ()> {
            unreachable!()
        }

        fn on_package_privacy_changed(
            &mut self,
            _manifest: &wasmer_config::package::Manifest,
        ) -> ControlFlow<(), ()> {
            unreachable!()
        }
    }

    #[derive(Debug, Default, Copy, Clone, PartialEq)]
    pub(crate) struct Interactive;

    impl ValidationPolicy for Interactive {
        fn skip_validation(&mut self) -> bool {
            false
        }

        fn on_invalid_module(
            &mut self,
            module: &wasmer_config::package::Module,
            error: &ValidationError,
        ) -> ControlFlow<(), ()> {
            let module_name = &module.name;
            let prompt =
                format!("Validation error with the \"{module_name}\" module: {error}. Would you like to continue?");

            match dialoguer::Confirm::new()
                .with_prompt(prompt)
                .default(false)
                .interact()
            {
                Ok(true) => ControlFlow::Continue(()),
                Ok(false) => ControlFlow::Break(()),
                Err(e) => {
                    tracing::error!(
                        error = &e as &dyn std::error::Error,
                        "Unable to check whether the user wants to change the package's privacy",
                    );
                    ControlFlow::Break(())
                }
            }
        }

        fn on_package_privacy_changed(
            &mut self,
            manifest: &wasmer_config::package::Manifest,
        ) -> ControlFlow<(), ()> {
            if let Some(pkg) = &manifest.package {
                let privacy = if pkg.private { "private" } else { "public" };
                let prompt =
                    format!("This will make the package {privacy}. Would you like to continue?");

                match dialoguer::Confirm::new()
                    .with_prompt(prompt)
                    .default(false)
                    .interact()
                {
                    Ok(true) => ControlFlow::Continue(()),
                    Ok(false) => ControlFlow::Break(()),
                    Err(e) => {
                        tracing::error!(
                        error = &e as &dyn std::error::Error,
                        "Unable to check whether the user wants to change the package's privacy",
                    );
                        ControlFlow::Break(())
                    }
                }
            } else {
                ControlFlow::Continue(())
            }
        }
    }

    #[derive(Debug, Default, Copy, Clone, PartialEq)]
    pub(crate) struct NonInteractive;

    impl ValidationPolicy for NonInteractive {
        fn skip_validation(&mut self) -> bool {
            false
        }

        fn on_invalid_module(
            &mut self,
            _module: &wasmer_config::package::Module,
            _error: &ValidationError,
        ) -> ControlFlow<(), ()> {
            ControlFlow::Break(())
        }

        fn on_package_privacy_changed(
            &mut self,
            _manifest: &wasmer_config::package::Manifest,
        ) -> ControlFlow<(), ()> {
            ControlFlow::Break(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn test_construct_package_tar_gz() {
        let manifest_str = r#"[package]
name = "wasmer-tests/wcgi-always-panic"
version = "0.6.0"
description = "wasmer-tests/wcgi-always-panic website"

[[module]]
name = "wcgi-always-panic"
source = "module.wasm"
abi = "wasi"

[[command]]
name = "wcgi"
module = "wcgi-always-panic"
runner = "https://webc.org/runner/wcgi"
"#;

        let archive_dir = tempfile::tempdir().unwrap();

        let manifest_dir = tempfile::tempdir().unwrap();

        let mp = manifest_dir.path();
        let manifest_path = mp.join("wasmer.toml");

        std::fs::write(&manifest_path, manifest_str).unwrap();
        std::fs::write(mp.join("module.wasm"), "()").unwrap();

        let manifest = wasmer_config::package::Manifest::parse(manifest_str).unwrap();

        let meta = construct_tar_gz(archive_dir.path(), &manifest, &manifest_path).unwrap();

        let mut data = std::io::Cursor::new(std::fs::read(meta.archive_path).unwrap());

        let gz = flate2::read::GzDecoder::new(&mut data);
        let mut archive = tar::Archive::new(gz);

        let map = archive
            .entries()
            .unwrap()
            .map(|res| {
                let mut entry = res.unwrap();
                let name = entry.path().unwrap().to_str().unwrap().to_string();
                let mut contents = String::new();
                entry.read_to_string(&mut contents).unwrap();
                eprintln!("{name}:\n{contents}\n\n");
                (name, contents)
            })
            .collect::<std::collections::HashMap<String, String>>();

        let expected: std::collections::HashMap<String, String> = [
            ("wapm.toml".to_string(), manifest_str.to_string()),
            ("module.wasm".to_string(), "()".to_string()),
        ]
        .into_iter()
        .collect();

        pretty_assertions::assert_eq!(map, expected);
    }

    #[test]
    fn test_construct_wai_package_tar_gz() {
        let manifest_str = r#"[package]
name = "wasmer/crumsort-wasm"
version = "0.2.4"
description = "Crumsort from Google made for WASM"

[[module]]
name = "crumsort-wasm"
source = "crumsort_wasm.wasm"

[module.bindings]
wai-version = "0.2.0"
exports = "crum-sort.wai"
"#;

        let archive_dir = tempfile::tempdir().unwrap();

        let manifest_dir = tempfile::tempdir().unwrap();

        let mp = manifest_dir.path();
        let manifest_path = mp.join("wasmer.toml");

        std::fs::write(&manifest_path, manifest_str).unwrap();
        std::fs::write(mp.join("crumsort_wasm.wasm"), "()").unwrap();
        std::fs::write(mp.join("crum-sort.wai"), "/// crum-sort.wai").unwrap();

        let manifest = wasmer_config::package::Manifest::parse(manifest_str).unwrap();
        let meta = construct_tar_gz(archive_dir.path(), &manifest, &manifest_path).unwrap();

        let mut data = std::io::Cursor::new(std::fs::read(meta.archive_path).unwrap());

        let gz = flate2::read::GzDecoder::new(&mut data);
        let mut archive = tar::Archive::new(gz);

        let map = archive
            .entries()
            .unwrap()
            .map(|res| {
                let mut entry = res.unwrap();
                let name = entry.path().unwrap().to_str().unwrap().to_string();
                let mut contents = String::new();
                entry.read_to_string(&mut contents).unwrap();
                eprintln!("{name}:\n{contents}\n\n");
                (name, contents)
            })
            .collect::<std::collections::HashMap<String, String>>();

        let expected: std::collections::HashMap<String, String> = [
            ("wapm.toml".to_string(), manifest_str.to_string()),
            ("crum-sort.wai".to_string(), "/// crum-sort.wai".to_string()),
            ("crumsort_wasm.wasm".to_string(), "()".to_string()),
        ]
        .into_iter()
        .collect();

        pretty_assertions::assert_eq!(map, expected);
    }
}
