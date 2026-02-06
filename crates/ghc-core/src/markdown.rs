//! Markdown rendering for terminal output.
//!
//! Maps from Go's usage of glamour for markdown rendering.

/// Render markdown text for terminal display.
pub fn render(text: &str, width: usize) -> String {
    // Use termimad for terminal markdown rendering
    let skin = termimad::MadSkin::default();
    let area = termimad::Area::new(0, 0, u16::try_from(width).unwrap_or(u16::MAX), u16::MAX);
    let fmt = termimad::FmtText::from(&skin, text, Some(area.width as usize));
    fmt.to_string()
}

/// Render markdown to plain text (strip formatting).
pub fn render_plain(text: &str) -> String {
    // Simple stripping of common markdown syntax
    let mut result = text.to_string();
    // Remove headers
    result = regex::Regex::new(r"(?m)^#{1,6}\s+")
        .unwrap_or_else(|_| unreachable!())
        .replace_all(&result, "")
        .to_string();
    // Remove bold/italic
    result = result.replace("**", "").replace("__", "");
    result = regex::Regex::new(r"(?m)\*([^*]+)\*")
        .unwrap_or_else(|_| unreachable!())
        .replace_all(&result, "$1")
        .to_string();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_render_plain_strips_headers() {
        let plain = render_plain("# Hello");
        assert!(!plain.starts_with('#'));
        assert!(plain.contains("Hello"));
    }

    #[test]
    fn test_should_render_plain_strips_bold() {
        let plain = render_plain("**bold text**");
        assert!(!plain.contains("**"));
        assert!(plain.contains("bold text"));
    }

    #[test]
    fn test_should_render_plain_strips_underscore_bold() {
        let plain = render_plain("__underscored__");
        assert!(!plain.contains("__"));
    }

    #[test]
    fn test_should_render_plain_strips_italic() {
        let plain = render_plain("*italic*");
        assert!(plain.contains("italic"));
    }

    #[test]
    fn test_should_render_plain_combined() {
        let plain = render_plain("# Hello **World**");
        assert!(plain.contains("Hello"));
        assert!(plain.contains("World"));
        assert!(!plain.contains('#'));
        assert!(!plain.contains("**"));
    }

    #[test]
    fn test_should_render_plain_empty_string() {
        let plain = render_plain("");
        assert_eq!(plain, "");
    }

    #[test]
    fn test_should_render_markdown_returns_string() {
        let output = render("Hello world", 80);
        assert!(output.contains("Hello"));
    }

    #[test]
    fn test_should_render_markdown_with_small_width() {
        let output = render("Hello", 10);
        assert!(output.contains("Hello"));
    }
}
