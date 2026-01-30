use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::Error as AnyError;
use deno_core::v8;
use deno_error::JsErrorBox;
use deno_maybe_sync::MaybeArc;
use deno_resolver::npm::{
    ByonmNpmResolver, ByonmNpmResolverCreateOptions, CreateInNpmPkgCheckerOptions,
    DenoInNpmPackageChecker,
};
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;
use deno_runtime::deno_core::{
    self, Extension, JsBuffer, JsRuntime, ModuleId, ModuleLoadOptions, ModuleLoadReferrer,
    ModuleLoadResponse, ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType,
    OpState, PollEventLoopOptions, RuntimeOptions, op2,
};
use deno_runtime::deno_fetch;
use deno_runtime::deno_kv::MultiBackendDbHandler;
use deno_runtime::deno_node::{NodeExtInitServices, NodeRequireLoaderRc};
use deno_runtime::deno_permissions::{Permissions, PermissionsContainer};
use deno_runtime::deno_tls::TlsKeys;
use deno_runtime::deno_web::{BlobStore, InMemoryBroadcastChannel};
use deno_runtime::ops;
use deno_runtime::ops::bootstrap::SnapshotOptions;
use deno_runtime::permissions::RuntimePermissionDescriptorParser;
use deno_runtime::worker::{
    WorkerOptions, WorkerServiceOptions, create_permissions_stack_trace_callback,
    create_validate_import_attributes_callback, make_wait_for_inspector_disconnect_callback,
};
use futures_util::FutureExt;
use futures_util::Stream;
use futures_util::StreamExt;
use futures_util::stream;
use http_body::Body as _;
use http_body::Frame;
use http_body_util::{BodyExt, BodyStream, Full, StreamBody, combinators::BoxBody};
use hyper_util::client::legacy::connect::dns::Name;
use node_resolver::{
    DenoIsBuiltInNodeModuleChecker, NodeResolverOptions, PackageJsonResolver,
    PackageJsonResolverRc, errors::PackageJsonLoadError,
};
use serde::{Deserialize, Serialize};
use std::io;
use std::rc::Rc;
use sys_traits::impls::RealSys;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use virtual_fs::AsyncReadExt as _;
use wasmer_wasix::bin_factory::{BinaryPackage, BinaryPackageCommand};

use crate::fs;
use crate::net;

#[derive(Debug, Error)]
pub enum JsRuntimeError {
    #[error("javascript runtime is not configured with an entrypoint")]
    MissingEntrypoint,
    #[error("javascript runtime package is missing its filesystem")]
    MissingWebcFs,
    #[error("javascript runtime handler threw: {0}")]
    Handler(String),
    #[error("javascript runtime error: {0}")]
    Runtime(String),
}

pub type JsBody = BoxBody<bytes::Bytes, AnyError>;
pub const JS_RUNNER_URI: &str = "https://webc.org/runner/js";
type SharedFs = Arc<dyn virtual_fs::FileSystem + Send + Sync>;

pub fn can_run_command(command: &BinaryPackageCommand) -> bool {
    command.metadata().runner.starts_with(JS_RUNNER_URI)
}

pub fn body_from_data(data: impl Into<bytes::Bytes>) -> JsBody {
    BoxBody::new(Full::new(data.into()).map_err(|_| -> AnyError { unreachable!() }))
}

pub fn body_from_stream<S>(s: S) -> JsBody
where
    S: Stream<Item = Result<Frame<bytes::Bytes>, AnyError>> + Send + Sync + 'static,
{
    BoxBody::new(StreamBody::new(s))
}

#[derive(Debug, Clone)]
pub struct JsRuntimePackage {
    id: String,
    entrypoint: String,
    webc_fs: SharedFs,
}

impl JsRuntimePackage {
    pub fn new(id: impl Into<String>, entrypoint: impl Into<String>, webc_fs: SharedFs) -> Self {
        let entrypoint = entrypoint.into();
        Self {
            id: id.into(),
            entrypoint: normalize_webc_path(&entrypoint),
            webc_fs,
        }
    }

    pub fn from_binary_package(
        package: &BinaryPackage,
        command_name: &str,
    ) -> Result<Self, JsRuntimeError> {
        let command = package
            .get_command(command_name)
            .ok_or(JsRuntimeError::MissingEntrypoint)?;
        let entrypoint = resolve_entrypoint(command_name, command, package)?;
        let webc_fs = package
            .webc_fs
            .clone()
            .ok_or(JsRuntimeError::MissingWebcFs)?;
        let webc_fs: SharedFs = webc_fs;
        Ok(Self::new(package.id.to_string(), entrypoint, webc_fs))
    }
}

#[derive(Debug, Clone)]
pub struct JsRunner {
    pool: JsRuntimePool,
}

impl JsRunner {
    pub fn new() -> Self {
        Self::new_with_network(Arc::new(virtual_net::host::LocalNetworking::default()))
    }

    pub fn new_with_network(net: Arc<dyn virtual_net::VirtualNetworking>) -> Self {
        Self {
            pool: JsRuntimePool::new(net),
        }
    }

    pub async fn handle_request(
        &self,
        package: &BinaryPackage,
        command_name: &str,
        request: http::Request<JsBody>,
    ) -> Result<http::Response<JsBody>, JsRuntimeError> {
        let package = JsRuntimePackage::from_binary_package(package, command_name)?;
        self.pool.handle_request(&package, request).await
    }
}

impl Default for JsRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct JsRuntimePool {
    sender: mpsc::UnboundedSender<JsRuntimeCommand>,
    net: net::SharedNet,
}

impl JsRuntimePool {
    pub fn new(net: net::SharedNet) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let pool_net = net.clone();
        thread::Builder::new()
            .name("wasmer-js-runtime".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .expect("js runtime thread requires tokio runtime");
                let local = tokio::task::LocalSet::new();
                local.block_on(&runtime, async move {
                    let (return_sender, mut return_receiver) = mpsc::unbounded_channel();
                    let mut receiver = receiver;
                    let mut pool = JsRuntimePoolInner::new(return_sender, pool_net);
                    loop {
                        tokio::select! {
                            Some(command) = receiver.recv() => {
                                pool.handle_command(command).await;
                            }
                            Some(command) = return_receiver.recv() => {
                                pool.handle_local_command(command).await;
                            }
                            else => break,
                        }
                    }
                });
            })
            .expect("failed to spawn js runtime thread");
        Self { sender, net }
    }

    pub async fn handle_request(
        &self,
        package: &JsRuntimePackage,
        request: http::Request<JsBody>,
    ) -> Result<http::Response<JsBody>, JsRuntimeError> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = JsRuntimeCommand::HandleRequest {
            package: package.clone(),
            request,
            response_tx,
        };
        self.sender
            .send(command)
            .map_err(|_| JsRuntimeError::Runtime("js runtime thread stopped".to_string()))?;
        response_rx
            .await
            .map_err(|_| JsRuntimeError::Runtime("js runtime thread stopped".to_string()))?
    }
}

struct JsRuntimePoolInner {
    workers: HashMap<String, JsWorker>,
    return_sender: mpsc::UnboundedSender<JsRuntimeLocalCommand>,
    net: net::SharedNet,
}

impl JsRuntimePoolInner {
    fn new(
        return_sender: mpsc::UnboundedSender<JsRuntimeLocalCommand>,
        net: net::SharedNet,
    ) -> Self {
        Self {
            workers: HashMap::new(),
            return_sender,
            net,
        }
    }

    async fn handle_command(&mut self, command: JsRuntimeCommand) {
        match command {
            JsRuntimeCommand::HandleRequest {
                package,
                request,
                response_tx,
            } => {
                let response = self.handle_request(&package, request).await;
                let _ = response_tx.send(response);
            }
        }
    }

    async fn handle_local_command(&mut self, command: JsRuntimeLocalCommand) {
        match command {
            JsRuntimeLocalCommand::ReturnWorker { key, worker } => {
                self.workers.insert(key, worker);
            }
        }
    }

    async fn handle_request(
        &mut self,
        package: &JsRuntimePackage,
        request: http::Request<JsBody>,
    ) -> Result<http::Response<JsBody>, JsRuntimeError> {
        let entrypoint = Entrypoint::new(package.entrypoint.clone());
        let worker_key = format!("{}:{}", package.id, entrypoint.key());

        if !self.workers.contains_key(&worker_key) {
            let worker = JsWorker::new(entrypoint, package, self.net.clone()).await?;
            self.workers.insert(worker_key.clone(), worker);
        }

        let mut worker = self.workers.remove(&worker_key).expect("worker exists");
        let decoded = worker.handle_request(request).await?;

        match decoded.body {
            JsDecodedBody::Stream(stream_value) => {
                let (tx, rx) = mpsc::channel(16);
                let sender = self.return_sender.clone();
                let return_key = worker_key.clone();
                tokio::task::spawn_local(async move {
                    let _ = worker.stream_response_body(stream_value, tx).await;
                    let _ = sender.send(JsRuntimeLocalCommand::ReturnWorker {
                        key: return_key,
                        worker,
                    });
                });
                Ok(build_http_response(
                    decoded.status,
                    decoded.headers,
                    JsResponseBody::Stream(rx),
                )?)
            }
            JsDecodedBody::Bytes(bytes) => {
                let response = build_http_response(
                    decoded.status,
                    decoded.headers,
                    JsResponseBody::Bytes(bytes),
                )?;
                self.workers.insert(worker_key, worker);
                Ok(response)
            }
            JsDecodedBody::Text(text) => {
                let response = build_http_response(
                    decoded.status,
                    decoded.headers,
                    JsResponseBody::Text(text),
                )?;
                self.workers.insert(worker_key, worker);
                Ok(response)
            }
            JsDecodedBody::Empty => {
                let response =
                    build_http_response(decoded.status, decoded.headers, JsResponseBody::Empty)?;
                self.workers.insert(worker_key, worker);
                Ok(response)
            }
        }
    }
}

enum JsRuntimeCommand {
    HandleRequest {
        package: JsRuntimePackage,
        request: http::Request<JsBody>,
        response_tx: oneshot::Sender<Result<http::Response<JsBody>, JsRuntimeError>>,
    },
}

enum JsRuntimeLocalCommand {
    ReturnWorker { key: String, worker: JsWorker },
}

#[derive(Debug, Clone)]
struct Entrypoint {
    path: String,
}

impl Entrypoint {
    fn new(path: String) -> Self {
        Self { path }
    }

    fn key(&self) -> &str {
        self.path.as_str()
    }
}

fn resolve_entrypoint(
    command_name: &str,
    command: &BinaryPackageCommand,
    package: &BinaryPackage,
) -> Result<String, JsRuntimeError> {
    if let Some(annotation) = command
        .metadata()
        .annotation::<JsRunnerAnnotation>(JsRunnerAnnotation::KEY)
        .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?
    {
        if let Some(path) = annotation.entrypoint() {
            return Ok(normalize_webc_path(path));
        }
    }

    for key in ["entrypoint", "module", "script"] {
        if let Some(path) = command
            .metadata()
            .annotation::<String>(key)
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?
        {
            return Ok(normalize_webc_path(&path));
        }
    }

    if looks_like_path(command_name) {
        return Ok(normalize_webc_path(command_name));
    }

    if let Some(entrypoint) = package.entrypoint_cmd.as_deref()
        && looks_like_path(entrypoint)
    {
        return Ok(normalize_webc_path(entrypoint));
    }

    Err(JsRuntimeError::MissingEntrypoint)
}

fn normalize_webc_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn looks_like_path(path: &str) -> bool {
    path.contains('/')
        || path.ends_with(".js")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
        || path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".jsx")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct JsRunnerAnnotation {
    #[serde(default)]
    entrypoint: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    script: Option<String>,
}

impl JsRunnerAnnotation {
    const KEY: &'static str = "js";

    fn entrypoint(&self) -> Option<&str> {
        self.entrypoint
            .as_deref()
            .or(self.module.as_deref())
            .or(self.script.as_deref())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JsRequestParts {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body_stream_id: Option<u64>,
}

#[derive(Debug)]
enum JsResponseBody {
    Empty,
    Text(String),
    Bytes(bytes::Bytes),
    Stream(mpsc::Receiver<bytes::Bytes>),
}

#[derive(Debug)]
enum JsDecodedBody {
    Empty,
    Text(String),
    Bytes(bytes::Bytes),
    Stream(v8::Global<v8::Value>),
}

#[derive(Debug)]
struct JsDecodedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: JsDecodedBody,
}

#[derive(Clone)]
struct WebcModuleLoader {
    webc_fs: SharedFs,
}

impl WebcModuleLoader {
    async fn read_module_source(&self, path: &Path) -> Result<String, JsRuntimeError> {
        let mut file = self
            .webc_fs
            .new_open_options()
            .read(true)
            .open(path)
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)
            .await
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
        Ok(buffer)
    }

    fn module_type_for_path(path: &Path) -> ModuleType {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => ModuleType::Json,
            Some("mjs") | Some("cjs") => ModuleType::JavaScript,
            _ => ModuleType::JavaScript,
        }
    }
}

impl ModuleLoader for WebcModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: deno_core::ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if let Some(mapped) = map_node_specifier(specifier) {
            return Ok(ModuleSpecifier::parse(&mapped).map_err(JsErrorBox::from_err)?);
        }
        if is_bare_specifier(specifier) {
            let mapped = format!("file:///node_modules/{specifier}/mod.js");
            return Ok(ModuleSpecifier::parse(&mapped).map_err(JsErrorBox::from_err)?);
        }
        deno_core::resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let module_specifier = module_specifier.clone();
        let loader = self.clone();
        let fut = async move {
            let path = module_specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic("invalid module path"))?;
            let code = loader
                .read_module_source(&path)
                .await
                .map_err(|err| JsErrorBox::generic(err.to_string()))?;
            let module_type = WebcModuleLoader::module_type_for_path(&path);
            Ok(ModuleSource::new(
                module_type,
                ModuleSourceCode::String(deno_core::FastString::from(code)),
                &module_specifier,
                None,
            ))
        }
        .boxed_local();
        ModuleLoadResponse::Async(Box::pin(fut))
    }
}

fn is_bare_specifier(specifier: &str) -> bool {
    if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
        return false;
    }
    !specifier.contains("://")
}

fn map_node_specifier(specifier: &str) -> Option<String> {
    let path = specifier.strip_prefix("node:")?;
    Some(format!("file:///node_modules/{path}/mod.js"))
}

#[derive(Default)]
struct StreamState {
    next_id: u64,
    request_streams: HashMap<u64, Arc<tokio::sync::Mutex<mpsc::Receiver<bytes::Bytes>>>>,
    response_streams: HashMap<u64, mpsc::Sender<bytes::Bytes>>,
}

impl StreamState {
    fn insert_request_stream(&mut self, receiver: mpsc::Receiver<bytes::Bytes>) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.request_streams
            .insert(id, Arc::new(tokio::sync::Mutex::new(receiver)));
        id
    }

    fn insert_response_stream(&mut self, sender: mpsc::Sender<bytes::Bytes>) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.response_streams.insert(id, sender);
        id
    }
}

#[op2(async)]
#[buffer]
async fn op_js_request_body_next(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<Option<Vec<u8>>, JsErrorBox> {
    let receiver = {
        let mut state = state.borrow_mut();
        let streams = state.borrow_mut::<StreamState>();
        streams
            .request_streams
            .get(&(id as u64))
            .cloned()
            .ok_or_else(|| JsErrorBox::generic("unknown request body stream"))?
    };

    let mut receiver = receiver.lock().await;
    let chunk = receiver.recv().await;
    if chunk.is_none() {
        let mut state = state.borrow_mut();
        let streams = state.borrow_mut::<StreamState>();
        streams.request_streams.remove(&(id as u64));
    }
    Ok(chunk.map(|bytes| bytes.to_vec()))
}

#[op2(async)]
async fn op_js_response_body_send(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
    #[buffer] chunk: JsBuffer,
) -> Result<(), JsErrorBox> {
    let sender = {
        let mut state = state.borrow_mut();
        let streams = state.borrow_mut::<StreamState>();
        streams
            .response_streams
            .get(&(id as u64))
            .cloned()
            .ok_or_else(|| JsErrorBox::generic("unknown response body stream"))?
    };
    sender
        .send(bytes::Bytes::copy_from_slice(&chunk))
        .await
        .map_err(|_| JsErrorBox::generic("response body stream closed"))?;
    Ok(())
}

#[op2(async)]
async fn op_js_response_body_close(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<(), JsErrorBox> {
    let sender = {
        let mut state = state.borrow_mut();
        let streams = state.borrow_mut::<StreamState>();
        streams.response_streams.remove(&(id as u64))
    };
    drop(sender);
    Ok(())
}

deno_core::extension!(
    wasmer_js_stream,
    ops = [
        op_js_request_body_next,
        op_js_response_body_send,
        op_js_response_body_close
    ],
    js = ["ext:wasmer_js_stream/bootstrap.js" = {
        source = r#"
const ops = Deno.core.ops;
const encode = typeof Deno?.core?.encode === "function"
  ? Deno.core.encode
  : null;

function normalizeChunk(chunk) {
  if (chunk == null) return null;
  if (chunk instanceof Uint8Array) return chunk;
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  if (ArrayBuffer.isView(chunk)) {
    return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  }
  if (typeof chunk === "string") {
    if (!encode) throw new TypeError("Deno.core.encode is not available");
    return encode(chunk);
  }
  throw new TypeError("response body chunks must be Uint8Array, ArrayBuffer, or string");
}

globalThis.__wasmer_stream = {
  buildRequest(req) {
    const body = req.body_stream_id == null
      ? null
      : {
          async *[Symbol.asyncIterator]() {
            while (true) {
              const chunk = await ops.op_js_request_body_next(req.body_stream_id);
              if (chunk === null) return;
              yield chunk;
            }
          }
        };
    return {
      method: req.method,
      url: req.url,
      headers: req.headers,
      body
    };
  },
  async normalizeResponse(response) {
    if (response instanceof Response) {
      const headers = Array.from(response.headers.entries());
      const body = new Uint8Array(await response.arrayBuffer());
      return {
        status: response.status,
        headers,
        body,
      };
    }
    if (response && typeof response === "object") {
      const headersValue = response.headers;
      if (headersValue && !Array.isArray(headersValue)) {
        if (headersValue instanceof Headers) {
          response.headers = Array.from(headersValue.entries());
        } else {
          response.headers = Object.entries(headersValue);
        }
      }
    }
    return response;
  },
  async pumpResponseBody(id, body) {
    try {
      if (body && typeof body.getReader === "function") {
        const reader = body.getReader();
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          const normalized = normalizeChunk(value);
          if (normalized) {
            await ops.op_js_response_body_send(id, normalized);
          }
        }
        return;
      }
      if (body && body[Symbol.asyncIterator]) {
        for await (const chunk of body) {
          const normalized = normalizeChunk(chunk);
          if (normalized) {
            await ops.op_js_response_body_send(id, normalized);
          }
        }
      }
    } finally {
      await ops.op_js_response_body_close(id);
    }
  }
};
"#
    }],
    state = |state| {
        if !state.has::<SnapshotOptions>() {
            state.put(SnapshotOptions::default());
        }
        state.put(StreamState::default());
    },
);

struct EnableRawImports(Arc<AtomicBool>);

fn base_extensions(has_snapshot: bool) -> Vec<Extension> {
    vec![
        deno_runtime::deno_telemetry::deno_telemetry::init(),
        deno_runtime::deno_webidl::deno_webidl::init(),
        deno_runtime::deno_web::deno_web::lazy_init(),
        deno_runtime::deno_webgpu::deno_webgpu::init(),
        deno_runtime::deno_image::deno_image::init(),
        deno_runtime::deno_fetch::deno_fetch::lazy_init(),
        deno_runtime::deno_cache::deno_cache::lazy_init(),
        deno_runtime::deno_websocket::deno_websocket::lazy_init(),
        deno_runtime::deno_webstorage::deno_webstorage::lazy_init(),
        deno_runtime::deno_crypto::deno_crypto::lazy_init(),
        deno_runtime::deno_ffi::deno_ffi::lazy_init(),
        net::deno_net::lazy_init(),
        deno_runtime::deno_tls::deno_tls::init(),
        deno_runtime::deno_kv::deno_kv::lazy_init::<MultiBackendDbHandler>(),
        deno_runtime::deno_cron::deno_cron::init(deno_runtime::deno_cron::LocalCronHandler::new()),
        deno_runtime::deno_napi::deno_napi::lazy_init(),
        deno_runtime::deno_http::deno_http::lazy_init(),
        deno_runtime::deno_io::deno_io::lazy_init(),
        deno_runtime::deno_fs::deno_fs::lazy_init(),
        deno_runtime::deno_os::deno_os::lazy_init(),
        deno_runtime::deno_process::deno_process::lazy_init(),
        deno_runtime::deno_node::deno_node::lazy_init::<
            DenoInNpmPackageChecker,
            ByonmNpmResolver<fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>>,
            fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>,
        >(),
        deno_runtime::ops::runtime::deno_runtime::lazy_init(),
        deno_runtime::ops::worker_host::deno_worker_host::lazy_init(),
        deno_runtime::ops::fs_events::deno_fs_events::init(),
        deno_runtime::ops::permissions::deno_permissions::init(),
        deno_runtime::ops::tty::deno_tty::init(),
        deno_runtime::ops::http::deno_http_runtime::init(),
        deno_runtime::deno_bundle_runtime::deno_bundle_runtime::lazy_init(),
        deno_runtime::ops::bootstrap::deno_bootstrap::init(
            has_snapshot.then(Default::default),
            false,
        ),
        deno_runtime::runtime::init(),
        deno_runtime::ops::web_worker::deno_web_worker::init().disable(),
    ]
}

fn build_js_runtime(
    main_module: &ModuleSpecifier,
    services: WorkerServiceOptions<
        DenoInNpmPackageChecker,
        ByonmNpmResolver<fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>>,
        fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>,
    >,
    mut options: WorkerOptions,
    net: net::SharedNet,
) -> Result<JsRuntime, JsRuntimeError> {
    let enable_raw_imports = Arc::new(AtomicBool::new(false));
    let exit_code = deno_runtime::deno_os::ExitCode::default();

    let WorkerServiceOptions {
        blob_store,
        broadcast_channel,
        deno_rt_native_addon_loader,
        feature_checker,
        fs,
        module_loader,
        node_services,
        npm_process_state_provider,
        permissions,
        root_cert_store_provider,
        fetch_dns_resolver,
        shared_array_buffer_store,
        compiled_wasm_module_store,
        v8_code_cache: _,
        bundle_provider,
    } = services;

    let mut extensions = base_extensions(options.startup_snapshot.is_some());
    extensions.extend(std::mem::take(&mut options.extensions));

    let mut js_runtime = JsRuntime::new(RuntimeOptions {
        module_loader: Some(module_loader.clone()),
        startup_snapshot: options.startup_snapshot,
        create_params: options.create_params,
        skip_op_registration: options.skip_op_registration,
        shared_array_buffer_store,
        compiled_wasm_module_store,
        extensions,
        #[cfg(feature = "transpile")]
        extension_transpiler: Some(Rc::new(|specifier, source| {
            deno_runtime::transpile::maybe_transpile_source(specifier, source)
        })),
        #[cfg(not(feature = "transpile"))]
        extension_transpiler: None,
        inspector: true,
        is_main: true,
        worker_id: None,
        op_metrics_factory_fn: None,
        wait_for_inspector_disconnect_callback: Some(make_wait_for_inspector_disconnect_callback()),
        validate_import_attributes_cb: Some(create_validate_import_attributes_callback(
            enable_raw_imports.clone(),
        )),
        import_assertions_support: deno_core::ImportAssertionsSupport::Error,
        maybe_op_stack_trace_callback: options
            .enable_stack_trace_arg_in_ops
            .then(create_permissions_stack_trace_callback),
        extension_code_cache: None,
        v8_platform: None,
        custom_module_evaluation_cb: None,
        eval_context_code_cache_cbs: None,
    });

    js_runtime
        .op_state()
        .borrow_mut()
        .put(EnableRawImports(enable_raw_imports.clone()));

    js_runtime
        .op_state()
        .borrow_mut()
        .borrow::<EnableRawImports>()
        .0
        .store(options.enable_raw_imports, Ordering::Relaxed);

    if !js_runtime.op_state().borrow().has::<SnapshotOptions>() {
        js_runtime
            .op_state()
            .borrow_mut()
            .put(SnapshotOptions::default());
    }

    js_runtime
        .lazy_init_extensions(vec![
            deno_runtime::deno_web::deno_web::args(
                blob_store.clone(),
                options.bootstrap.location.clone(),
                broadcast_channel.clone(),
            ),
            deno_runtime::deno_fetch::deno_fetch::args(deno_runtime::deno_fetch::Options {
                user_agent: options.bootstrap.user_agent.clone(),
                root_cert_store_provider: root_cert_store_provider.clone(),
                unsafely_ignore_certificate_errors: options
                    .unsafely_ignore_certificate_errors
                    .clone(),
                file_fetch_handler: Rc::new(deno_runtime::deno_fetch::FsFetchHandler),
                resolver: fetch_dns_resolver,
                ..Default::default()
            }),
            deno_runtime::deno_cache::deno_cache::args(None),
            deno_runtime::deno_websocket::deno_websocket::args(),
            deno_runtime::deno_webstorage::deno_webstorage::args(
                options.origin_storage_dir.clone(),
            ),
            deno_runtime::deno_crypto::deno_crypto::args(options.seed),
            deno_runtime::deno_ffi::deno_ffi::args(deno_rt_native_addon_loader.clone()),
            net::deno_net::args(
                net,
                root_cert_store_provider.clone(),
                options.unsafely_ignore_certificate_errors.clone(),
            ),
            deno_runtime::deno_kv::deno_kv::args(
                MultiBackendDbHandler::remote_or_sqlite(
                    options.origin_storage_dir.clone(),
                    options.seed,
                    deno_runtime::deno_kv::remote::HttpOptions {
                        user_agent: options.bootstrap.user_agent.clone(),
                        root_cert_store_provider: root_cert_store_provider.clone(),
                        unsafely_ignore_certificate_errors: options
                            .unsafely_ignore_certificate_errors
                            .clone(),
                        client_cert_chain_and_key: TlsKeys::Null,
                        proxy: None,
                    },
                ),
                deno_runtime::deno_kv::KvConfig::builder().build(),
            ),
            deno_runtime::deno_napi::deno_napi::args(deno_rt_native_addon_loader.clone()),
            deno_runtime::deno_http::deno_http::args(deno_runtime::deno_http::Options {
                no_legacy_abort: options.bootstrap.no_legacy_abort,
                ..Default::default()
            }),
            deno_runtime::deno_io::deno_io::args(Some(options.stdio)),
            deno_runtime::deno_fs::deno_fs::args(fs.clone()),
            deno_runtime::deno_os::deno_os::args(Some(exit_code.clone())),
            deno_runtime::deno_process::deno_process::args(npm_process_state_provider),
            deno_runtime::deno_node::deno_node::args::<
                DenoInNpmPackageChecker,
                ByonmNpmResolver<fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>>,
                fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>,
            >(node_services, fs.clone()),
            deno_runtime::ops::runtime::deno_runtime::args(main_module.clone()),
            deno_runtime::ops::worker_host::deno_worker_host::args(
                options.create_web_worker_cb.clone(),
                options.format_js_error_fn.clone(),
            ),
            deno_runtime::deno_bundle_runtime::deno_bundle_runtime::args(bundle_provider.clone()),
        ])
        .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;

    {
        let state = js_runtime.op_state();
        let mut state = state.borrow_mut();
        state.put::<PermissionsContainer>(permissions);
        state.put(ops::TestingFeaturesEnabled(
            options.bootstrap.enable_testing_features,
        ));
        state.put(feature_checker);
    }

    bootstrap_runtime(&mut js_runtime, options.bootstrap)?;
    Ok(js_runtime)
}

fn bootstrap_runtime(
    js_runtime: &mut JsRuntime,
    options: BootstrapOptions,
) -> Result<(), JsRuntimeError> {
    {
        let op_state = js_runtime.op_state();
        let mut state = op_state.borrow_mut();
        state.put(options.clone());
        if let Some((fd, serialization)) = options.node_ipc_init {
            state.put(deno_runtime::deno_node::ChildPipeFd(fd, serialization));
        }
    }

    deno_core::scope!(scope, js_runtime);
    let args = options.as_v8(scope);
    let context = js_runtime.main_context();
    let context_local = v8::Local::new(scope, context);
    let global_obj = context_local.global(scope);
    let bootstrap_str = v8::String::new_external_onebyte_static(scope, b"bootstrap").unwrap();
    let bootstrap_ns: v8::Local<v8::Object> = global_obj
        .get(scope, bootstrap_str.into())
        .unwrap()
        .try_into()
        .unwrap();
    let main_runtime_str = v8::String::new_external_onebyte_static(scope, b"mainRuntime").unwrap();
    let bootstrap_fn = bootstrap_ns.get(scope, main_runtime_str.into()).unwrap();
    let bootstrap_fn = v8::Local::<v8::Function>::try_from(bootstrap_fn).unwrap();
    let undefined = v8::undefined(scope);
    bootstrap_fn.call(scope, undefined.into(), &[args]);
    if let Some(exception) = scope.exception() {
        let error = deno_core::error::JsError::from_v8_exception(scope, exception);
        return Err(JsRuntimeError::Runtime(error.to_string()));
    }
    Ok(())
}

async fn preload_main_module(
    runtime: &mut JsRuntime,
    specifier: &ModuleSpecifier,
) -> Result<ModuleId, deno_core::error::AnyError> {
    runtime.load_main_es_module(specifier).await
}

async fn evaluate_module(
    runtime: &mut JsRuntime,
    id: ModuleId,
) -> Result<(), deno_core::error::AnyError> {
    let mut receiver = runtime.mod_evaluate(id);
    tokio::select! {
        biased;
        maybe_result = &mut receiver => maybe_result,
        event_loop_result = runtime.run_event_loop(PollEventLoopOptions::default()) => {
            event_loop_result?;
            receiver.await
        }
    }
}

struct JsWorker {
    runtime: JsRuntime,
    handler: deno_core::v8::Global<deno_core::v8::Function>,
}

impl std::fmt::Debug for JsWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsWorker")
            .field("handler", &"<v8::Function>")
            .finish()
    }
}

impl JsWorker {
    async fn new(
        entrypoint: Entrypoint,
        package: &JsRuntimePackage,
        net: net::SharedNet,
    ) -> Result<Self, JsRuntimeError> {
        let webc_fs = package.webc_fs.clone();
        let main_specifier = ModuleSpecifier::parse(&format!("file://{}", entrypoint.path))
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;

        let loader = WebcModuleLoader {
            webc_fs: webc_fs.clone(),
        };
        let module_loader = std::rc::Rc::new(loader);
        let permissions = build_permissions()?;
        let node_services = Some(build_node_services(webc_fs.clone())?);
        let services = WorkerServiceOptions {
            blob_store: Arc::new(BlobStore::default()),
            broadcast_channel: InMemoryBroadcastChannel::default(),
            deno_rt_native_addon_loader: None,
            feature_checker: Arc::new(FeatureChecker::default()),
            fs: Arc::new(fs::FsBridge::new(webc_fs.clone())),
            module_loader,
            node_services,
            npm_process_state_provider: None,
            permissions,
            root_cert_store_provider: None,
            fetch_dns_resolver: deny_dns_resolver(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            v8_code_cache: None,
            bundle_provider: None,
        };
        let mut options = WorkerOptions::default();
        options.extensions.push(wasmer_js_stream::init());
        let mut runtime = build_js_runtime(&main_specifier, services, options, net)?;

        let module_id = preload_main_module(&mut runtime, &main_specifier)
            .await
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
        evaluate_module(&mut runtime, module_id)
            .await
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;

        let handler = {
            let namespace = runtime
                .get_module_namespace(module_id)
                .expect("module namespace");
            deno_core::scope!(scope, runtime);
            let namespace = deno_core::v8::Local::new(scope, namespace);
            let default_key = deno_core::v8::String::new(scope, "default").unwrap();
            let default = namespace.get(scope, default_key.into()).unwrap();
            let function = deno_core::v8::Local::<deno_core::v8::Function>::try_from(default)
                .expect("default export must be a function");
            deno_core::v8::Global::new(scope.as_ref(), function)
        };

        Ok(Self { runtime, handler })
    }

    async fn handle_request(
        &mut self,
        request: http::Request<JsBody>,
    ) -> Result<JsDecodedResponse, JsRuntimeError> {
        let (parts, body) = request.into_parts();
        let body_stream_id = self.register_request_body_stream(body)?;

        let request = JsRequestParts {
            method: parts.method.to_string(),
            url: parts.uri.to_string(),
            headers: parts
                .headers
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
                .collect(),
            body_stream_id,
        };

        let response = self.invoke_handler(request).await?;
        Ok(response)
    }

    async fn run_event_loop(
        &mut self,
        wait_for_inspector: bool,
    ) -> Result<(), deno_core::error::AnyError> {
        self.runtime
            .run_event_loop(PollEventLoopOptions {
                wait_for_inspector,
                ..Default::default()
            })
            .await
    }

    fn register_request_body_stream(
        &mut self,
        body: JsBody,
    ) -> Result<Option<u64>, JsRuntimeError> {
        if body.is_end_stream() {
            return Ok(None);
        }

        let (body_tx, body_rx) = mpsc::channel(16);
        let op_state = self.runtime.op_state();
        let stream_id = {
            let mut state = op_state.borrow_mut();
            let streams = state.borrow_mut::<StreamState>();
            streams.insert_request_stream(body_rx)
        };

        tokio::task::spawn_local(async move {
            let mut stream = BodyStream::new(body);
            while let Some(frame) = stream.next().await {
                match frame {
                    Ok(frame) => {
                        let Ok(data) = frame.into_data() else {
                            continue;
                        };
                        if body_tx.send(data).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Some(stream_id))
    }

    async fn invoke_handler(
        &mut self,
        request: JsRequestParts,
    ) -> Result<JsDecodedResponse, JsRuntimeError> {
        let (value, is_promise) = {
            deno_core::scope!(scope, self.runtime);
            let handler = v8::Local::new(scope, &self.handler);
            let arg = deno_core::serde_v8::to_v8(scope, request)
                .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;

            let request_builder = get_js_helper(scope, "buildRequest")?;
            let request_value = request_builder
                .call(scope, request_builder.into(), &[arg])
                .ok_or_else(|| JsRuntimeError::Handler("handler threw".to_string()))?;

            let recv = handler
                .call(
                    scope,
                    scope.get_current_context().global(scope).into(),
                    &[request_value],
                )
                .ok_or_else(|| JsRuntimeError::Handler("handler threw".to_string()))?;
            let is_promise = recv.is_promise();
            (v8::Global::new(scope.as_ref(), recv), is_promise)
        };

        let value = if is_promise {
            let resolve = self.runtime.resolve(value);
            self.runtime
                .with_event_loop_promise(resolve, Default::default())
                .await
                .map_err(|err| JsRuntimeError::Handler(err.to_string()))?
        } else {
            value
        };

        let normalized = self.normalize_response(value).await?;
        self.decode_response(normalized)
    }

    async fn normalize_response(
        &mut self,
        value: v8::Global<v8::Value>,
    ) -> Result<v8::Global<v8::Value>, JsRuntimeError> {
        let (value, is_promise) = {
            deno_core::scope!(scope, self.runtime);
            let normalize_response = get_js_helper(scope, "normalizeResponse")?;
            let value = v8::Local::new(scope, value);
            let value = normalize_response
                .call(scope, normalize_response.into(), &[value])
                .ok_or_else(|| {
                    JsRuntimeError::Runtime("response normalization failed".to_string())
                })?;
            (v8::Global::new(scope.as_ref(), value), value.is_promise())
        };

        if is_promise {
            let resolve = self.runtime.resolve(value);
            let value = self
                .runtime
                .with_event_loop_promise(resolve, Default::default())
                .await
                .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
            Ok(value)
        } else {
            Ok(value)
        }
    }

    fn decode_response(
        &mut self,
        value: v8::Global<v8::Value>,
    ) -> Result<JsDecodedResponse, JsRuntimeError> {
        deno_core::scope!(scope, self.runtime);
        let value = v8::Local::new(scope, value);
        let response = value
            .to_object(scope)
            .ok_or_else(|| JsRuntimeError::Runtime("response must be an object".to_string()))?;

        let status_key = v8::String::new(scope, "status").unwrap();
        let headers_key = v8::String::new(scope, "headers").unwrap();
        let body_key = v8::String::new(scope, "body").unwrap();

        let status_value = response
            .get(scope, status_key.into())
            .ok_or_else(|| JsRuntimeError::Runtime("response missing status".to_string()))?;
        let status = status_value
            .integer_value(scope)
            .ok_or_else(|| JsRuntimeError::Runtime("invalid status".to_string()))?;

        let headers_value = response
            .get(scope, headers_key.into())
            .unwrap_or_else(|| v8::Array::new(scope, 0).into());
        let headers: Vec<(String, String)> = deno_core::serde_v8::from_v8(scope, headers_value)
            .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;

        let body_value = response
            .get(scope, body_key.into())
            .unwrap_or_else(|| v8::undefined(scope).into());
        let body = decode_body(scope, body_value)?;

        Ok(JsDecodedResponse {
            status: status as u16,
            headers,
            body,
        })
    }

    async fn stream_response_body(
        &mut self,
        body: v8::Global<v8::Value>,
        body_tx: mpsc::Sender<bytes::Bytes>,
    ) -> Result<(), JsRuntimeError> {
        let stream_id = {
            let op_state = self.runtime.op_state();
            let mut state = op_state.borrow_mut();
            let streams = state.borrow_mut::<StreamState>();
            streams.insert_response_stream(body_tx)
        };
        let (value, is_promise) = {
            deno_core::scope!(scope, self.runtime);
            let body = v8::Local::new(scope, body);
            let helper = get_js_helper(scope, "pumpResponseBody")?;
            let stream_id_value = v8::Number::new(scope, stream_id as f64);
            let value = helper
                .call(scope, helper.into(), &[stream_id_value.into(), body])
                .ok_or_else(|| JsRuntimeError::Handler("handler threw".to_string()))?;
            (v8::Global::new(scope.as_ref(), value), value.is_promise())
        };

        if is_promise {
            let resolve = self.runtime.resolve(value);
            let result = self
                .runtime
                .with_event_loop_future(resolve, Default::default())
                .await;
            if let Err(err) = result {
                let op_state = self.runtime.op_state();
                let mut state = op_state.borrow_mut();
                let streams = state.borrow_mut::<StreamState>();
                streams.response_streams.remove(&stream_id);
                return Err(JsRuntimeError::Runtime(err.to_string()));
            }
        } else {
            self.run_event_loop(false)
                .await
                .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
        }
        Ok(())
    }
}

fn build_http_response(
    status: u16,
    headers: Vec<(String, String)>,
    body: JsResponseBody,
) -> Result<http::Response<JsBody>, JsRuntimeError> {
    let mut builder = http::Response::builder().status(status);
    let mut has_content_type = false;

    for (key, value) in headers {
        if key.eq_ignore_ascii_case(http::header::CONTENT_TYPE.as_str()) {
            has_content_type = true;
        }
        if let (Ok(key), Ok(value)) = (
            http::header::HeaderName::try_from(key),
            value.parse::<http::header::HeaderValue>(),
        ) {
            builder = builder.header(key, value);
        }
    }

    let body = match body {
        JsResponseBody::Empty => body_from_data(bytes::Bytes::new()),
        JsResponseBody::Text(text) => {
            if !has_content_type {
                builder = builder.header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8");
            }
            body_from_data(text)
        }
        JsResponseBody::Bytes(bytes) => body_from_data(bytes),
        JsResponseBody::Stream(rx) => body_from_stream(stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Some(bytes) => {
                    let frame = Frame::data(bytes);
                    Some((Ok::<_, AnyError>(frame), rx))
                }
                None => None,
            }
        })),
    };

    builder
        .body(body)
        .map_err(|err| JsRuntimeError::Runtime(err.to_string()))
}

fn get_js_helper<'a, 'b>(
    scope: &mut v8::PinScope<'a, 'b>,
    name: &str,
) -> Result<v8::Local<'a, v8::Function>, JsRuntimeError> {
    let global = scope.get_current_context().global(scope);
    let js_key = v8::String::new(scope, "__wasmer_stream").unwrap();
    let js_value = global
        .get(scope, js_key.into())
        .ok_or_else(|| JsRuntimeError::Runtime("missing __wasmer_stream helpers".to_string()))?;
    let js_object = js_value
        .to_object(scope)
        .ok_or_else(|| JsRuntimeError::Runtime("invalid __wasmer_stream helpers".to_string()))?;
    let fn_key = v8::String::new(scope, name).unwrap();
    let fn_value = js_object
        .get(scope, fn_key.into())
        .ok_or_else(|| JsRuntimeError::Runtime("missing helper function".to_string()))?;
    v8::Local::<v8::Function>::try_from(fn_value)
        .map_err(|_| JsRuntimeError::Runtime("invalid helper function".to_string()))
}

fn decode_body<'a, 'b>(
    scope: &mut v8::PinScope<'a, 'b>,
    body_value: v8::Local<'a, v8::Value>,
) -> Result<JsDecodedBody, JsRuntimeError> {
    if body_value.is_null() || body_value.is_undefined() {
        return Ok(JsDecodedBody::Empty);
    }

    if body_value.is_string() {
        let body = body_value
            .to_string(scope)
            .map(|value| value.to_rust_string_lossy(scope.as_ref()))
            .unwrap_or_default();
        return Ok(JsDecodedBody::Text(body));
    }

    if body_value.is_typed_array() {
        let view = v8::Local::<v8::ArrayBufferView>::try_from(body_value)
            .map_err(|_| JsRuntimeError::Runtime("invalid typed array body".to_string()))?;
        let mut buffer = vec![0u8; view.byte_length()];
        view.copy_contents(&mut buffer);
        return Ok(JsDecodedBody::Bytes(bytes::Bytes::from(buffer)));
    }

    if is_stream_body(scope, body_value) {
        return Ok(JsDecodedBody::Stream(v8::Global::new(
            scope.as_ref(),
            body_value,
        )));
    }

    Err(JsRuntimeError::Runtime(
        "response body must be string, Uint8Array, or stream".to_string(),
    ))
}

fn is_stream_body<'a, 'b>(
    scope: &mut v8::PinScope<'a, 'b>,
    value: v8::Local<'a, v8::Value>,
) -> bool {
    let Some(obj) = value.to_object(scope) else {
        return false;
    };
    let get_reader_key = v8::String::new(scope, "getReader").unwrap();
    let has_reader = obj
        .get(scope, get_reader_key.into())
        .map(|value| value.is_function())
        .unwrap_or(false);
    if has_reader {
        return true;
    }

    let async_iter = v8::Symbol::get_async_iterator(scope);
    obj.get(scope, async_iter.into())
        .map(|value| value.is_function())
        .unwrap_or(false)
}

fn build_permissions() -> Result<PermissionsContainer, JsRuntimeError> {
    let parser = RuntimePermissionDescriptorParser::new(RealSys);
    let opts = deno_runtime::deno_permissions::PermissionsOptions {
        allow_read: Some(vec!["/".to_string()]),
        allow_net: Some(Vec::new()),
        prompt: false,
        ..Default::default()
    };
    let perms = Permissions::from_options(&parser, &opts)
        .map_err(|err| JsRuntimeError::Runtime(err.to_string()))?;
    Ok(PermissionsContainer::new(Arc::new(parser), perms))
}

fn deny_dns_resolver() -> deno_fetch::dns::Resolver {
    deno_fetch::dns::Resolver::Custom(Arc::new(DenyDnsResolver))
}

#[derive(Debug)]
struct DenyDnsResolver;

impl deno_fetch::dns::Resolve for DenyDnsResolver {
    fn resolve(&self, _name: Name) -> deno_fetch::dns::Resolving {
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "dns resolution is not permitted",
            ))
        })
    }
}

fn build_node_services(
    webc_fs: SharedFs,
) -> Result<
    NodeExtInitServices<
        DenoInNpmPackageChecker,
        ByonmNpmResolver<fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>>,
        fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>,
    >,
    JsRuntimeError,
> {
    let sys = fs::FsBridge::new(webc_fs.clone());
    let pkg_json_resolver: PackageJsonResolverRc<fs::FsBridge<Arc<fs::FsBridgeState<SharedFs>>>> =
        MaybeArc::new(PackageJsonResolver::new(sys.clone(), None));
    let node_resolver_sys = node_resolver::cache::NodeResolutionSys::new(sys.clone(), None);
    let npm_resolver = ByonmNpmResolver::new(ByonmNpmResolverCreateOptions {
        root_node_modules_dir: Some(PathBuf::from("/node_modules")),
        sys: node_resolver_sys.clone(),
        pkg_json_resolver: pkg_json_resolver.clone(),
    });
    let in_npm_pkg_checker = DenoInNpmPackageChecker::new(CreateInNpmPkgCheckerOptions::Byonm);
    let node_resolver = deno_runtime::deno_node::NodeResolver::new(
        in_npm_pkg_checker.clone(),
        DenoIsBuiltInNodeModuleChecker,
        npm_resolver.clone(),
        pkg_json_resolver.clone(),
        node_resolver_sys,
        NodeResolverOptions::default(),
    );
    let node_require_loader: NodeRequireLoaderRc = std::rc::Rc::new(fs::FsNodeRequireLoader {
        bridge: sys.clone(),
        pkg_json_resolver: pkg_json_resolver.clone(),
    });
    Ok(NodeExtInitServices {
        node_require_loader,
        node_resolver: MaybeArc::new(node_resolver),
        pkg_json_resolver,
        sys,
    })
}
