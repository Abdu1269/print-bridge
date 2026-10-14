use std::path::PathBuf;

use print_bridge_core::config::AgentConfig;
use serde::{Deserialize, Serialize};

use crate::{config_transfer::ExportConfigOptions, CommandPolicy, ProductKind};

/// Functional commands executable via CLI, desktop IPC, or local Agent IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum Command {
    GetConfig,
    SaveConfig(AgentConfig),
    ListPrinters,
    ListPapers {
        printer_name: String,
    },
    GetLogs,
    GetTaskHistory,
    GetTaskHistoryEvents {
        job_id: String,
    },
    ClearTaskHistory,
    ExportConfig {
        path: PathBuf,
        password: String,
        options: ExportConfigOptions,
    },
    PreviewConfigImport {
        path: PathBuf,
        password: String,
    },
    ImportConfig {
        path: PathBuf,
        password: String,
        expected_file_hash: String,
    },
    TestRemoteConnection {
        config: AgentConfig,
    },
    TestPrint {
        config: AgentConfig,
    },
    ValidateConfig {
        path: Option<PathBuf>,
    },
    Doctor {
        product: ProductKind,
    },
    Status,
}

impl Command {
    /// Return dependency policy of command on running Agent.
    pub fn policy(&self) -> CommandPolicy {
        match self {
            Self::GetLogs
            | Self::TestRemoteConnection { .. }
            | Self::TestPrint { .. }
            | Self::Status => CommandPolicy::OnlineOnly,
            Self::GetConfig
            | Self::SaveConfig(_)
            | Self::ListPrinters
            | Self::ListPapers { .. }
            | Self::GetTaskHistory
            | Self::GetTaskHistoryEvents { .. }
            | Self::ClearTaskHistory
            | Self::ExportConfig { .. }
            | Self::PreviewConfigImport { .. }
            | Self::ImportConfig { .. }
            | Self::Doctor { .. } => CommandPolicy::OnlinePreferred,
            Self::ValidateConfig { .. } => CommandPolicy::OfflineAllowed,
        }
    }
}
