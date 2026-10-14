use crate::{
    config::AgentConfig,
    html::{browser::BrowserHtmlRenderer, resource_policy::ResourcePolicy, HtmlRenderer},
    printing::{self, PrintBackend},
    remote_store::RemoteStore,
    state::AgentState,
    task_history::TaskHistoryStore,
    AgentRuntime, RuntimeError,
};
use std::{path::PathBuf, sync::Arc};

/// File locations for Agent configuration, persistent data, and in-process runtime files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl RuntimePaths {
    /// Construct path set using configuration file and data directories.
    pub fn new(config_path: PathBuf, data_dir: PathBuf, runtime_dir: PathBuf) -> Self {
        Self {
            config_path,
            data_dir,
            runtime_dir,
        }
    }

    /// Return remote task database path.
    pub fn remote_store_path(&self) -> PathBuf {
        self.data_dir.join("remote.sqlite3")
    }

    /// Return task history database path.
    pub fn task_history_path(&self) -> PathBuf {
        self.data_dir.join("task_history.sqlite3")
    }
}

/// Assemble platform implementations and persistent paths required by Agent runtime.
pub struct RuntimeBuilder {
    paths: RuntimePaths,
    printing: Option<Box<dyn PrintBackend + Send + Sync>>,
    html_renderer: Option<Arc<dyn HtmlRenderer>>,
}

impl RuntimeBuilder {
    /// Begin assembly with explicit configuration, data, and runtime directories.
    pub fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            printing: None,
            html_renderer: None,
        }
    }

    /// Inject platform printing backend selected for current product.
    pub fn print_backend(mut self, printing: Box<dyn PrintBackend + Send + Sync>) -> Self {
        self.printing = Some(printing);
        self
    }

    /// Inject HTML renderer, allowing tests and platform products to override default implementation.
    pub fn html_renderer(mut self, renderer: Arc<dyn HtmlRenderer>) -> Self {
        self.html_renderer = Some(renderer);
        self
    }

    /// Create directories, load configuration, and open persistent stores.
    pub fn build(self) -> Result<AgentRuntime, RuntimeError> {
        std::fs::create_dir_all(&self.paths.data_dir)?;
        std::fs::create_dir_all(&self.paths.runtime_dir)?;
        if let Some(parent) = self.paths.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let config = AgentConfig::load(&self.paths.config_path)?;
        let remote_store = RemoteStore::open(&self.paths.remote_store_path())?;
        let task_history = TaskHistoryStore::open(&self.paths.task_history_path())?;
        let printing = self.printing.unwrap_or_else(printing::default_backend);
        let html_renderer = self.html_renderer.unwrap_or_else(|| {
            Arc::new(BrowserHtmlRenderer::new(ResourcePolicy::system())) as Arc<dyn HtmlRenderer>
        });
        let state = AgentState::with_config_path_printing_and_html_renderer(
            config,
            self.paths.config_path.clone(),
            printing,
            html_renderer,
        )
        .with_remote_store(remote_store)
        .with_task_history_store(task_history);

        Ok(AgentRuntime::new(self.paths, state))
    }
}
