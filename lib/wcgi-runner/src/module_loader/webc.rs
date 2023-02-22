use bytes::Bytes;
use wcgi_host::CgiDialect;
use webc::{metadata::Manifest, v1::ParseOptions, v2::read::OwnedReader, Version};

use crate::{
    annotations::{WasiCommandAnnotation, WcgiAnnotation},
    errors::WebcLoadError,
    module_loader::{ModuleLoader, ModuleLoaderContext},
    Error,
};

use super::LoadedModule;

pub(crate) struct WebcLoader {
    command: String,
    atom: Bytes,
    dialect: CgiDialect,
}

impl WebcLoader {
    /// Create a new [`WebcLoader`] which uses the WEBC file's default
    /// entrypoint.
    pub fn new(webc: impl Into<Bytes>) -> Result<Self, WebcLoadError> {
        WebcLoader::new_with_options(&WebcOptions::default(), webc)
    }

    pub fn new_with_options(
        options: &WebcOptions<'_>,
        webc: impl Into<Bytes>,
    ) -> Result<Self, WebcLoadError> {
        let webc = webc.into();
        match webc::detect(webc.as_ref())? {
            Version::V1 => WebcLoader::v1(options, webc),
            Version::V2 => WebcLoader::v2(options, webc),
            other => Err(WebcLoadError::UnsupportedVersion(other)),
        }
    }

    fn v1(options: &WebcOptions<'_>, webc: Bytes) -> Result<Self, WebcLoadError> {
        let parse_options = ParseOptions::default();
        let webc = webc::v1::WebC::parse(&webc, &parse_options)?;

        let (command, atom, dialect) = get_atom_and_dialect(&webc.manifest, options, |name| {
            let pkg = webc.get_package_name();
            webc.get_atom(&pkg, name).ok().map(|b| b.to_vec().into())
        })?;

        Ok(WebcLoader {
            command,
            atom,
            dialect,
        })
    }

    fn v2(options: &WebcOptions<'_>, webc: Bytes) -> Result<Self, WebcLoadError> {
        let webc = OwnedReader::parse(webc)?;
        let (command, atom, dialect) = get_atom_and_dialect(webc.manifest(), options, |name| {
            webc.get_atom(name).cloned()
        })?;

        Ok(WebcLoader {
            command,
            atom,
            dialect,
        })
    }
}

#[async_trait::async_trait]
impl ModuleLoader for WebcLoader {
    async fn load(&self, ctx: ModuleLoaderContext<'_>) -> Result<LoadedModule, Error> {
        Ok(LoadedModule {
            module: ctx.compile_wasm(self.atom.clone()).await?,
            dialect: self.dialect,
            program: self.command.clone(),
        })
    }
}

fn get_atom_and_dialect(
    manifest: &Manifest,
    options: &WebcOptions<'_>,
    get_atom: impl FnOnce(&str) -> Option<Bytes>,
) -> Result<(String, Bytes, CgiDialect), WebcLoadError> {
    let command = options
        .command
        .as_str()
        .or(manifest.entrypoint.as_deref())
        .ok_or(WebcLoadError::UnknownEntrypoint)?;

    let cmd = manifest
        .commands
        .get(command)
        .ok_or_else(|| WebcLoadError::UnknownCommand {
            name: command.to_string(),
        })?;

    let wcgi_annotations: WcgiAnnotation = cmd
        .annotations
        .get("wcgi")
        .cloned()
        .and_then(|a| serde_cbor::value::from_value(a).ok())
        .unwrap_or_default();

    let wasi_annotations: WasiCommandAnnotation = cmd
        .annotations
        .get("wasi")
        .cloned()
        .and_then(|a| serde_cbor::value::from_value(a).ok())
        .unwrap_or_default();

    // Note: Not all WCGI binaries have "wcgi" annotations (e.g. because they
    // were published before "wasmer.toml" started using it), so we fall back
    // to using the command as our atom name.
    let atom_name = wasi_annotations.atom.as_deref().unwrap_or(command);

    let atom = get_atom(atom_name).ok_or_else(|| WebcLoadError::MissingAtom {
        name: atom_name.to_string(),
    })?;

    // Note: We explicitly use WCGI instead of CgiDialect::default() so we can
    // use existing WCGI packages. We also prefer the user-provided CgiDialect
    // so they can work around packages with bad metadata.

    Ok((
        command.to_string(),
        atom,
        options
            .dialect
            .or(wcgi_annotations.dialect)
            .unwrap_or(CgiDialect::Wcgi),
    ))
}

#[derive(Debug, Default, Clone)]
pub(crate) struct WebcOptions<'a> {
    pub command: WebcCommand<'a>,
    pub dialect: Option<CgiDialect>,
}

#[derive(Debug, Default, Copy, Clone)]
pub(crate) enum WebcCommand<'a> {
    #[default]
    Entrypoint,
    Named(&'a str),
}

impl<'a> WebcCommand<'a> {
    fn as_str(self) -> Option<&'a str> {
        match self {
            WebcCommand::Entrypoint => None,
            WebcCommand::Named(name) => Some(name),
        }
    }
}
