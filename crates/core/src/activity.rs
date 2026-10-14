use serde::{Deserialize, Serialize};

use crate::protocol::JobStatus;

/// Task status record saved locally and broadcast via WebSocket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLogEntry {
    pub timestamp: String,
    pub request_id: Option<String>,
    pub batch_id: Option<String>,
    pub job_id: Option<String>,
    pub origin: Option<String>,
    pub status: JobStatus,
    pub message: String,
}

/// Task status in local task history records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskHistoryStatus {
    Queued,
    Downloading,
    Printing,
    Submitted,
    Completed,
    Failed,
    Unknown,
    Cancelled,
}

impl TaskHistoryStatus {
    /// Return status string used for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Printing => "printing",
            Self::Submitted => "submitted",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
            Self::Cancelled => "cancelled",
        }
    }

    /// Restore status from persisted string.
    pub fn from_storage_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "downloading" => Some(Self::Downloading),
            "printing" => Some(Self::Printing),
            "submitted" => Some(Self::Submitted),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "unknown" => Some(Self::Unknown),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Return whether this status concludes the task lifecycle.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Submitted | Self::Completed | Self::Failed | Self::Unknown | Self::Cancelled
        )
    }
}

/// Task source in local task history records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskHistorySource {
    WebSocket,
    Remote,
    Test,
}

impl TaskHistorySource {
    /// Return source string used for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebSocket => "web_socket",
            Self::Remote => "remote",
            Self::Test => "test",
        }
    }

    /// Restore source from persisted string.
    pub fn from_storage_str(value: &str) -> Option<Self> {
        match value {
            "web_socket" => Some(Self::WebSocket),
            "remote" => Some(Self::Remote),
            "test" => Some(Self::Test),
            _ => None,
        }
    }
}

/// Individual task summary in task history list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskHistoryJob {
    pub job_id: String,
    pub request_id: Option<String>,
    pub batch_id: Option<String>,
    pub source: TaskHistorySource,
    pub current_status: TaskHistoryStatus,
    pub current_message: Option<String>,
    pub printer_name: Option<String>,
    pub paper_name: Option<String>,
    pub copies: Option<u16>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

/// Individual status event in task history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskHistoryEvent {
    pub id: i64,
    pub job_id: String,
    pub status: TaskHistoryStatus,
    pub message: Option<String>,
    pub occurred_at: String,
}
