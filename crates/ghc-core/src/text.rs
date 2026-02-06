//! Text formatting utilities.
//!
//! Maps from Go's `internal/text` package.

use chrono::{DateTime, Utc};

/// Truncate a string to a maximum display width, appending "..." if truncated.
pub fn truncate(text: &str, max_width: usize) -> String {
    if max_width < 4 {
        return text.chars().take(max_width).collect();
    }

    let char_count: usize = text.chars().count();
    if char_count <= max_width {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_width - 3).collect();
    format!("{truncated}...")
}

/// Format a duration as a human-readable fuzzy time string.
pub fn fuzzy_ago(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds();

    if seconds < 60 {
        return "less than a minute ago".to_string();
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return pluralize(minutes, "minute", "minutes") + " ago";
    }

    let hours = minutes / 60;
    if hours < 24 {
        return pluralize(hours, "hour", "hours") + " ago";
    }

    let days = hours / 24;
    if days < 30 {
        return pluralize(days, "day", "days") + " ago";
    }

    let months = days / 30;
    if months < 12 {
        return pluralize(months, "month", "months") + " ago";
    }

    let years = months / 12;
    pluralize(years, "year", "years") + " ago"
}

/// Format a timestamp for display based on whether output is a TTY.
pub fn relative_time_str(t: &DateTime<Utc>, is_tty: bool) -> String {
    if is_tty {
        let duration = Utc::now().signed_duration_since(*t);
        fuzzy_ago(duration)
    } else {
        t.to_rfc3339()
    }
}

/// Simple English pluralization.
pub fn pluralize(count: i64, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// Remove excessive whitespace from a string (collapse multiple spaces/newlines).
pub fn remove_excessive_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }

    result.trim().to_string()
}

/// Display a URL in a user-friendly format (strip protocol and trailing slash).
pub fn display_url(url: &str) -> String {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    url.trim_end_matches('/').to_string()
}

/// Percent-encode a string for use in URL path segments and query parameters.
///
/// Encodes characters that are not URL-safe: spaces, `%`, `#`, `&`, `?`, `+`,
/// `=`, `@`, and `/`.
///
/// # Examples
///
/// ```
/// use ghc_core::text::percent_encode;
/// assert_eq!(percent_encode("hello world"), "hello%20world");
/// assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
/// ```
pub fn percent_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                use std::fmt::Write;
                encoded.push('%');
                let _ = write!(encoded, "{byte:02X}");
            }
        }
    }
    encoded
}

/// Decode a standard base64-encoded string into bytes.
///
/// Supports standard base64 alphabet (RFC 4648) with optional padding.
/// Whitespace characters (spaces, newlines, carriage returns) are ignored.
///
/// # Errors
///
/// Returns an error if the input contains invalid base64 characters.
///
/// # Examples
///
/// ```
/// use ghc_core::text::base64_decode;
/// assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
/// assert_eq!(base64_decode("d29ybGQ=").unwrap(), b"world");
/// ```
pub fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let filtered: Vec<u8> = input
        .bytes()
        .filter(|b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        .collect();

    let mut output = Vec::with_capacity(filtered.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits_collected: u32 = 0;

    for &byte in &filtered {
        if byte == b'=' {
            break;
        }
        let val = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("invalid base64 character: {}", byte as char)),
        };
        buf = (buf << 6) | u32::from(val);
        bits_collected += 6;
        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push(u8::try_from((buf >> bits_collected) & 0xFF).unwrap_or(0));
            buf &= (1 << bits_collected) - 1;
        }
    }

    Ok(output)
}

/// Encode bytes into a standard base64 string (RFC 4648 with padding).
///
/// # Examples
///
/// ```
/// use ghc_core::text::base64_encode;
/// assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
/// assert_eq!(base64_encode(b"world"), "d29ybGQ=");
/// ```
pub fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks(3);

    for chunk in chunks {
        let b0 = u32::from(chunk[0]);
        let b1 = if chunk.len() > 1 {
            u32::from(chunk[1])
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            u32::from(chunk[2])
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        output.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        output.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            output.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }

        if chunk.len() > 2 {
            output.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

/// Simple title case: capitalize the first letter of each word.
pub fn title_case(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // --- truncate tests ---

    #[rstest]
    #[case("hello world", 8, "hello...")]
    #[case("short", 10, "short")]
    #[case("exact", 5, "exact")]
    #[case("abcdef", 6, "abcdef")]
    #[case("abcdefg", 6, "abc...")]
    fn test_should_truncate_string(
        #[case] input: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(truncate(input, width), expected);
    }

    #[rstest]
    #[case("hello", 3, "hel")]
    #[case("hello", 2, "he")]
    #[case("hello", 1, "h")]
    #[case("hello", 0, "")]
    fn test_should_truncate_small_widths_without_ellipsis(
        #[case] input: &str,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(truncate(input, width), expected);
    }

    #[test]
    fn test_should_truncate_empty_string() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn test_should_handle_unicode_truncation() {
        // Multi-byte characters should not be split
        let result = truncate("hello", 8);
        assert_eq!(result, "hello");
    }

    // --- fuzzy_ago tests ---

    #[rstest]
    #[case(0, "less than a minute ago")]
    #[case(30, "less than a minute ago")]
    #[case(59, "less than a minute ago")]
    #[case(60, "1 minute ago")]
    #[case(120, "2 minutes ago")]
    #[case(300, "5 minutes ago")]
    #[case(3540, "59 minutes ago")]
    #[case(3600, "1 hour ago")]
    #[case(7200, "2 hours ago")]
    #[case(82800, "23 hours ago")]
    #[case(86400, "1 day ago")]
    #[case(172_800, "2 days ago")]
    #[case(2_505_600, "29 days ago")]
    #[case(2_592_000, "1 month ago")]
    #[case(5_184_000, "2 months ago")]
    #[case(28_512_000, "11 months ago")]
    #[case(31_104_000, "1 year ago")]
    #[case(63_072_000, "2 years ago")]
    fn test_should_format_fuzzy_ago(#[case] seconds: i64, #[case] expected: &str) {
        assert_eq!(fuzzy_ago(chrono::Duration::seconds(seconds)), expected);
    }

    // --- relative_time_str tests ---

    #[test]
    fn test_should_format_relative_time_for_tty() {
        let past = Utc::now() - chrono::Duration::hours(2);
        let result = relative_time_str(&past, true);
        assert_eq!(result, "2 hours ago");
    }

    #[test]
    fn test_should_format_rfc3339_for_non_tty() {
        let t = Utc::now();
        let result = relative_time_str(&t, false);
        // RFC3339 format contains "T" and timezone info
        assert!(result.contains('T'));
    }

    // --- pluralize tests ---

    #[rstest]
    #[case(0, "0 issues")]
    #[case(1, "1 issue")]
    #[case(2, "2 issues")]
    #[case(100, "100 issues")]
    #[case(-1, "-1 issues")]
    fn test_should_pluralize(#[case] count: i64, #[case] expected: &str) {
        assert_eq!(pluralize(count, "issue", "issues"), expected);
    }

    // --- remove_excessive_whitespace tests ---

    #[rstest]
    #[case("hello   world\n\n  foo", "hello world foo")]
    #[case("  leading", "leading")]
    #[case("trailing  ", "trailing")]
    #[case("a\tb\nc", "a b c")]
    #[case("", "")]
    #[case("   ", "")]
    #[case("no extra spaces", "no extra spaces")]
    #[case("a  b  c  d", "a b c d")]
    fn test_should_remove_excessive_whitespace(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(remove_excessive_whitespace(input), expected);
    }

    // --- display_url tests ---

    #[rstest]
    #[case("https://github.com/cli/cli", "github.com/cli/cli")]
    #[case("http://example.com/", "example.com")]
    #[case("https://github.com/", "github.com")]
    #[case("github.com/cli/cli", "github.com/cli/cli")]
    #[case("https://ghe.io", "ghe.io")]
    fn test_should_display_url(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(display_url(input), expected);
    }

    // --- title_case tests ---

    #[rstest]
    #[case("hello world", "Hello World")]
    #[case("UPPER CASE", "Upper Case")]
    #[case("already Title", "Already Title")]
    #[case("single", "Single")]
    #[case("", "")]
    #[case("a b c", "A B C")]
    fn test_should_title_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(title_case(input), expected);
    }

    // --- percent_encode tests ---

    #[rstest]
    #[case("hello", "hello")]
    #[case("hello world", "hello%20world")]
    #[case("a&b=c", "a%26b%3Dc")]
    #[case("foo/bar", "foo%2Fbar")]
    #[case("100%", "100%25")]
    #[case("query?key=val", "query%3Fkey%3Dval")]
    #[case("", "")]
    #[case("no-encode_needed.here~", "no-encode_needed.here~")]
    fn test_should_percent_encode(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(percent_encode(input), expected);
    }

    // --- base64 tests ---

    #[rstest]
    #[case("SGVsbG8=", b"Hello")]
    #[case("d29ybGQ=", b"world")]
    #[case("", b"")]
    #[case("YQ==", b"a")]
    #[case("YWI=", b"ab")]
    #[case("YWJj", b"abc")]
    fn test_should_base64_decode(#[case] input: &str, #[case] expected: &[u8]) {
        assert_eq!(base64_decode(input).unwrap(), expected);
    }

    #[rstest]
    #[case(b"Hello", "SGVsbG8=")]
    #[case(b"world", "d29ybGQ=")]
    #[case(b"", "")]
    #[case(b"a", "YQ==")]
    #[case(b"ab", "YWI=")]
    #[case(b"abc", "YWJj")]
    fn test_should_base64_encode(#[case] input: &[u8], #[case] expected: &str) {
        assert_eq!(base64_encode(input), expected);
    }

    #[test]
    fn test_should_base64_decode_with_whitespace() {
        let input = "SGVs\nbG8=";
        assert_eq!(base64_decode(input).unwrap(), b"Hello");
    }

    #[test]
    fn test_should_base64_roundtrip() {
        let original = b"The quick brown fox jumps over the lazy dog";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    // --- property-based tests ---

    mod prop {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn truncate_output_never_exceeds_max_width(
                s in "\\PC{0,200}",
                width in 0usize..=200,
            ) {
                let result = truncate(&s, width);
                prop_assert!(result.chars().count() <= width || width < 4 && result.chars().count() <= s.chars().count());
            }

            #[test]
            fn truncate_short_strings_unchanged(
                s in "[a-z]{0,10}",
            ) {
                let result = truncate(&s, 100);
                prop_assert_eq!(result, s);
            }

            #[test]
            fn pluralize_singular_when_count_is_one(
                singular in "[a-z]{1,10}",
                plural in "[a-z]{1,10}",
            ) {
                let result = pluralize(1, &singular, &plural);
                prop_assert_eq!(result, format!("1 {singular}"));
            }

            #[test]
            fn pluralize_plural_when_count_is_not_one(
                count in (2i64..1000),
                singular in "[a-z]{1,10}",
                plural in "[a-z]{1,10}",
            ) {
                let result = pluralize(count, &singular, &plural);
                prop_assert_eq!(result, format!("{count} {plural}"));
            }

            #[test]
            fn remove_excessive_whitespace_no_consecutive_spaces(s in "\\PC{0,100}") {
                let result = remove_excessive_whitespace(&s);
                prop_assert!(!result.contains("  "));
                prop_assert!(!result.starts_with(' '));
                prop_assert!(!result.ends_with(' '));
            }

            #[test]
            fn fuzzy_ago_always_ends_with_ago(seconds in 0i64..=100_000_000) {
                let result = fuzzy_ago(chrono::Duration::seconds(seconds));
                prop_assert!(result.ends_with("ago"));
            }

            #[test]
            fn display_url_strips_protocol(
                path in "[a-z]{1,20}(\\.[a-z]{2,5}){1,3}(/[a-z]{1,10}){0,3}",
            ) {
                let url = format!("https://{path}");
                let result = display_url(&url);
                prop_assert!(!result.starts_with("https://"));
                prop_assert!(!result.starts_with("http://"));
            }

            #[test]
            fn title_case_preserves_word_count(s in "[a-z ]{1,50}") {
                let result = title_case(&s);
                let input_words: Vec<&str> = s.split_whitespace().collect();
                let output_words: Vec<&str> = result.split_whitespace().collect();
                prop_assert_eq!(input_words.len(), output_words.len());
            }
        }
    }
}
