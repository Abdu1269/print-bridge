use crate::printing::{PaperInfo, PrinterInfo, PrinterMediaTypeInfo, PrinterTrayInfo};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use url::Url;

pub const DEFAULT_HTML_WAIT_MS: u64 = 1_000;
pub const MAX_HTML_WAIT_MS: u64 = 30_000;

/// File formats accepted for browser print tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportedFormat {
    Pdf,
    Image,
    Png,
    Jpg,
    Jpeg,
    Docx,
    Xlsx,
    Pptx,
    Raw,
    Html,
    #[serde(rename = "raw-html")]
    RawHtml,
}

/// Error returned when string cannot be parsed into a supported format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSupportedFormatError;

impl fmt::Display for ParseSupportedFormatError {
    /// Write stable human-readable parse error.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unsupported file format")
    }
}

impl std::error::Error for ParseSupportedFormatError {}

impl FromStr for SupportedFormat {
    type Err = ParseSupportedFormatError;

    /// Parse lowercase wire format names used by browser SDK.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pdf" => Ok(Self::Pdf),
            "image" => Ok(Self::Image),
            "png" => Ok(Self::Png),
            "jpg" => Ok(Self::Jpg),
            "jpeg" => Ok(Self::Jpeg),
            "docx" => Ok(Self::Docx),
            "xlsx" => Ok(Self::Xlsx),
            "pptx" => Ok(Self::Pptx),
            "raw" => Ok(Self::Raw),
            "html" => Ok(Self::Html),
            "raw-html" => Ok(Self::RawHtml),
            _ => Err(ParseSupportedFormatError),
        }
    }
}

/// Validation errors for browser Origin, file URL, and paper dimensions.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid origin")]
    InvalidOrigin,
    #[error("unsupported origin scheme")]
    UnsupportedOriginScheme,
    #[error("invalid file url")]
    InvalidFileUrl,
    #[error("unsupported file url scheme")]
    UnsupportedFileUrlScheme,
    #[error("paper dimensions must be positive")]
    InvalidPaperDimensions,
}

/// Error returned when print task field combinations violate protocol specifications.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JobValidationError {
    #[error("raw job requires data_base64")]
    MissingRawData,
    #[error("raw job does not accept file_url")]
    RawFileUrlNotAllowed,
    #[error("raw job does not accept paper")]
    RawPaperNotAllowed,
    #[error("raw job does not accept copies")]
    RawCopiesNotAllowed,
    #[error("file job requires file_url")]
    MissingFileUrl,
    #[error("file job does not accept data_base64")]
    FileRawDataNotAllowed,
    #[error("invalid raw data_base64")]
    InvalidRawData,
    #[error("file too large")]
    FileTooLarge,
    #[error("html job requires file_url")]
    MissingHtmlFileUrl,
    #[error("html job file_url must be an absolute http or https url")]
    InvalidHtmlFileUrl,
    #[error("html job does not accept inline html")]
    HtmlInlineNotAllowed,
    #[error("raw-html job requires html")]
    MissingRawHtml,
    #[error("raw-html job does not accept file_url")]
    RawHtmlFileUrlNotAllowed,
    #[error("html jobs do not accept data_base64")]
    HtmlDataBase64NotAllowed,
    #[error("non-html job does not accept html")]
    NonHtmlHtmlNotAllowed,
    #[error("non-html job does not accept wait_ms")]
    NonHtmlWaitNotAllowed,
    #[error("html wait_ms must be between 0 and 30000")]
    HtmlWaitOutOfRange,
}

/// Merged paper dimensions from task and configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectivePaper {
    pub width_mm: f64,
    pub height_mm: f64,
}

impl EffectivePaper {
    /// Ensure paper width and height are positive.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.width_mm > 0.0 && self.height_mm > 0.0 {
            Ok(())
        } else {
            Err(ProtocolError::InvalidPaperDimensions)
        }
    }
}

/// Single print task received from browser client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintJobInput {
    pub job_id: String,
    pub format: SupportedFormat,
    #[serde(default)]
    pub printer_name: Option<String>,
    #[serde(default)]
    pub file_url: Option<String>,
    #[serde(default)]
    pub data_base64: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    #[serde(default)]
    pub copies: Option<u16>,
    #[serde(default)]
    pub paper: Option<EffectivePaper>,
}

impl PrintJobInput {
    /// Validate whether task field combinations can be accepted into the queue.
    pub fn validate_for_acceptance(&self, max_file_size_mb: u64) -> Result<(), JobValidationError> {
        match self.format {
            SupportedFormat::Raw => self.validate_raw(max_file_size_mb),
            SupportedFormat::Pdf
            | SupportedFormat::Image
            | SupportedFormat::Png
            | SupportedFormat::Jpg
            | SupportedFormat::Jpeg
            | SupportedFormat::Docx
            | SupportedFormat::Xlsx
            | SupportedFormat::Pptx => self.validate_file_job(),
            SupportedFormat::Html => self.validate_html_file_job(),
            SupportedFormat::RawHtml => self.validate_raw_html(max_file_size_mb),
        }
    }

    fn validate_raw(&self, max_file_size_mb: u64) -> Result<(), JobValidationError> {
        if self.file_url.is_some() {
            return Err(JobValidationError::RawFileUrlNotAllowed);
        }
        if self.paper.is_some() {
            return Err(JobValidationError::RawPaperNotAllowed);
        }
        if self.copies.is_some() {
            return Err(JobValidationError::RawCopiesNotAllowed);
        }
        self.validate_non_html_fields()?;

        let data_base64 = self
            .data_base64
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(JobValidationError::MissingRawData)?;
        let bytes = STANDARD
            .decode(data_base64)
            .map_err(|_| JobValidationError::InvalidRawData)?;
        let max_bytes = max_file_size_mb.saturating_mul(1024 * 1024);
        if bytes.len() as u64 > max_bytes {
            return Err(JobValidationError::FileTooLarge);
        }

        Ok(())
    }

    fn validate_file_job(&self) -> Result<(), JobValidationError> {
        if self
            .file_url
            .as_deref()
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(JobValidationError::MissingFileUrl);
        }
        if self.data_base64.is_some() {
            return Err(JobValidationError::FileRawDataNotAllowed);
        }
        self.validate_non_html_fields()?;

        Ok(())
    }

    /// Validate that fields exclusive to HTML jobs do not appear in other formats.
    fn validate_non_html_fields(&self) -> Result<(), JobValidationError> {
        if self.html.is_some() {
            return Err(JobValidationError::NonHtmlHtmlNotAllowed);
        }
        if self.wait_ms.is_some() {
            return Err(JobValidationError::NonHtmlWaitNotAllowed);
        }

        Ok(())
    }

    /// Validate field combinations for URL-based HTML jobs.
    fn validate_html_file_job(&self) -> Result<(), JobValidationError> {
        let file_url = self
            .file_url
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(JobValidationError::MissingHtmlFileUrl)?;
        validate_html_file_url(file_url).map_err(|_| JobValidationError::InvalidHtmlFileUrl)?;
        if self.html.is_some() {
            return Err(JobValidationError::HtmlInlineNotAllowed);
        }
        if self.data_base64.is_some() {
            return Err(JobValidationError::HtmlDataBase64NotAllowed);
        }
        self.validate_html_wait()
    }

    /// Validate field combinations for inline HTML text jobs.
    fn validate_raw_html(&self, max_file_size_mb: u64) -> Result<(), JobValidationError> {
        if self.file_url.is_some() {
            return Err(JobValidationError::RawHtmlFileUrlNotAllowed);
        }
        if self.data_base64.is_some() {
            return Err(JobValidationError::HtmlDataBase64NotAllowed);
        }
        let html = self
            .html
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(JobValidationError::MissingRawHtml)?;
        let max_bytes = max_file_size_mb.saturating_mul(1024 * 1024);
        if html.len() as u64 > max_bytes {
            return Err(JobValidationError::FileTooLarge);
        }

        self.validate_html_wait()
    }

    /// Validate allowable wait time before HTML page rendering.
    fn validate_html_wait(&self) -> Result<(), JobValidationError> {
        if matches!(self.wait_ms, Some(wait_ms) if wait_ms > MAX_HTML_WAIT_MS) {
            return Err(JobValidationError::HtmlWaitOutOfRange);
        }

        Ok(())
    }
}

/// Browser client messages accepted by the WebSocket protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "ping")]
    Ping { time: i64 },
    #[serde(rename = "get_printers_list")]
    GetPrintersList { request_id: String },
    #[serde(rename = "get_printer_info")]
    GetPrinterInfo {
        request_id: String,
        printer_name: String,
    },
    #[serde(rename = "get_print_queue")]
    GetPrintQueue { request_id: String },
    #[serde(rename = "print")]
    Print {
        request_id: String,
        #[serde(flatten)]
        job: PrintJobInput,
    },
    #[serde(rename = "print_batch")]
    PrintBatch {
        request_id: String,
        batch_id: String,
        jobs: Vec<PrintJobInput>,
    },
}

/// Messages returned to browser clients by the local Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "pong")]
    Pong { time: i64, agent_status: String },
    #[serde(rename = "printers_list")]
    PrintersList {
        request_id: String,
        printers: Vec<PrinterInfo>,
    },
    #[serde(rename = "printer_info")]
    PrinterInfo {
        request_id: String,
        printer: PrinterDetails,
    },
    #[serde(rename = "print_queue")]
    PrintQueue {
        request_id: String,
        jobs: Vec<PrintQueueJobInfo>,
    },
    #[serde(rename = "job_status")]
    JobStatus {
        request_id: Option<String>,
        job_id: String,
        status: JobStatus,
        message: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        request_id: Option<String>,
        error_code: ErrorCode,
        message: String,
    },
}

/// WebSocket response body for a single printer and its available papers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrinterDetails {
    pub name: String,
    pub is_default: bool,
    pub dpi: Option<u32>,
    pub port: Option<String>,
    pub is_local: Option<bool>,
    pub is_network: Option<bool>,
    pub is_virtual: Option<bool>,
    pub papers: Vec<PaperInfo>,
    pub trays: Vec<PrinterTrayInfo>,
    pub media_types: Vec<PrinterMediaTypeInfo>,
}

/// Summary of tasks currently in the in-memory print queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintQueueJobInfo {
    pub request_id: String,
    pub batch_id: Option<String>,
    pub job_id: String,
    pub status: JobStatus,
    pub message: Option<String>,
}

/// Print task states exposed in protocol and local logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Downloading,
    Printing,
    Submitted,
    Completed,
    Failed,
    Unknown,
    Cancelled,
}

/// Protocol error codes serialized as SCREAMING_SNAKE_CASE strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    OriginNotAllowed,
    InvalidMessage,
    PrinterNotConfigured,
    PrinterNotFound,
    PaperNotConfigured,
    PaperNotFound,
    DownloadFailed,
    FileTooLarge,
    UnsupportedFormat,
    FormatMismatch,
    OfficeConvertFailed,
    PrintFailed,
    JobDuplicated,
    BatchDuplicated,
    BatchTooLarge,
    CopiesOutOfRange,
    ServicePortInUse,
    InternalError,
}

/// Check whether incoming WebSocket Origin strictly matches allowlist.
pub fn is_allowed_origin(origin: Option<&str>, allowed_origins: &[String]) -> bool {
    origin.is_some_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin))
}

/// Validate and normalize allowed browser Origin string.
pub fn validate_origin(value: &str) -> Result<(), ProtocolError> {
    let url = Url::parse(value).map_err(|_| ProtocolError::InvalidOrigin)?;

    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(ProtocolError::UnsupportedOriginScheme),
    }

    if url.origin().ascii_serialization() == value {
        Ok(())
    } else {
        Err(ProtocolError::InvalidOrigin)
    }
}

/// Validate that print file URL is downloadable via HTTP(S) or provided as inline PDF data URL.
pub fn validate_file_url(value: &str) -> Result<Url, ProtocolError> {
    let url = Url::parse(value).map_err(|_| ProtocolError::InvalidFileUrl)?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        "data" if is_pdf_data_url(value) => Ok(url),
        _ => Err(ProtocolError::UnsupportedFileUrlScheme),
    }
}

/// Validate that HTML page URL is an absolute HTTP(S) URL with authority.
///
/// Hostname and IP reachability are further evaluated during rendering by `ResourcePolicy`.
pub fn validate_html_file_url(value: &str) -> Result<Url, ProtocolError> {
    let url = Url::parse(value).map_err(|_| ProtocolError::InvalidFileUrl)?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(ProtocolError::UnsupportedFileUrlScheme);
    }
    if url.host_str().is_none() {
        return Err(ProtocolError::InvalidFileUrl);
    }

    Ok(url)
}

/// Determine whether file_url is a base64 PDF data URL accepted by PrintBridge.
pub fn is_pdf_data_url(value: &str) -> bool {
    let Some(payload) = value.strip_prefix("data:") else {
        return false;
    };
    let Some((metadata, data)) = payload.split_once(',') else {
        return false;
    };
    if data.is_empty() {
        return false;
    }

    let mut parts = metadata.split(';');
    let Some(media_type) = parts.next() else {
        return false;
    };

    media_type.eq_ignore_ascii_case("application/pdf")
        && parts.any(|part| part.eq_ignore_ascii_case("base64"))
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, JobValidationError, PrintJobInput, SupportedFormat};
    use std::str::FromStr;

    #[test]
    fn parses_office_supported_formats() {
        assert_eq!(
            SupportedFormat::from_str("docx").unwrap(),
            SupportedFormat::Docx
        );
        assert_eq!(
            SupportedFormat::from_str("xlsx").unwrap(),
            SupportedFormat::Xlsx
        );
        assert_eq!(
            SupportedFormat::from_str("pptx").unwrap(),
            SupportedFormat::Pptx
        );
    }

    #[test]
    fn serializes_office_convert_failed_error_code() {
        let value = serde_json::to_value(ErrorCode::OfficeConvertFailed).unwrap();
        assert_eq!(value, serde_json::json!("OFFICE_CONVERT_FAILED"));
    }

    #[test]
    fn raw_job_accepts_inline_base64_and_optional_printer_name() {
        let job: PrintJobInput = serde_json::from_str(
            r#"{
                "job_id":"RAW-001",
                "format":"raw",
                "printer_name":"Zebra ZD421",
                "data_base64":"XlhB"
            }"#,
        )
        .unwrap();

        assert_eq!(job.format, SupportedFormat::Raw);
        assert_eq!(job.printer_name.as_deref(), Some("Zebra ZD421"));
        assert_eq!(job.data_base64.as_deref(), Some("XlhB"));
        assert!(job.file_url.is_none());
        assert!(job.paper.is_none());
        assert!(job.copies.is_none());
        assert_eq!(job.validate_for_acceptance(20), Ok(()));
    }

    #[test]
    fn file_job_accepts_printer_name_and_existing_fields() {
        let job: PrintJobInput = serde_json::from_str(
            r#"{
                "job_id":"PDF-001",
                "format":"pdf",
                "printer_name":"Office Printer",
                "file_url":"https://example.com/a.pdf",
                "copies":2,
                "paper":{"width_mm":60,"height_mm":40}
            }"#,
        )
        .unwrap();

        assert_eq!(job.format, SupportedFormat::Pdf);
        assert_eq!(job.printer_name.as_deref(), Some("Office Printer"));
        assert_eq!(job.copies, Some(2));
        assert_eq!(job.validate_for_acceptance(20), Ok(()));
    }

    #[test]
    fn raw_job_rejects_file_fields() {
        let mut job = PrintJobInput {
            job_id: "RAW-INVALID".to_string(),
            format: SupportedFormat::Raw,
            printer_name: None,
            file_url: Some("https://example.com/raw.bin".to_string()),
            data_base64: Some("XlhB".to_string()),
            html: None,
            wait_ms: None,
            copies: None,
            paper: None,
        };

        assert_eq!(
            job.validate_for_acceptance(20),
            Err(JobValidationError::RawFileUrlNotAllowed)
        );

        job.file_url = None;
        job.copies = Some(1);
        assert_eq!(
            job.validate_for_acceptance(20),
            Err(JobValidationError::RawCopiesNotAllowed)
        );
    }
}
