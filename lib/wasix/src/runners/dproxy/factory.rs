use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    task::Context,
    time::Instant,
};

use hyper_util::rt::TokioExecutor;
use wasmer_journal::{DynJournal, RecombinedJournal};

use crate::{
    runners::Runner,
    runtime::{DynRuntime, OverriddenRuntime},
};

use super::{
    handler::Handler, hyper_proxy::HyperProxyConnectorBuilder, instance::DProxyInstance,
    networking::LocalWithLoopbackNetworking, shard::Shard, socket_manager::SocketManager,
};

#[derive(Debug, Default)]
struct State {
    instance: HashMap<Shard, DProxyInstance>,
}

/// This factory will store and reuse instances between invocations thus
/// allowing for the instances to be stateful.
#[derive(Debug, Clone, Default)]
pub struct DProxyInstanceFactory {
    state: Arc<Mutex<State>>,
}

impl DProxyInstanceFactory {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn acquire(&self, handler: &Handler, shard: Shard) -> anyhow::Result<DProxyInstance> {
        loop {
            {
                let state = self.state.lock().unwrap();
                if let Some(instance) = state.instance.get(&shard).cloned() {
                    return Ok(instance);
                }
            }

            let instance = self.spin_up(handler, shard.clone()).await?;

            let mut state = self.state.lock().unwrap();
            state.instance.insert(shard.clone(), instance);
        }
    }

    pub async fn spin_up(&self, handler: &Handler, shard: Shard) -> anyhow::Result<DProxyInstance> {
        // Get the runtime with its already wired local networking
        let runtime = handler.runtime.clone();

        // DProxy is able to resume execution of the stateful workload using memory
        // snapshots hence the journals it stores are complete journals
        let journals = runtime
            .writable_journals()
            .map(|journal| {
                let rx = journal.as_restarted()?;
                let combined = RecombinedJournal::new(journal, rx);
                anyhow::Result::Ok(Arc::new(combined) as Arc<DynJournal>)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut runtime = OverriddenRuntime::new(runtime).with_writable_journals(journals);

        // We attach a composite networking to the runtime which includes a loopback
        // networking implementation connected to a socket manager
        let composite_networking = LocalWithLoopbackNetworking::new();
        let poll_listening = {
            let networking = composite_networking.clone();
            Arc::new(move |cx: &mut Context<'_>| networking.poll_listening(cx))
        };
        let socket_manager = Arc::new(SocketManager::new(
            poll_listening,
            composite_networking.loopback_networking(),
            handler.config.proxy_connect_init_timeout,
            handler.config.proxy_connect_nominal_timeout,
        ));
        runtime = runtime.with_networking(Arc::new(composite_networking));

        // The connector uses the socket manager to open sockets to the instance
        let connector = HyperProxyConnectorBuilder::new(socket_manager.clone())
            .build()
            .await;

        // Now we run the actual instance under a WasiRunner
        #[cfg(feature = "sys")]
        let handle = tokio::runtime::Handle::current();
        let this = self.clone();
        let pkg = handler.config.pkg.clone();
        let command_name = handler.command_name.clone();
        let connector_inner = connector.clone();
        let runtime = Arc::new(runtime) as Arc<DynRuntime>;
        let mut runner = handler.config.inner.clone();
        runtime
            .task_manager()
            .clone()
            .task_dedicated(Box::new(move || {
                #[cfg(feature = "sys")]
                let _guard = handle.enter();
                if let Err(err) = runner.run_command(&command_name, &pkg, runtime) {
                    tracing::error!("Instance Exited: {}", err);
                } else {
                    tracing::info!("Instance Exited: Nominal");
                }
                {
                    let mut state = this.state.lock().unwrap();
                    state.instance.remove(&shard);
                }
                connector_inner.shutdown();
            }))?;

        // Return an instance
        Ok(DProxyInstance {
            last_used: Arc::new(Mutex::new(Instant::now())),
            socket_manager,
            client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(connector),
        })
    }
}
