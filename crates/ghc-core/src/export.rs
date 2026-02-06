//! Export utilities for `--jq` and `--template` flags.
//!
//! Provides real jq filtering via the `jaq` crate and basic Go-template-style
//! formatting for JSON output, matching the Go CLI's `--jq` and `--template`
//! behavior.

use anyhow::{Context, Result};
use serde_json::Value;

/// Apply a jq expression to a JSON value and return the formatted result.
///
/// Uses the `jaq` crate for full jq compatibility. The filter is compiled with
/// the standard library loaded, so expressions like `.[] | select(.foo)`,
/// `map(...)`, `keys`, `length`, etc. all work.
///
/// # Errors
///
/// Returns an error if the jq expression cannot be parsed or execution fails.
pub fn apply_jq_filter(value: &Value, expression: &str) -> Result<String> {
    use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

    // Build the parsing context with the standard library loaded
    let mut defs = ParseCtx::new(Vec::new());
    defs.insert_natives(jaq_core::core());
    defs.insert_defs(jaq_std::std());

    // Parse the filter expression
    let (filter, errs) = jaq_parse::parse(expression, jaq_parse::main());
    if !errs.is_empty() {
        let err_msgs: Vec<String> = errs.iter().map(|e| format!("{e}")).collect();
        anyhow::bail!("failed to parse jq expression: {}", err_msgs.join(", "));
    }

    let filter = filter.context("failed to parse jq expression")?;
    let filter = defs.compile(filter);

    let inputs = RcIter::new(core::iter::empty());
    let out = filter.run((Ctx::new([], &inputs), Val::from(value.clone())));

    let mut results: Vec<String> = Vec::new();
    for item in out {
        match item {
            Ok(val) => {
                let json_val: Value = val.into();
                match json_val {
                    Value::String(s) => results.push(s),
                    Value::Null => results.push("null".to_string()),
                    other => {
                        results.push(
                            serde_json::to_string(&other).unwrap_or_else(|_| format!("{other}")),
                        );
                    }
                }
            }
            Err(err) => {
                anyhow::bail!("jq filter error: {err}");
            }
        }
    }

    Ok(results.join("\n"))
}

/// Apply a Go-template-style expression to a JSON value.
///
/// Supports a subset of Go template syntax:
/// - `{{.field}}` - access a field
/// - `{{.field.subfield}}` - nested access
/// - `{{range .array}}...{{end}}` - iterate arrays
/// - `{{.}}` - current value
/// - `{{tablerow .field1 .field2}}` - tab-separated fields (per gh CLI)
/// - Plain text is passed through as-is
///
/// # Errors
///
/// Returns an error if the template syntax is invalid or field access fails.
pub fn apply_template(value: &Value, template: &str) -> Result<String> {
    let mut output = String::new();
    let mut pos = 0;
    let bytes = template.as_bytes();

    while pos < bytes.len() {
        if pos + 1 < bytes.len() && bytes[pos] == b'{' && bytes[pos + 1] == b'{' {
            // Find closing }}
            let start = pos + 2;
            let end = template[start..]
                .find("}}")
                .map(|i| start + i)
                .context("unclosed template expression: missing }}")?;

            let expr = template[start..end].trim();
            pos = end + 2;

            if expr.starts_with('"') && expr.ends_with('"') {
                // Go-style string literal: {{"\n"}} → newline, {{"\t"}} → tab
                let inner = &expr[1..expr.len() - 1];
                let unescaped = unescape_go_string(inner);
                output.push_str(&unescaped);
            } else if let Some(range_expr) = expr.strip_prefix("range ") {
                // Handle {{range .field}}...{{end}}
                let field_path = range_expr.trim();
                let arr_val = resolve_path(value, field_path)?;
                let arr = arr_val.as_array().context("range target is not an array")?;

                // Find {{end}}
                let body_start = pos;
                let end_tag = "{{end}}";
                let body_end = template[body_start..]
                    .find(end_tag)
                    .map(|i| body_start + i)
                    .context("missing {{end}} for range")?;
                let body_template = &template[body_start..body_end];
                pos = body_end + end_tag.len();

                for item in arr {
                    let rendered = apply_template(item, body_template)?;
                    output.push_str(&rendered);
                }
            } else if let Some(fields_str) = expr.strip_prefix("tablerow ") {
                let fields: Vec<&str> = fields_str.split_whitespace().collect();
                let mut parts = Vec::new();
                for field in fields {
                    let val = resolve_path(value, field)?;
                    parts.push(value_to_string(&val));
                }
                output.push_str(&parts.join("\t"));
            } else if expr == "." {
                output.push_str(&value_to_string(value));
            } else if expr.starts_with('.') {
                let val = resolve_path(value, expr)?;
                output.push_str(&value_to_string(&val));
            } else {
                // Unknown expression, output as-is
                output.push_str("{{");
                output.push_str(expr);
                output.push_str("}}");
            }
        } else if pos + 1 < bytes.len() && bytes[pos] == b'\\' && bytes[pos + 1] == b'n' {
            output.push('\n');
            pos += 2;
        } else if pos + 1 < bytes.len() && bytes[pos] == b'\\' && bytes[pos + 1] == b't' {
            output.push('\t');
            pos += 2;
        } else {
            output.push(bytes[pos] as char);
            pos += 1;
        }
    }

    Ok(output)
}

/// Unescape a Go string literal, handling common escape sequences.
fn unescape_go_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') | None => result.push('\\'),
                Some('"') => result.push('"'),
                Some('r') => result.push('\r'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Resolve a dotted path like `.field.subfield` on a JSON value.
fn resolve_path(value: &Value, path: &str) -> Result<Value> {
    let path = path.strip_prefix('.').unwrap_or(path);
    if path.is_empty() {
        return Ok(value.clone());
    }

    let mut current = value;
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        current = current
            .get(part)
            .ok_or_else(|| anyhow::anyhow!("template: field '{part}' not found"))?;
    }
    Ok(current.clone())
}

/// Convert a JSON value to a display string (without quotes for strings).
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- jq filter tests ---

    #[test]
    fn test_should_apply_jq_identity() {
        let val = json!({"a": 1, "b": 2});
        let result = apply_jq_filter(&val, ".").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, val);
    }

    #[test]
    fn test_should_apply_jq_field_access() {
        let val = json!({"name": "test", "count": 42});
        let result = apply_jq_filter(&val, ".name").unwrap();
        assert_eq!(result, "test");
    }

    #[test]
    fn test_should_apply_jq_nested_access() {
        let val = json!({"a": {"b": {"c": "deep"}}});
        let result = apply_jq_filter(&val, ".a.b.c").unwrap();
        assert_eq!(result, "deep");
    }

    #[test]
    fn test_should_apply_jq_array_iteration() {
        let val = json!(["a", "b", "c"]);
        let result = apply_jq_filter(&val, ".[]").unwrap();
        assert_eq!(result, "a\nb\nc");
    }

    #[test]
    fn test_should_apply_jq_array_map() {
        let val = json!([{"name": "alice"}, {"name": "bob"}]);
        let result = apply_jq_filter(&val, ".[].name").unwrap();
        assert_eq!(result, "alice\nbob");
    }

    #[test]
    fn test_should_apply_jq_array_index() {
        let val = json!(["a", "b", "c"]);
        let result = apply_jq_filter(&val, ".[1]").unwrap();
        assert_eq!(result, "b");
    }

    #[test]
    fn test_should_apply_jq_length() {
        let val = json!([1, 2, 3]);
        let result = apply_jq_filter(&val, "length").unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_should_apply_jq_keys() {
        let val = json!({"b": 1, "a": 2});
        let result = apply_jq_filter(&val, "keys").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let arr = parsed.as_array().unwrap();
        assert!(arr.contains(&json!("a")));
        assert!(arr.contains(&json!("b")));
    }

    #[test]
    fn test_should_apply_jq_select() {
        let val = json!([{"name": "a", "v": 1}, {"name": "b", "v": 2}, {"name": "c", "v": 3}]);
        let result = apply_jq_filter(&val, ".[] | select(.v > 1) | .name").unwrap();
        assert_eq!(result, "b\nc");
    }

    #[test]
    fn test_should_return_error_for_invalid_jq() {
        let val = json!({});
        let result = apply_jq_filter(&val, "invalid[[[");
        assert!(result.is_err());
    }

    // --- template tests ---

    #[test]
    fn test_should_apply_template_field() {
        let val = json!({"name": "test", "count": 42});
        let result = apply_template(&val, "Name: {{.name}}").unwrap();
        assert_eq!(result, "Name: test");
    }

    #[test]
    fn test_should_apply_template_nested() {
        let val = json!({"user": {"login": "alice"}});
        let result = apply_template(&val, "{{.user.login}}").unwrap();
        assert_eq!(result, "alice");
    }

    #[test]
    fn test_should_apply_template_range() {
        let val = json!({"items": [{"name": "a"}, {"name": "b"}]});
        let result = apply_template(&val, "{{range .items}}{{.name}}\n{{end}}").unwrap();
        assert_eq!(result, "a\nb\n");
    }

    #[test]
    fn test_should_apply_template_tablerow() {
        let val = json!({"name": "test", "id": 42});
        let result = apply_template(&val, "{{tablerow .name .id}}").unwrap();
        assert_eq!(result, "test\t42");
    }

    #[test]
    fn test_should_apply_template_escape_sequences() {
        let val = json!({"a": 1});
        let result = apply_template(&val, "hello\\tworld\\n").unwrap();
        assert_eq!(result, "hello\tworld\n");
    }

    #[test]
    fn test_should_return_error_for_unclosed_template() {
        let val = json!({});
        let result = apply_template(&val, "{{.name");
        assert!(result.is_err());
    }

    #[test]
    fn test_should_apply_template_identity() {
        let val = json!("hello");
        let result = apply_template(&val, "{{.}}").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_should_apply_template_go_string_escape_newline() {
        let val = json!({"items": [{"name": "a"}, {"name": "b"}]});
        let result = apply_template(&val, "{{range .items}}{{.name}}{{\"\\n\"}}{{end}}").unwrap();
        assert_eq!(result, "a\nb\n");
    }

    #[test]
    fn test_should_apply_template_go_string_escape_tab() {
        let val = json!({"a": 1});
        let result = apply_template(&val, "hello{{\"\\t\"}}world{{\"\\n\"}}").unwrap();
        assert_eq!(result, "hello\tworld\n");
    }
}
