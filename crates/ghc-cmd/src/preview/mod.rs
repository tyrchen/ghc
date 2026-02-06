//! Preview command (`ghc preview`).
//!
//! Manage feature previews.

use anyhow::{Context, Result};
use clap::Subcommand;
use ghc_core::ios_eprintln;

/// Manage feature previews.
#[derive(Debug, Subcommand)]
pub enum PreviewCommand {
    /// List available feature previews.
    List(ListArgs),
    /// Enable a feature preview.
    Enable(ToggleArgs),
    /// Disable a feature preview.
    Disable(ToggleArgs),
}

impl PreviewCommand {
    /// Run the preview subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        match self {
            Self::List(args) => args.run(factory).await,
            Self::Enable(args) => args.run(factory, true).await,
            Self::Disable(args) => args.run(factory, false).await,
        }
    }
}

/// List available feature previews.
#[derive(Debug, clap::Args)]
pub struct ListArgs;

impl ListArgs {
    #[allow(clippy::unused_async)]
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let cfg_lock = factory.config().context("failed to load config")?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock poisoned: {e}"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{}", cs.bold("Feature Previews"));

        let previews = cfg.get("", "feature_previews");
        match previews {
            Some(val) if !val.is_empty() => {
                ios_eprintln!(ios, "  {val}");
            }
            _ => {
                ios_eprintln!(ios, "  No feature previews currently available");
            }
        }

        Ok(())
    }
}

/// Enable or disable a feature preview.
#[derive(Debug, clap::Args)]
pub struct ToggleArgs {
    /// Name of the feature preview.
    #[arg(value_name = "FEATURE")]
    feature: String,
}

impl ToggleArgs {
    #[allow(clippy::unused_async)]
    async fn run(&self, factory: &crate::factory::Factory, enable: bool) -> Result<()> {
        let cfg_lock = factory.config().context("failed to load config")?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock poisoned: {e}"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        let value = if enable { "true" } else { "false" };
        let key = format!("preview.{}", self.feature);
        cfg.set("", &key, value)?;

        if enable {
            ios_eprintln!(
                ios,
                "{} Feature preview '{}' enabled",
                cs.success_icon(),
                self.feature
            );
        } else {
            ios_eprintln!(
                ios,
                "{} Feature preview '{}' disabled",
                cs.success_icon(),
                self.feature
            );
        }

        Ok(())
    }
}
