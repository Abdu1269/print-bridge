use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};

/// Entry-point arguments for headless product.
#[derive(Debug, Parser)]
#[command(name = "print-bridge", about = "PrintBridge headless Agent")]
pub struct ServerArgs {
    #[command(subcommand)]
    pub command: Option<ServerCommand>,
}

/// Process commands provided exclusively by headless product.
#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Run PrintBridge Agent in foreground.
    Serve,
}

impl ServerArgs {
    /// Parse testable arguments, generating help when no subcommand is provided.
    pub fn try_parse_product_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let parsed = Self::try_parse_from(args);
        match parsed {
            Ok(args) if args.command.is_none() => {
                Err(Self::command().error(ErrorKind::DisplayHelp, Self::command().render_help()))
            }
            result => result,
        }
    }
}
