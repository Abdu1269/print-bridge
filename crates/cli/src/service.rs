use std::sync::Arc;

use async_trait::async_trait;

use crate::{Command, CommandError, CommandErrorKind, CommandPolicy, CommandResult};

/// Online or offline port for executing commands.
// async-trait 0.1.89 generates #[must_use] for Future return values, triggering duplicate attribute checks on Rust 1.99.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(&self, command: Command) -> Result<CommandResult, CommandError>;
}

/// Select online Agent or offline implementation according to command policy.
pub struct CommandService {
    online: Option<Arc<dyn CommandExecutor>>,
    offline: Arc<dyn CommandExecutor>,
}

impl CommandService {
    /// Create shared command service.
    pub fn new(
        online: Option<Arc<dyn CommandExecutor>>,
        offline: Arc<dyn CommandExecutor>,
    ) -> Self {
        Self { online, offline }
    }

    /// Execute according to command policy, falling back offline only for explicit NotRunning errors.
    pub async fn execute(&self, command: Command) -> Result<CommandResult, CommandError> {
        match command.policy() {
            CommandPolicy::OfflineAllowed => self.offline.execute(command).await,
            CommandPolicy::OnlineOnly => {
                let online = self.online.as_ref().ok_or_else(not_running_error)?;
                online.execute(command).await
            }
            CommandPolicy::OnlinePreferred => {
                let Some(online) = self.online.as_ref() else {
                    return self.offline.execute(command).await;
                };
                match online.execute(command.clone()).await {
                    Err(error) if error.kind == CommandErrorKind::NotRunning => {
                        self.offline.execute(command).await
                    }
                    result => result,
                }
            }
        }
    }
}

fn not_running_error() -> CommandError {
    CommandError::new(CommandErrorKind::NotRunning, "agent is not running")
}
