use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Printer summary returned by platform backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrinterInfo {
    pub name: String,
    pub is_default: bool,
    pub dpi: Option<u32>,
    pub port: Option<String>,
    pub is_local: Option<bool>,
    pub is_network: Option<bool>,
    pub is_virtual: Option<bool>,
}

impl PrinterInfo {
    /// Construct summary with only name and default status, leaving unknown platform fields empty.
    pub fn new(name: String, is_default: bool) -> Self {
        Self {
            name,
            is_default,
            dpi: None,
            port: None,
            is_local: None,
            is_network: None,
            is_virtual: None,
        }
    }
}

/// Paper dimensions supported by printer or custom fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperInfo {
    pub id: String,
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
}

/// Printer input tray or paper source option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterTrayInfo {
    pub id: String,
    pub name: String,
}

/// Printer media type option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterMediaTypeInfo {
    pub id: String,
    pub name: String,
}

/// Options required by backend to submit a single PDF print task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintOptions {
    pub printer_name: String,
    pub paper: PaperInfo,
    pub copies: u16,
}

/// Options required by backend to submit a single raw print task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPrintOptions {
    pub printer_name: String,
}

/// Trackable information after successful submission of platform print command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintSubmission {
    pub submitted_at: String,
    pub backend: String,
    pub system_job_id: Option<String>,
    pub tracking_supported: bool,
}

/// Conservative outcome from platform queue tracking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrintTrackingOutcome {
    Completed { message: String },
    Failed { message: String },
    Unknown { message: String },
}

/// Errors returned by platform printing backends.
#[derive(Debug, Error)]
pub enum PrintError {
    #[error("printer not found: {0}")]
    PrinterNotFound(String),
    #[error("paper not found: {0}")]
    PaperNotFound(String),
    #[error("print command failed: {command}: {message}")]
    CommandFailed { command: String, message: String },
    #[error("printing is not supported on this platform")]
    UnsupportedPlatform,
}

/// Unified result type returned by platform printing backends.
pub type PrintResult<T> = Result<T, PrintError>;

/// Platform printing abstraction used by queue worker and functional entry points.
pub trait PrintBackend {
    /// List installed printers.
    fn list_printers(&self) -> PrintResult<Vec<PrinterInfo>>;
    /// List paper sizes for specified printer.
    fn list_papers(&self, printer_name: &str) -> PrintResult<Vec<PaperInfo>>;
    /// List available paper trays for specified printer; returns empty list if platform does not support enumeration.
    fn list_trays(&self, _printer_name: &str) -> PrintResult<Vec<PrinterTrayInfo>> {
        Ok(Vec::new())
    }
    /// List available media types for specified printer; returns empty list if platform does not support enumeration.
    fn list_media_types(&self, _printer_name: &str) -> PrintResult<Vec<PrinterMediaTypeInfo>> {
        Ok(Vec::new())
    }
    /// Send PDF file to platform print queue.
    fn print_pdf(&self, path: &Path, options: &PrintOptions) -> PrintResult<PrintSubmission>;
    /// Send raw printer command bytes to platform print queue.
    fn print_raw(&self, data: &[u8], options: &RawPrintOptions) -> PrintResult<PrintSubmission>;
    /// Query conservative status from platform queue for this submission.
    fn track_submission(
        &self,
        _submission: &PrintSubmission,
        _options: &PrintOptions,
    ) -> PrintTrackingOutcome {
        PrintTrackingOutcome::Unknown {
            message: "platform does not provide trackable print status".to_string(),
        }
    }
    /// Query conservative status from platform queue for raw submission.
    fn track_raw_submission(
        &self,
        _submission: &PrintSubmission,
        _options: &RawPrintOptions,
    ) -> PrintTrackingOutcome {
        PrintTrackingOutcome::Unknown {
            message: "platform does not provide trackable raw print status".to_string(),
        }
    }
}
