//! Core types, traits, and utilities for the GHC GitHub CLI.
//!
//! This crate provides the foundational abstractions used across all GHC crates:
//! - [`IOStreams`] for terminal I/O handling
//! - [`Config`] trait for configuration management
//! - [`AuthConfig`] trait for authentication state
//! - [`Prompter`] trait for interactive prompts
//! - Text utilities, table formatting, and color schemes

pub mod browser;
pub mod cmdutil;
pub mod config;
pub mod errors;
pub mod export;
pub mod instance;
pub mod iostreams;
pub mod json;
pub mod keyring_store;
pub mod markdown;
pub mod prompter;
pub mod repo;
pub mod table;
#[cfg(test)]
pub mod test_utils;
pub mod text;

pub use errors::CoreError;
pub use iostreams::IOStreams;
pub use repo::Repo;
