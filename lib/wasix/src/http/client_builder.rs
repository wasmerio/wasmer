use anyhow::{Context, Result};
use reqwest::{Certificate, ClientBuilder, Proxy};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ClientBuilderConfig {
    pub proxy: Option<Proxy>,
    pub ca_file: Option<PathBuf>,
    pub unsafe_disable_ssl_verify: bool,
}

impl Default for ClientBuilderConfig {
    fn default() -> Self {
        ClientBuilderConfig {
            proxy: None,
            ca_file: None,
            unsafe_disable_ssl_verify: false,
        }
    }
}

pub fn create_client_builder(config: ClientBuilderConfig) -> Result<ClientBuilder> {
    let mut builder = ClientBuilder::new()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90));

    if let Some(proxy) = config.proxy {
        builder = builder.proxy(proxy);
    }

    if let Some(ca_file) = config.ca_file {
        let ca_file_borrow = ca_file.clone();
        let ca_cert = fs::read_to_string(ca_file)
            .with_context(|| format!("Failed to read CA certificate from {ca_file_borrow:?}"))?;
        let ca_cert =
            Certificate::from_pem(ca_cert.as_bytes()).context("Failed to parse CA certificate")?;
        builder = builder.add_root_certificate(ca_cert);
    }

    if config.unsafe_disable_ssl_verify {
        builder = builder.danger_accept_invalid_certs(true);
    }

    Ok(builder)
}
