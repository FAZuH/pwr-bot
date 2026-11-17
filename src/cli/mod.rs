//! Unified command-line interface for pwr-bot.
//!
//! The command tree lives here; the binary owns the foreground `serve` path
//! and passes every other command to the modules in this directory.

pub mod log_tail;
pub mod plugin;
pub mod service;

use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
pub use plugin::PluginCommand;
pub use service::ServiceCommand;

#[derive(Debug, Parser)]
#[command(
    name = "pwr-bot",
    about = "Discord plugin host",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// Show the last N log lines before following (bare invocation)
    #[arg(short = 'n', long = "lines", value_name = "N")]
    pub lines: Option<usize>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the bot in the foreground
    Serve {
        /// Run as a detached, registered service
        #[arg(long)]
        service: bool,
    },
    /// Manage the external plugin catalog
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
    /// Manage the background service
    Service {
        #[command(subcommand)]
        action: ServiceCommand,
    },
    /// Print the package version
    Version,
    /// Print resolved paths
    Debug {
        #[command(subcommand)]
        target: DebugCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DebugCommand {
    /// Print config, data, log, and registration paths
    Paths,
}

/// Private config path shown by `debug paths`.
pub fn service_config_display() -> PathBuf {
    service::service_config_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_invocation_has_no_subcommand() {
        let cli = Cli::try_parse_from(["pwr-bot"]).expect("bare invocation must parse");
        assert!(cli.command.is_none());
        assert_eq!(cli.lines, None);
    }

    #[test]
    fn bare_invocation_accepts_lines() {
        let cli = Cli::try_parse_from(["pwr-bot", "-n", "25"]).expect("-n must parse");
        assert_eq!(cli.lines, Some(25));
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_serve_service() {
        let cli = Cli::try_parse_from(["pwr-bot", "serve", "--service"])
            .expect("serve --service must parse");
        assert!(matches!(
            cli.command,
            Some(Command::Serve { service: true })
        ));
    }

    #[test]
    fn parses_serve_without_service() {
        let cli = Cli::try_parse_from(["pwr-bot", "serve"]).expect("serve must parse");
        assert!(matches!(
            cli.command,
            Some(Command::Serve { service: false })
        ));
    }

    #[test]
    fn parses_plugin_add_flags() {
        let cli = Cli::try_parse_from([
            "pwr-bot",
            "plugin",
            "add",
            "--name",
            "feed",
            "--url",
            "https://example.test/feed",
            "--sha256",
            "abc123",
            "--manifest",
            "/tmp/manifest.json",
            "--grant-token",
            "--auto-enable",
        ])
        .expect("plugin add must parse");
        let Some(Command::Plugin {
            action:
                PluginCommand::Add {
                    name,
                    sha256,
                    grant_token,
                    auto_enable,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected plugin add");
        };
        assert_eq!(name, "feed");
        assert_eq!(sha256, "abc123");
        assert!(grant_token);
        assert!(auto_enable);
    }

    #[test]
    fn parses_service_install_env_file() {
        let cli = Cli::try_parse_from([
            "pwr-bot",
            "service",
            "install",
            "--env-file",
            "/etc/pwr-bot.env",
        ])
        .expect("service install must parse");
        let Some(Command::Service {
            action: ServiceCommand::Install { env_file },
        }) = cli.command
        else {
            panic!("expected service install");
        };
        assert_eq!(env_file, PathBuf::from("/etc/pwr-bot.env"));
    }

    #[test]
    fn parses_debug_paths_and_version() {
        let paths = Cli::try_parse_from(["pwr-bot", "debug", "paths"]).expect("debug paths parses");
        assert!(matches!(
            paths.command,
            Some(Command::Debug {
                target: DebugCommand::Paths
            })
        ));
        let version = Cli::try_parse_from(["pwr-bot", "version"]).expect("version parses");
        assert!(matches!(version.command, Some(Command::Version)));
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(Cli::try_parse_from(["pwr-bot", "frobnicate"]).is_err());
    }
}
