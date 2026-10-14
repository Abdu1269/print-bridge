use async_trait::async_trait;

use crate::{CommandError, CommandErrorKind, ProductKind};

/// Desktop integration capabilities implemented by specific products.
// async-trait 0.1.89 generates #[must_use] for Future return values, triggering duplicate attribute checks on Rust 1.99.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ProductCommandAdapter: Send + Sync {
    /// Return product type of current executable.
    fn product_kind(&self) -> ProductKind {
        ProductKind::Desktop
    }
    /// Return whether system autostart is currently enabled.
    async fn autostart_status(&self) -> Result<bool, CommandError>;

    /// Enable or disable system autostart.
    async fn set_autostart(&self, enabled: bool) -> Result<(), CommandError>;

    /// Set desktop application UI language.
    async fn set_language(&self, language: &str) -> Result<(), CommandError>;
}

/// Return stable error for products that do not support desktop integration capabilities.
pub struct UnsupportedProductCommandAdapter {
    reason: &'static str,
    kind: ProductKind,
}

impl UnsupportedProductCommandAdapter {
    /// Create adapter for unsupported product capabilities.
    pub fn new(reason: &'static str) -> Self {
        Self {
            reason,
            kind: ProductKind::Desktop,
        }
    }

    /// Create unsupported adapter for Headless product.
    pub fn headless(reason: &'static str) -> Self {
        Self {
            reason,
            kind: ProductKind::Headless,
        }
    }

    fn error(&self) -> CommandError {
        CommandError::new(CommandErrorKind::Unsupported, self.reason)
    }
}

#[async_trait]
impl ProductCommandAdapter for UnsupportedProductCommandAdapter {
    fn product_kind(&self) -> ProductKind {
        self.kind
    }
    async fn autostart_status(&self) -> Result<bool, CommandError> {
        Err(self.error())
    }

    async fn set_autostart(&self, _enabled: bool) -> Result<(), CommandError> {
        Err(self.error())
    }

    async fn set_language(&self, _language: &str) -> Result<(), CommandError> {
        Err(self.error())
    }
}
