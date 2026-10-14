pub mod browser;

pub mod resource_policy;

pub mod proxy;

use crate::protocol::EffectivePaper;
use std::{future::Future, path::PathBuf, pin::Pin};
use thiserror::Error;
use url::Url;

/// Source of HTML rendered by browser.
#[derive(Debug, Clone)]
pub enum HtmlSource {
    Url(Url),
    Inline(String),
}

/// Input required by browser to render HTML.
#[derive(Debug, Clone)]
pub struct HtmlRenderRequest {
    pub source: HtmlSource,
    /// Allowable local HTML address sharing origin with submitting page.
    pub allowed_loopback_origin: Option<Url>,
    pub paper: EffectivePaper,
    pub wait_ms: u64,
    pub output_path: PathBuf,
}

/// PDF result upon successful HTML rendering.
#[derive(Debug, Clone)]
pub struct HtmlRenderResult {
    pub renderer: &'static str,
    pub output_path: PathBuf,
}

/// Async result of HTML rendering.
pub type HtmlRenderFuture =
    Pin<Box<dyn Future<Output = Result<HtmlRenderResult, HtmlRenderError>> + Send>>;

/// Backend rendering restricted HTML sources into PDF.
pub trait HtmlRenderer: Send + Sync {
    fn render(&self, request: HtmlRenderRequest) -> HtmlRenderFuture;
}

/// Errors occurring during HTML rendering phase.
#[derive(Debug, Error)]
pub enum HtmlRenderError {
    #[error("no installed Chromium-family browser is available; searched: {searched:?}")]
    RendererUnavailable { searched: Vec<String> },
    #[error("blocked HTML resource: {resource}")]
    BlockedResource { resource: String },
    #[error("browser navigation failed: {message}")]
    Navigation { message: String },
    #[error("browser rendering timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("browser PDF export failed: {message}")]
    PdfExport { message: String },
    #[error("invalid proxy request: {reason}")]
    InvalidProxyRequest { reason: String },
    #[error("HTML proxy I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTML proxy HTTP error: {0}")]
    Http(#[from] hyper::Error),
}
