//! Feature detection for GitHub API capabilities.
//!
//! Maps from Go's `internal/featuredetection` package.

/// Detected API features for a GitHub instance.
#[derive(Debug, Default, Clone)]
pub struct Features {
    /// Whether merge queue is supported.
    pub merge_queue: bool,
    /// Whether Projects V2 is supported.
    pub projects_v2: bool,
    /// Whether autolink references are supported.
    pub autolinks: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_default_all_features_false() {
        let features = Features::default();
        assert!(!features.merge_queue);
        assert!(!features.projects_v2);
        assert!(!features.autolinks);
    }

    #[test]
    fn test_should_clone_features() {
        let features = Features {
            merge_queue: true,
            projects_v2: true,
            autolinks: false,
        };
        let cloned = features.clone();
        assert!(cloned.merge_queue);
        assert!(cloned.projects_v2);
        assert!(!cloned.autolinks);
    }
}
