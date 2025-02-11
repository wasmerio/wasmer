use super::AuthorizationState;
use http_body_util::BodyExt;
use hyper::{body::Incoming, Request, Response, StatusCode};
use reqwest::{Body, Method};
use tokio::net::TcpListener;

/// A utility struct used to manage the local server for browser-based authorization.
#[derive(Clone)]
pub(super) struct BrowserAuthContext {
    pub server_shutdown_tx: tokio::sync::mpsc::Sender<bool>,
    pub token_tx: tokio::sync::mpsc::Sender<AuthorizationState>,
}

/// Payload from the frontend after the user has authenticated.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum TokenStatus {
    /// Signifying that the token is cancelled
    Cancelled,
    /// Signifying that the token is authorized
    Authorized,
}

#[inline]
pub(super) async fn setup_listener() -> Result<(TcpListener, String), anyhow::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let port = addr.port();

    let server_url = format!("http://localhost:{port}");

    Ok((listener, server_url))
}

/// Payload from the frontend after the user has authenticated.
///
/// This has the token that we need to set in the WASMER_TOML file.
#[derive(Clone, Debug, serde::Deserialize)]
pub(super) struct ValidatedNonceOutput {
    /// Token Received from the frontend
    pub token: Option<String>,
    /// Status of the token , whether it is authorized or cancelled
    pub status: TokenStatus,
}

pub(super) async fn service_router(
    context: BrowserAuthContext,
    req: Request<Incoming>,
) -> Result<Response<Body>, anyhow::Error> {
    match *req.method() {
        Method::OPTIONS => preflight(req).await,
        Method::POST => handle_post_save_token(context, req).await,
        _ => handle_unknown_method(context).await,
    }
}

async fn preflight(_: Request<Incoming>) -> Result<Response<Body>, anyhow::Error> {
    let response = Response::builder()
        .status(http::StatusCode::OK)
        .header("Access-Control-Allow-Origin", "*") // FIXME: this is not secure, Don't allow all origins. @syrusakbary
        .header("Access-Control-Allow-Headers", "Content-Type")
        .header("Access-Control-Allow-Methods", "POST, OPTIONS")
        .body(Body::default())?;
    Ok(response)
}

async fn handle_post_save_token(
    context: BrowserAuthContext,
    req: Request<Incoming>,
) -> Result<Response<Body>, anyhow::Error> {
    let BrowserAuthContext {
        server_shutdown_tx,
        token_tx,
    } = context;
    let (.., body) = req.into_parts();
    let body = body.collect().await?.to_bytes();

    let ValidatedNonceOutput {
        token,
        status: token_status,
    } = serde_json::from_slice::<ValidatedNonceOutput>(&body)?;

    // send the AuthorizationState based on token_status to the main thread and get the response message
    let (response_message, parse_failure) = match token_status {
        TokenStatus::Cancelled => {
            token_tx
                .send(AuthorizationState::Cancelled)
                .await
                .expect("Failed to send token");

            ("Token Cancelled by the user", false)
        }
        TokenStatus::Authorized => {
            if let Some(token) = token {
                token_tx
                    .send(AuthorizationState::TokenSuccess(token.clone()))
                    .await
                    .expect("Failed to send token");
                ("Token Authorized", false)
            } else {
                ("Token not found", true)
            }
        }
    };

    server_shutdown_tx
        .send(true)
        .await
        .expect("Failed to send shutdown signal");

    let status = if parse_failure {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };

    Ok(Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*") // FIXME: this is not secure, Don't allow all origins. @syrusakbary
        .header("Access-Control-Allow-Headers", "Content-Type")
        .header("Access-Control-Allow-Methods", "POST, OPTIONS")
        .body(Body::from(response_message))?)
}

async fn handle_unknown_method(
    context: BrowserAuthContext,
) -> Result<Response<Body>, anyhow::Error> {
    let BrowserAuthContext {
        server_shutdown_tx,
        token_tx,
    } = context;

    token_tx
        .send(AuthorizationState::UnknownMethod)
        .await
        .expect("Failed to send token");

    server_shutdown_tx
        .send(true)
        .await
        .expect("Failed to send shutdown signal");

    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Body::from("Method not allowed"))?)
}
