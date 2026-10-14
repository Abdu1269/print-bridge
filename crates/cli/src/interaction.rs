use std::io::{self, IsTerminal, Write};

use crate::{CommandError, CommandErrorKind};

/// CLI password entry and confirmation interaction boundary.
pub trait CliInteraction: Send + Sync {
    /// Read non-echoing password input.
    fn read_password(&self, prompt: &str) -> Result<String, CommandError>;

    /// Request confirmation from user.
    fn confirm(&self, prompt: &str) -> Result<bool, CommandError>;

    /// Return whether interaction is currently possible.
    fn is_interactive(&self) -> bool;
}

/// Interaction implementation using current terminal standard I/O.
pub struct TerminalInteraction;

impl CliInteraction for TerminalInteraction {
    fn read_password(&self, prompt: &str) -> Result<String, CommandError> {
        rpassword::prompt_password(prompt).map_err(interaction_error)
    }

    fn confirm(&self, prompt: &str) -> Result<bool, CommandError> {
        eprint!("{prompt} [y/N] ");
        io::stderr().flush().map_err(interaction_error)?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(interaction_error)?;
        Ok(matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }

    fn is_interactive(&self) -> bool {
        io::stdin().is_terminal() && io::stderr().is_terminal()
    }
}

fn interaction_error(error: impl ToString) -> CommandError {
    CommandError::new(CommandErrorKind::Runtime, error.to_string())
}
