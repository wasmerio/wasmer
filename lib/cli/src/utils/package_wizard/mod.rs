use std::path::{Path, PathBuf};

use anyhow::Context;
use dialoguer::{theme::ColorfulTheme, Select};
use wasmer_api::{types::UserWithNamespaces, WasmerClient};

use super::prompts::PackageCheckMode;

const WASM_STATIC_SERVER_PACKAGE: &str = "wasmer/static-web-server";
const WASM_STATIC_SERVER_VERSION: &str = "1";

const WASMER_WINTER_JS_PACKAGE: &str = "wasmer/winterjs";
const WASMER_WINTER_JS_VERSION: &str = "*";

const WASM_PYTHON_PACKAGE: &str = "wasmer/python";
const WASM_PYTHON_VERSION: &str = "3.12.6";

const SAMPLE_INDEX_HTML: &str = include_str!("./templates/static-site/index.html");
const SAMPLE_JS_WORKER: &str = include_str!("./templates/js-worker/index.js");
const SAMPLE_PY_APPLICATION: &str = include_str!("./templates/py-application/main.py");

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum PackageType {
    #[clap(name = "regular")]
    Regular,
    /// A static website.
    #[clap(name = "static-website")]
    StaticWebsite,
    /// A js-worker
    #[clap(name = "js-worker")]
    JsWorker,
    /// A py-worker
    #[clap(name = "py-application")]
    PyApplication,
}

#[derive(Clone, Copy, Debug)]
pub enum CreateMode {
    Create,
    SelectExisting,
    #[allow(dead_code)]
    CreateOrSelect,
}

fn prompt_for_package_type() -> Result<PackageType, anyhow::Error> {
    let theme = ColorfulTheme::default();
    Select::with_theme(&theme)
        .with_prompt("What type of package do you want to create?")
        .items(&["Basic pacakge", "Static website"])
        .interact()
        .map(|idx| match idx {
            0 => PackageType::Regular,
            1 => PackageType::StaticWebsite,
            _ => unreachable!(),
        })
        .map_err(anyhow::Error::from)
}

#[derive(Debug)]
pub struct PackageWizard {
    pub path: PathBuf,
    pub type_: Option<PackageType>,

    pub create_mode: CreateMode,

    /// Namespace to use.
    pub namespace: Option<String>,
    /// Default namespace to use.
    /// Will still show a prompt, with this as the default value.
    /// Ignored if [`Self::namespace`] is set.
    pub namespace_default: Option<String>,

    /// Pre-configured package name.
    pub name: Option<String>,

    pub user: Option<UserWithNamespaces>,
}

pub struct PackageWizardOutput {
    pub api: Option<wasmer_api::types::Package>,
    pub local_path: Option<PathBuf>,
    pub local_manifest: Option<wasmer_config::package::Manifest>,
}

impl PackageWizard {
    fn build_new_package(&self) -> Result<PackageWizardOutput, anyhow::Error> {
        let ty = match self.type_ {
            Some(t) => t,
            None => prompt_for_package_type()?,
        };

        if !self.path.is_dir() {
            std::fs::create_dir_all(&self.path).with_context(|| {
                format!("Failed to create directory: '{}'", self.path.display())
            })?;
        }

        let manifest = match ty {
            PackageType::Regular => todo!(),
            PackageType::StaticWebsite => initialize_static_site(&self.path)?,
            PackageType::JsWorker => initialize_js_worker(&self.path)?,
            PackageType::PyApplication => initialize_py_worker(&self.path)?,
        };

        let manifest_path = self.path.join("wasmer.toml");
        let manifest_raw = manifest
            .to_string()
            .context("could not serialize package manifest")?;
        std::fs::write(manifest_path, manifest_raw)
            .with_context(|| format!("Failed to write manifest to '{}'", self.path.display()))?;

        Ok(PackageWizardOutput {
            api: None,
            local_path: Some(self.path.clone()),
            local_manifest: Some(manifest),
        })
    }

    async fn prompt_existing_package(
        &self,
        api: Option<&WasmerClient>,
    ) -> Result<PackageWizardOutput, anyhow::Error> {
        // Existing package
        let check = if api.is_some() {
            Some(PackageCheckMode::MustExist)
        } else {
            None
        };

        eprintln!("Enter the name of an existing package:");
        let (_ident, api) = super::prompts::prompt_for_package("Package", None, check, api).await?;
        Ok(PackageWizardOutput {
            api,
            local_path: None,
            local_manifest: None,
        })
    }

    pub async fn run(
        self,
        api: Option<&WasmerClient>,
    ) -> Result<PackageWizardOutput, anyhow::Error> {
        match self.create_mode {
            CreateMode::Create => self.build_new_package(),
            CreateMode::SelectExisting => self.prompt_existing_package(api).await,
            CreateMode::CreateOrSelect => {
                let theme = ColorfulTheme::default();
                let index = Select::with_theme(&theme)
                    .with_prompt("What package do you want to use?")
                    .items(&["Create new package", "Use existing package"])
                    .default(0)
                    .interact()?;

                match index {
                    0 => self.build_new_package(),
                    1 => self.prompt_existing_package(api).await,
                    other => {
                        unreachable!("Unexpected index: {other}");
                    }
                }
            }
        }
    }
}

fn initialize_static_site(path: &Path) -> Result<wasmer_config::package::Manifest, anyhow::Error> {
    let pubdir_name = "public";
    let pubdir = path.join(pubdir_name);
    if !pubdir.is_dir() {
        std::fs::create_dir_all(&pubdir)
            .with_context(|| format!("Failed to create directory: '{}'", pubdir.display()))?;
    }
    let index = pubdir.join("index.html");

    let static_html = SAMPLE_INDEX_HTML.replace("{{title}}", "My static website");

    if !index.is_file() {
        std::fs::write(&index, static_html.as_str())
            .with_context(|| "Could not write index.html file".to_string())?;
    } else {
        // The index.js file already exists, so we can ask the user if they want to overwrite it
        let theme = dialoguer::theme::ColorfulTheme::default();
        let should_overwrite = dialoguer::Confirm::with_theme(&theme)
            .with_prompt("index.html already exists. Do you want to overwrite it?")
            .interact()
            .unwrap();
        if should_overwrite {
            std::fs::write(&index, static_html.as_str())
                .with_context(|| "Could not write index.html file".to_string())?;
        }
    }

    let raw_static_site_toml = format!(
        r#"
[dependencies]
"{}" = "{}"

[fs]
public = "{}"
"#,
        WASM_STATIC_SERVER_PACKAGE, WASM_STATIC_SERVER_VERSION, pubdir_name
    );

    let manifest = wasmer_config::package::Manifest::parse(raw_static_site_toml.as_str())
        .map_err(|e| anyhow::anyhow!("Could not parse js worker manifest: {}", e))?;

    Ok(manifest)
}

fn initialize_js_worker(path: &Path) -> Result<wasmer_config::package::Manifest, anyhow::Error> {
    let srcdir_name = "src";
    let srcdir = path.join(srcdir_name);
    if !srcdir.is_dir() {
        std::fs::create_dir_all(&srcdir)
            .with_context(|| format!("Failed to create directory: '{}'", srcdir.display()))?;
    }

    let index_js = srcdir.join("index.js");

    let sample_js = SAMPLE_JS_WORKER.replace("{{package}}", "My JS worker");

    if !index_js.is_file() {
        std::fs::write(&index_js, sample_js.as_str())
            .with_context(|| "Could not write index.js file".to_string())?;
    }

    // get the remote repository if it exists
    // Todo: add this to the manifest
    // let remote_repo_url = Command::new("git")
    //     .arg("remote")
    //     .arg("get-url")
    //     .arg("origin")
    //     .output()
    //     .map_or("".to_string(), |f| String::from_utf8(f.stdout).unwrap());

    let raw_js_worker_toml = format!(
        r#"
[dependencies]
"{winterjs_pkg}" = "{winterjs_version}"

[fs]
"/src" = "./src"

[[command]]
name = "script"
module = "{winterjs_pkg}:winterjs"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
main-args = ["/src/index.js"]
env = ["JS_PATH=/src/index.js"]
"#,
        winterjs_pkg = WASMER_WINTER_JS_PACKAGE,
        winterjs_version = WASMER_WINTER_JS_VERSION,
    );

    let manifest = wasmer_config::package::Manifest::parse(raw_js_worker_toml.as_str())
        .map_err(|e| anyhow::anyhow!("Could not parse js worker manifest: {}", e))?;

    Ok(manifest)
}

fn initialize_py_worker(path: &Path) -> Result<wasmer_config::package::Manifest, anyhow::Error> {
    let appdir_name = "src";
    let appdir = path.join(appdir_name);
    if !appdir.is_dir() {
        std::fs::create_dir_all(&appdir)
            .with_context(|| format!("Failed to create directory: '{}'", appdir.display()))?;
    }
    let main_py = appdir.join("main.py");

    let sample_main = SAMPLE_PY_APPLICATION.replace("{{package}}", "My Python Worker");

    if !main_py.is_file() {
        std::fs::write(&main_py, sample_main.as_str())
            .with_context(|| "Could not write main.py file".to_string())?;
    }

    // Todo: add this to the manifest
    // let remote_repo_url = Command::new("git")
    //     .arg("remote")
    //     .arg("get-url")
    //     .arg("origin")
    //     .output()
    //     .map_or("".to_string(), |f| String::from_utf8(f.stdout).unwrap());

    let raw_py_worker_toml = format!(
        r#"
[dependencies]
"{}" = "{}"

[fs]
"/src" = "./src"
# "/.env" = "./.env/" # Bundle the virtualenv

[[command]]
name = "script"
module = "{}:python" # The "python" atom from "wasmer/python"
runner = "wasi"

[command.annotations.wasi]
main-args = ["/src/main.py"]
# env = ["PYTHON_PATH=/app/.env:/etc/python3.12/site-packages"] # Make our virtualenv accessible    
"#,
        WASM_PYTHON_PACKAGE, WASM_PYTHON_VERSION, WASM_PYTHON_PACKAGE
    );

    let manifest = wasmer_config::package::Manifest::parse(raw_py_worker_toml.as_str())
        .map_err(|e| anyhow::anyhow!("Could not parse py worker manifest: {}", e))?;

    Ok(manifest)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_package_wizard_create_static_site() {
        let dir = tempfile::tempdir().unwrap();

        PackageWizard {
            path: dir.path().to_owned(),
            type_: Some(PackageType::StaticWebsite),
            create_mode: CreateMode::Create,
            namespace: None,
            namespace_default: None,
            name: None,
            user: None,
        }
        .run(None)
        .await
        .unwrap();

        let manifest = std::fs::read_to_string(dir.path().join("wasmer.toml")).unwrap();
        pretty_assertions::assert_eq!(
            manifest,
            r#"[dependencies]
"wasmer/static-web-server" = "^1"

[fs]
public = "public"
"#,
        );

        assert!(dir.path().join("public").join("index.html").is_file());
    }

    #[tokio::test]
    async fn test_package_wizard_create_js_worker() {
        let dir = tempfile::tempdir().unwrap();

        PackageWizard {
            path: dir.path().to_owned(),
            type_: Some(PackageType::JsWorker),
            create_mode: CreateMode::Create,
            namespace: None,
            namespace_default: None,
            name: None,
            user: None,
        }
        .run(None)
        .await
        .unwrap();
        let manifest = std::fs::read_to_string(dir.path().join("wasmer.toml")).unwrap();

        pretty_assertions::assert_eq!(
            manifest,
            r#"[dependencies]
"wasmer/winterjs" = "*"

[fs]
"/src" = "./src"

[[command]]
name = "script"
module = "wasmer/winterjs:winterjs"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
env = ["JS_PATH=/src/index.js"]
main-args = ["/src/index.js"]
"#,
        );

        assert!(dir.path().join("src").join("index.js").is_file());
    }

    #[tokio::test]
    async fn test_package_wizard_create_py_worker() {
        let dir = tempfile::tempdir().unwrap();

        PackageWizard {
            path: dir.path().to_owned(),
            type_: Some(PackageType::PyApplication),
            create_mode: CreateMode::Create,
            namespace: None,
            namespace_default: None,
            name: None,
            user: None,
        }
        .run(None)
        .await
        .unwrap();
        let manifest = std::fs::read_to_string(dir.path().join("wasmer.toml")).unwrap();

        pretty_assertions::assert_eq!(
            manifest,
            r#"[dependencies]
"wasmer/python" = "^3.12.6"

[fs]
"/src" = "./src"

[[command]]
name = "script"
module = "wasmer/python:python"
runner = "wasi"

[command.annotations.wasi]
main-args = ["/src/main.py"]
"#,
        );

        assert!(dir.path().join("src").join("main.py").is_file());
    }
}
