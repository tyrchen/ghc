//! Version command implementation.

use clap::Args;

use ghc_core::ios_println;
use ghc_core::iostreams::IOStreams;

/// Show GHC version information.
#[derive(Debug, Args)]
pub struct VersionArgs {}

impl VersionArgs {
    /// Run the version command.
    pub fn run(&self, ios: &IOStreams, version: &str, build_date: &str) {
        if build_date.is_empty() {
            ios_println!(ios, "ghc version {version}");
        } else {
            ios_println!(ios, "ghc version {version} ({build_date})");
        }
    }
}

/// Format version info for display.
pub fn format_version(version: &str, build_date: &str) -> String {
    if build_date.is_empty() {
        format!("ghc version {version}")
    } else {
        format!("ghc version {version} ({build_date})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::iostreams::IOStreams;

    #[test]
    fn test_should_format_version_without_build_date() {
        assert_eq!(format_version("0.1.0", ""), "ghc version 0.1.0");
    }

    #[test]
    fn test_should_format_version_with_build_date() {
        assert_eq!(
            format_version("0.1.0", "2024-01-15"),
            "ghc version 0.1.0 (2024-01-15)",
        );
    }

    #[test]
    fn test_should_print_version_to_stdout() {
        let (ios, output) = IOStreams::test_with_output();
        let args = VersionArgs {};
        args.run(&ios, "1.2.3", "");
        assert_eq!(output.stdout(), "ghc version 1.2.3\n");
    }

    #[test]
    fn test_should_print_version_with_date_to_stdout() {
        let (ios, output) = IOStreams::test_with_output();
        let args = VersionArgs {};
        args.run(&ios, "1.2.3", "2024-06-01");
        assert_eq!(output.stdout(), "ghc version 1.2.3 (2024-06-01)\n");
    }
}
