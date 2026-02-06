//! Accessibility command (`ghc accessibility`).
//!
//! Manage accessibility settings for the CLI.

use anyhow::{Context, Result};
use clap::Subcommand;
use ghc_core::ios_eprintln;

/// Manage accessibility settings.
#[derive(Debug, Subcommand)]
pub enum AccessibilityCommand {
    /// Show current accessibility settings.
    Status(StatusArgs),
    /// Enable or disable screen reader mode.
    #[command(name = "screen-reader")]
    ScreenReader(ScreenReaderArgs),
}

impl AccessibilityCommand {
    /// Run the accessibility subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        match self {
            Self::Status(args) => args.run(factory).await,
            Self::ScreenReader(args) => args.run(factory).await,
        }
    }
}

/// Show current accessibility settings.
#[derive(Debug, clap::Args)]
pub struct StatusArgs;

impl StatusArgs {
    #[allow(clippy::unused_async)]
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let cfg_lock = factory.config().context("failed to load config")?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock poisoned: {e}"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        let screen_reader = cfg.get("", "accessibility.screen_reader");
        let sr_status = match screen_reader.as_deref() {
            Some("true") => cs.success("enabled"),
            _ => "disabled".to_string(),
        };

        ios_eprintln!(ios, "{}", cs.bold("Accessibility Settings"));
        ios_eprintln!(ios, "  Screen reader mode: {sr_status}");

        Ok(())
    }
}

/// Enable or disable screen reader mode.
#[derive(Debug, clap::Args)]
pub struct ScreenReaderArgs {
    /// Enable screen reader mode.
    #[arg(long, group = "toggle")]
    enable: bool,

    /// Disable screen reader mode.
    #[arg(long, group = "toggle")]
    disable: bool,
}

impl ScreenReaderArgs {
    #[allow(clippy::unused_async)]
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let cfg_lock = factory.config().context("failed to load config")?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock poisoned: {e}"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        if self.enable {
            cfg.set("", "accessibility.screen_reader", "true")?;
            ios_eprintln!(ios, "{} Screen reader mode enabled", cs.success_icon());
        } else if self.disable {
            cfg.set("", "accessibility.screen_reader", "false")?;
            ios_eprintln!(ios, "{} Screen reader mode disabled", cs.success_icon());
        } else {
            anyhow::bail!("specify --enable or --disable");
        }

        Ok(())
    }
}
