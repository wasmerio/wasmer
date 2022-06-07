//! When wasmer self-update is executed, this is what gets executed
use anyhow::{Context, Result};
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
use structopt::StructOpt;

/// The options for the `wasmer self-update` subcommand
#[derive(Debug, StructOpt)]
pub struct SelfUpdate {}

impl SelfUpdate {
    /// Runs logic for the `self-update` subcommand
    pub fn execute(&self) -> Result<()> {
        self.inner_execute().context("failed to self-update wasmer")
    }

    fn download_installer() -> Result<bool> {
        let client = reqwest::blocking::Client::new();
        match client
            .get("https://api.github.com/repos/wasmerio/wasmer/releases")
            .header("User-Agent", "wasmer.io self update")
            .header("Accept", "application/vnd.github.v3+json")
            .timeout(std::time::Duration::new(30, 0))
            .send()
            .context("Could not lookup wasmer repository on Github.")?
            .text()
            .map_err(|err| anyhow::Error::new(err))
            .and_then(|data| {
                let v: std::result::Result<serde_json::Value, _> = serde_json::from_str(&data);
                v.map_err(|err| anyhow::Error::new(err))
            }) {
            Ok(mut response) => {
                if let Some(releases) = response.as_array_mut() {
                    releases.retain(|r| {
                        r["tag_name"].is_string() && !r["tag_name"].as_str().unwrap().is_empty()
                    });
                    releases.sort_by_cached_key(|r| {
                        r["tag_name"].as_str().unwrap_or_default().to_string()
                    });
                    if let Some(mut latest) = releases.pop() {
                        println!("Latest release: {}", latest["name"]);
                        if let Some(assets) = latest["assets"].as_array_mut() {
                            assets.retain(|a| {
                                if let Some(name) = a["name"].as_str() {
                                    #[cfg(target_arch = "x86_64")]
                                    {
                                        name.contains("x86_64") || name.contains("amd64")
                                    }
                                    #[cfg(target_arch = "aarch64")]
                                    {
                                        name.contains("arm64") || name.contains("aarch64")
                                    }
                                } else {
                                    false
                                }
                            });
                            assets.retain(|a| {
                                if let Some(name) = a["name"].as_str() {
                                    #[cfg(target_os = "macos")]
                                    {
                                        name.contains("darwin") || name.contains("macos")
                                    }
                                    #[cfg(target_arch = "windows")]
                                    {
                                        name.contains("windows")
                                    }
                                    #[cfg(target_arch = "linux")]
                                    {
                                        name.contains("linux")
                                    }
                                } else {
                                    false
                                }
                            });
                            assets.retain(|a| {
                                if let Some(name) = a["name"].as_str() {
                                    #[cfg(target_env = "musl")]
                                    {
                                        name.contains("musl")
                                    }
                                    #[cfg(not(target_env = "musl"))]
                                    {
                                        !name.contains("musl")
                                    }
                                } else {
                                    false
                                }
                            });
                            if assets.len() == 1 {
                                let browser_download_url =
                                    if let Some(url) = assets[0]["browser_download_url"].as_str() {
                                        url
                                    } else {
                                        return Ok(false);
                                    };
                                println!("Downloading {}", browser_download_url);
                                let binary = client
                                    .get(browser_download_url)
                                    .header("User-Agent", "wasmer.io self update")
                                    .send()
                                    .context("Could not lookup wasmer repository on Github.")?
                                    .bytes()
                                    .map_err(|err| anyhow::Error::new(err))?;
                                // Debug
                                println!("downloaded {} bytes", binary.len());
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            Err(_err) => {
                // Debug
                println!("err {}", _err);
                #[cfg(not(feature = "debug"))]
                eprintln!("Could not get Github API response, falling back to downloading latest version.");
                #[cfg(feature = "debug")]
                log::warn!("Could not get Github API response, falling back to downloading latest version.");
                #[cfg(feature = "debug")]
                log::debug!("API response was:\n{}", _err);
            }
        }

        Ok(false)
    }

    #[cfg(not(target_os = "windows"))]
    fn inner_execute(&self) -> Result<()> {
        let installer = Self::download_installer();
        println!("Got network install: {:?}", installer);
        println!("Fetching latest installer");
        let cmd = Command::new("curl")
            .arg("https://get.wasmer.io")
            .arg("-sSfL")
            .stdout(Stdio::piped())
            .spawn()?;

        let mut process = Command::new("sh")
            .stdin(cmd.stdout.unwrap())
            .stdout(Stdio::inherit())
            .spawn()?;

        process.wait().unwrap();
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn inner_execute(&self) -> Result<()> {
        bail!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
    }
}
