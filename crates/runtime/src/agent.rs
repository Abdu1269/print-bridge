use crate::{
    agent_guard::{check_agent_port, AgentPortStatus, RunningAgent},
    builder::RuntimePaths,
    ipc, queue, remote_worker, server,
    state::AgentState,
    RuntimeCommandExecutor,
};
use std::{io, net::SocketAddr, sync::Arc};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type RuntimeTasks = (
    JoinHandle<Result<(), server::ServerError>>,
    JoinHandle<()>,
    JoinHandle<()>,
    JoinHandle<Result<(), io::Error>>,
);

/// Failure during Agent runtime assembly, startup, or shutdown.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Store(#[from] rusqlite::Error),
    #[error(transparent)]
    Server(#[from] server::ServerError),
    #[error("{}", crate::agent_guard::already_running_message(.0))]
    AlreadyRunning(RunningAgent),
    #[error("local service port is already occupied at {0}")]
    PortOccupied(SocketAddr),
    #[error("runtime task failed: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}

/// Agent runtime that has not yet started.
pub struct AgentRuntime {
    paths: RuntimePaths,
    state: AgentState,
}

impl AgentRuntime {
    pub(crate) fn new(paths: RuntimePaths, state: AgentState) -> Self {
        Self { paths, state }
    }

    /// Return paths used by runtime.
    pub fn paths(&self) -> &RuntimePaths {
        &self.paths
    }

    /// Return state available for offline commands before startup.
    pub fn state(&self) -> AgentState {
        self.state.clone()
    }

    /// Bind WebSocket listener and start queue and remote workers.
    pub async fn start(self) -> Result<AgentHandle, RuntimeError> {
        let config = self.state.config.read().await.clone();
        match check_agent_port(&config) {
            AgentPortStatus::Available => {}
            AgentPortStatus::PrintBridge(agent) => {
                return Err(RuntimeError::AlreadyRunning(agent));
            }
            AgentPortStatus::OccupiedByOther { addr } => {
                return Err(RuntimeError::PortOccupied(addr));
            }
        }

        let (listen_addr, listener) = server::bind_listener(&config).await?;
        let shutdown = CancellationToken::new();
        let command_executor =
            Arc::new(RuntimeCommandExecutor::new(self.state.clone(), listen_addr));
        let ipc_task = ipc::bind_and_spawn(
            ipc::socket_path(&self.paths.runtime_dir),
            command_executor,
            shutdown.child_token(),
        )?;
        let server_task = tokio::spawn(server::serve_listener_until(
            self.state.clone(),
            listener,
            shutdown.child_token(),
        ));
        let queue_task = tokio::spawn(queue::run_worker_until(
            self.state.clone(),
            shutdown.child_token(),
        ));
        let remote_task = tokio::spawn(remote_worker::run_worker_until(
            self.state.clone(),
            shutdown.child_token(),
        ));

        Ok(AgentHandle {
            state: self.state,
            listen_addr,
            shutdown,
            tasks: tokio::sync::Mutex::new(Some((server_task, queue_task, remote_task, ipc_task))),
        })
    }
}

/// Running Agent instance and its unified shutdown handle.
pub struct AgentHandle {
    state: AgentState,
    listen_addr: SocketAddr,
    shutdown: CancellationToken,
    tasks: tokio::sync::Mutex<Option<RuntimeTasks>>,
}

impl AgentHandle {
    /// Return state shareable with GUI and command services.
    pub fn state(&self) -> AgentState {
        self.state.clone()
    }

    /// Return actual bound address; returns OS-assigned address even when port is 0.
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// Notify all background tasks to stop and wait for cleanup to complete.
    pub async fn shutdown(&self) -> Result<(), RuntimeError> {
        self.shutdown.cancel();
        let Some((server_task, queue_task, remote_task, ipc_task)) = self.tasks.lock().await.take()
        else {
            return Ok(());
        };
        queue_task.await?;
        remote_task.await?;
        ipc_task.await??;
        server_task.await??;
        Ok(())
    }
}
