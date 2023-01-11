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
use wasmer_registry::{WasmerConfig, PACKAGE_TOML_FALLBACK_NAME};

const MIGRATIONS: &[(i32, &str)] = &[
    (0, include_str!("../../sql/migrations/0000.sql")),
    (1, include_str!("../../sql/migrations/0001.sql")),
    (2, include_str!("../../sql/migrations/0002.sql")),
];

const CURRENT_DATA_VERSION: usize = MIGRATIONS.len();

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
    /// Override the package of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub package_name: Option<String>,
    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub version: Option<semver::Version>,
    /// Override the token (by default, it will use the current logged in user)
    #[clap(long)]
    pub token: Option<String>,
    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,
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

        let manifest_path_buf = cwd.join("wasmer.toml");
        let manifest = std::fs::read_to_string(&manifest_path_buf)
            .map_err(|e| anyhow::anyhow!("could not find manifest: {e}"))
            .with_context(|| anyhow::anyhow!("{}", manifest_path_buf.display()))?;
        let mut manifest = wasmer_toml::Manifest::parse(&manifest)?;
        manifest.base_directory_path = cwd.clone();

        if let Some(package_name) = self.package_name.as_ref() {
            manifest.package.name = package_name.to_string();
        }

        if let Some(version) = self.version.as_ref() {
            manifest.package.version = version.clone();
        }

        let registry = match self.registry.as_deref() {
            Some(s) => wasmer_registry::format_graphql(s),
            None => {
                let wasmer_dir = WasmerConfig::get_wasmer_dir()
                    .map_err(|e| anyhow::anyhow!("no wasmer dir: {e}"))?;
                let config = WasmerConfig::from_file(&wasmer_dir)
                    .map_err(|e| anyhow::anyhow!("could not load config {e}"))?;
                config.registry.get_current_registry()
            }
        };

        if !self.no_validate {
            validate::validate_directory(&manifest, &registry, cwd.clone())?;
        }

        builder.append_path_with_name(&manifest_path_buf, PACKAGE_TOML_FALLBACK_NAME)?;

        let manifest_string = toml::to_string(&manifest)?;

        let (readme, license) = construct_tar_gz(&mut builder, &manifest, &cwd)?;

        builder
            .finish()
            .map_err(|e| anyhow::anyhow!("failed to finish .tar.gz builder: {e}"))?;
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
                manifest.package.name, manifest.package.version
            );

            log::info!(
                "Publish succeeded, but package was not published because it was run in dry-run mode"
            );

            return Ok(());
        }

        wasmer_registry::publish::try_chunked_uploading(
            Some(registry),
            self.token.clone(),
            &manifest.package,
            &manifest_string,
            &license,
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

fn construct_tar_gz(
    builder: &mut tar::Builder<Vec<u8>>,
    manifest: &wasmer_toml::Manifest,
    cwd: &Path,
) -> Result<(Option<String>, Option<String>), anyhow::Error> {
    let package = &manifest.package;
    let modules = manifest.module.as_ref().ok_or(PublishError::NoModule)?;

    let readme = match package.readme.as_ref() {
        None => None,
        Some(s) => {
            let path = append_path_to_tar_gz(builder, &manifest.base_directory_path, s).map_err(
                |(p, e)| PublishError::ErrorBuildingPackage(format!("{}", p.display()), e),
            )?;
            fs::read_to_string(path).ok()
        }
    };

    let license_file = match package.license_file.as_ref() {
        None => None,
        Some(s) => {
            let path = append_path_to_tar_gz(builder, &manifest.base_directory_path, s).map_err(
                |(p, e)| PublishError::ErrorBuildingPackage(format!("{}", p.display()), e),
            )?;
            fs::read_to_string(path).ok()
        }
    };

    for module in modules {
        append_path_to_tar_gz(builder, &manifest.base_directory_path, &module.source).map_err(
            |(normalized_path, _)| PublishError::SourceMustBeFile {
                module: module.name.clone(),
                path: normalized_path,
            },
        )?;

        if let Some(bindings) = &module.bindings {
            for path in bindings.referenced_files(&manifest.base_directory_path)? {
                append_path_to_tar_gz(builder, &manifest.base_directory_path, &path).map_err(
                    |(normalized_path, _)| PublishError::MissingBindings {
                        module: module.name.clone(),
                        path: normalized_path,
                    },
                )?;
            }
        }
    }

    // bundle the package filesystem
    let default = std::collections::HashMap::default();
    for (_alias, path) in manifest.fs.as_ref().unwrap_or(&default).iter() {
        let normalized_path = normalize_path(cwd, path);
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

    Ok((readme, license_file))
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
        .append_path_with_name(&normalized_path, &target_path)
        .map_err(|e| (normalized_path.clone(), e))?;
    Ok(normalized_path)
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

mod interfaces {

    use rusqlite::{params, Connection, TransactionBehavior};

    pub const WASM_INTERFACE_EXISTENCE_CHECK: &str =
        include_str!("./sql/wasm_interface_existence_check.sql");
    pub const INSERT_WASM_INTERFACE: &str = include_str!("./sql/insert_interface.sql");
    pub const GET_WASM_INTERFACE: &str = include_str!("./sql/get_interface.sql");

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
    use super::interfaces;
    use std::{
        fs,
        io::Read,
        path::{Path, PathBuf},
    };
    use thiserror::Error;
    use wasmer_registry::interface::InterfaceFromServer;
    use wasmer_wasm_interface::{validate, Interface};

    pub fn validate_directory(
        manifest: &wasmer_toml::Manifest,
        registry: &str,
        pkg_path: PathBuf,
    ) -> anyhow::Result<()> {
        // validate as dir
        if let Some(modules) = manifest.module.as_ref() {
            for module in modules.iter() {
                let source_path = if module.source.is_relative() {
                    manifest.base_directory_path.join(&module.source)
                } else {
                    module.source.clone()
                };
                let source_path_string = source_path.to_string_lossy().to_string();
                let mut wasm_file =
                    fs::File::open(&source_path).map_err(|_| ValidationError::MissingFile {
                        file: source_path_string.clone(),
                    })?;
                let mut wasm_buffer = Vec::new();
                wasm_file.read_to_end(&mut wasm_buffer).map_err(|err| {
                    ValidationError::MiscCannotRead {
                        file: source_path_string.clone(),
                        error: format!("{}", err),
                    }
                })?;

                if let Some(bindings) = &module.bindings {
                    validate_bindings(bindings, &manifest.base_directory_path)?;
                }

                // hack, short circuit if no interface for now
                if module.interfaces.is_none() {
                    return validate_wasm_and_report_errors_old(
                        &wasm_buffer[..],
                        source_path_string,
                    );
                }

                let mut conn = super::open_db()?;
                let mut interface: Interface = Default::default();
                for (interface_name, interface_version) in
                    module.interfaces.clone().unwrap_or_default().into_iter()
                {
                    if !interfaces::interface_exists(
                        &mut conn,
                        &interface_name,
                        &interface_version,
                    )? {
                        // download interface and store it if we don't have it locally
                        let interface_data_from_server = InterfaceFromServer::get(
                            registry,
                            interface_name.clone(),
                            interface_version.clone(),
                        )?;
                        interfaces::import_interface(
                            &mut conn,
                            &interface_name,
                            &interface_version,
                            &interface_data_from_server.content,
                        )?;
                    }
                    let sub_interface = interfaces::load_interface_from_db(
                        &mut conn,
                        &interface_name,
                        &interface_version,
                    )?;
                    interface = interface.merge(sub_interface).map_err(|e| {
                        anyhow!("Failed to merge interface {}: {}", &interface_name, e)
                    })?;
                }
                validate::validate_wasm_and_report_errors(&wasm_buffer, &interface).map_err(
                    |e| ValidationError::InvalidWasm {
                        file: source_path_string,
                        error: format!("{:?}", e),
                    },
                )?;
            }
        }
        log::debug!("package at path {:#?} validated", &pkg_path);

        Ok(())
    }

    fn validate_bindings(
        bindings: &wasmer_toml::Bindings,
        base_directory_path: &Path,
    ) -> Result<(), ValidationError> {
        // Note: checking for referenced files will make sure they all exist.
        let _ = bindings.referenced_files(base_directory_path)?;

        Ok(())
    }

    #[derive(Debug, Error)]
    pub enum ValidationError {
        #[error("WASM file \"{file}\" detected as invalid because {error}")]
        InvalidWasm { file: String, error: String },
        #[error("Could not find file {file}")]
        MissingFile { file: String },
        #[error("Failed to read file {file}; {error}")]
        MiscCannotRead { file: String, error: String },
        #[error(transparent)]
        Imports(#[from] wasmer_toml::ImportsError),
    }

    // legacy function, validates wasm.  TODO: clean up
    pub fn validate_wasm_and_report_errors_old(
        wasm: &[u8],
        file_name: String,
    ) -> anyhow::Result<()> {
        use wasmparser::WasmDecoder;
        let mut parser = wasmparser::ValidatingParser::new(
            wasm,
            Some(wasmparser::ValidatingParserConfig {
                operator_config: wasmparser::OperatorValidatorConfig {
                    enable_threads: true,
                    enable_reference_types: true,
                    enable_simd: true,
                    enable_bulk_memory: true,
                    enable_multi_value: true,
                },
            }),
        );
        loop {
            let state = parser.read();
            match state {
                wasmparser::ParserState::EndWasm => return Ok(()),
                wasmparser::ParserState::Error(e) => {
                    return Err(ValidationError::InvalidWasm {
                        file: file_name,
                        error: format!("{}", e),
                    }
                    .into());
                }
                _ => {}
            }
        }
    }
}
