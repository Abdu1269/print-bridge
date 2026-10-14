use std::net::SocketAddr;

use print_bridge_core::{
    activity::{TaskHistoryEvent, TaskHistoryJob, TaskLogEntry},
    config::AgentConfig,
    printing::{PaperInfo, PrinterInfo},
};
use serde::{Deserialize, Serialize};

use crate::config_transfer::ImportPreview;

/// Stable categories for command failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorKind {
    InvalidInput,
    NotRunning,
    PermissionDenied,
    Conflict,
    Runtime,
    Unsupported,
}

/// Command errors passable across CLI and IPC boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub message: String,
}

impl CommandError {
    /// Create command error with stable categorization.
    pub fn new(kind: CommandErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Return stable process exit code used by functional CLI.
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            CommandErrorKind::InvalidInput => 2,
            CommandErrorKind::NotRunning => 3,
            CommandErrorKind::PermissionDenied => 4,
            CommandErrorKind::Conflict => 5,
            CommandErrorKind::Runtime | CommandErrorKind::Unsupported => 1,
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

/// Agent running status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    pub running: bool,
    pub listen_addr: Option<SocketAddr>,
}

/// Product type providing the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    Desktop,
    Headless,
}

/// Status of individual Doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

/// Result of single read-only Doctor check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub code: String,
    pub status: DoctorStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// Aggregate counts for Doctor checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorSummary {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
}

/// Stable structured report for Doctor diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub summary: DoctorSummary,
}

impl DoctorReport {
    /// Calculate summary counts from check items.
    pub fn new(checks: Vec<DoctorCheck>) -> Self {
        let mut summary = DoctorSummary {
            pass: 0,
            warn: 0,
            fail: 0,
        };
        for check in &checks {
            match check.status {
                DoctorStatus::Pass => summary.pass += 1,
                DoctorStatus::Warn => summary.warn += 1,
                DoctorStatus::Fail => summary.fail += 1,
            }
        }
        Self { checks, summary }
    }
}

/// Structured result of functional commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "payload", rename_all = "snake_case")]
pub enum CommandResult {
    Empty,
    Config(Box<AgentConfig>),
    Printers(Vec<PrinterInfo>),
    Papers(Vec<PaperInfo>),
    Logs(Vec<TaskLogEntry>),
    TaskHistory(Vec<TaskHistoryJob>),
    TaskHistoryEvents(Vec<TaskHistoryEvent>),
    ImportPreview(ImportPreview),
    Doctor(DoctorReport),
    Status(AgentStatus),
}
