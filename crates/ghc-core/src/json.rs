//! JSON utilities for field selection and formatting.
//!
//! Provides `--json` field selector support matching the Go CLI's behavior.

use serde_json::Value;

/// Filter a JSON value to only include the specified fields.
///
/// For objects, returns only the specified keys. For arrays, filters each
/// element. Returns the value unchanged if fields is empty or the value is
/// not an object/array.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use ghc_core::json::filter_json_fields;
///
/// let data = json!({"name": "test", "description": "desc", "url": "https://example.com"});
/// let filtered = filter_json_fields(&data, &["name".to_string(), "url".to_string()]);
/// assert_eq!(filtered, json!({"name": "test", "url": "https://example.com"}));
/// ```
pub fn filter_json_fields(value: &Value, fields: &[String]) -> Value {
    if fields.is_empty() {
        return value.clone();
    }

    match value {
        Value::Object(map) => {
            let mut filtered = serde_json::Map::new();
            for field in fields {
                if let Some(v) = map.get(field) {
                    filtered.insert(field.clone(), v.clone());
                } else {
                    // Try alternate casing: camelCase <-> snake_case
                    let snake = to_snake_case(field);
                    if let Some(v) = map.get(&snake) {
                        filtered.insert(field.clone(), v.clone());
                    } else {
                        let camel = to_camel_case(field);
                        if let Some(v) = map.get(&camel) {
                            filtered.insert(field.clone(), v.clone());
                        }
                    }
                }
            }
            Value::Object(filtered)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|item| filter_json_fields(item, fields))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Convert a `camelCase` string to `snake_case`.
fn to_snake_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert a `snake_case` string to `camelCase`.
fn to_camel_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Format a filtered JSON value as a pretty-printed string.
///
/// Combines field filtering and pretty-print serialization.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn format_json_with_fields(
    value: &Value,
    fields: &[String],
) -> Result<String, serde_json::Error> {
    let filtered = filter_json_fields(value, fields);
    serde_json::to_string_pretty(&filtered)
}

/// Format JSON output applying field selection, jq filtering, or template rendering.
///
/// This is the unified output function for all commands that support `--json`,
/// `--jq`, and `--template` flags. It applies them in priority order:
/// 1. If `jq_expr` is set, apply jq filter on the (field-filtered) value
/// 2. If `template` is set, apply template on the (field-filtered) value
/// 3. Otherwise, pretty-print the field-filtered JSON
///
/// # Errors
///
/// Returns an error if filtering, template rendering, or serialization fails.
pub fn format_json_output(
    value: &Value,
    fields: &[String],
    jq_expr: Option<&str>,
    template: Option<&str>,
) -> anyhow::Result<String> {
    let filtered = filter_json_fields(value, fields);

    if let Some(jq) = jq_expr {
        return crate::export::apply_jq_filter(&filtered, jq);
    }

    if let Some(tmpl) = template {
        return crate::export::apply_template(&filtered, tmpl);
    }

    serde_json::to_string_pretty(&filtered)
        .map_err(|e| anyhow::anyhow!("failed to serialize JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_should_filter_object_fields() {
        let data = json!({"name": "test", "description": "desc", "url": "https://example.com"});
        let filtered = filter_json_fields(&data, &["name".to_string(), "url".to_string()]);
        assert_eq!(
            filtered,
            json!({"name": "test", "url": "https://example.com"})
        );
    }

    #[test]
    fn test_should_filter_array_elements() {
        let data = json!([
            {"name": "a", "extra": 1},
            {"name": "b", "extra": 2},
        ]);
        let filtered = filter_json_fields(&data, &["name".to_string()]);
        assert_eq!(filtered, json!([{"name": "a"}, {"name": "b"}]));
    }

    #[test]
    fn test_should_return_unchanged_when_fields_empty() {
        let data = json!({"name": "test"});
        let filtered = filter_json_fields(&data, &[]);
        assert_eq!(filtered, data);
    }

    #[test]
    fn test_should_skip_missing_fields() {
        let data = json!({"name": "test"});
        let filtered = filter_json_fields(&data, &["name".to_string(), "missing".to_string()]);
        assert_eq!(filtered, json!({"name": "test"}));
    }

    #[test]
    fn test_should_pass_through_non_object_values() {
        let data = json!("plain string");
        let filtered = filter_json_fields(&data, &["field".to_string()]);
        assert_eq!(filtered, data);
    }

    #[test]
    fn test_should_alias_camel_case_to_snake_case() {
        let data = json!({"tag_name": "v1.0", "created_at": "2024-01-01"});
        let filtered = filter_json_fields(&data, &["tagName".to_string(), "createdAt".to_string()]);
        assert_eq!(
            filtered,
            json!({"tagName": "v1.0", "createdAt": "2024-01-01"})
        );
    }

    #[test]
    fn test_should_alias_snake_case_to_camel_case() {
        let data = json!({"tagName": "v1.0", "isDraft": false});
        let filtered = filter_json_fields(&data, &["tag_name".to_string(), "is_draft".to_string()]);
        assert_eq!(filtered, json!({"tag_name": "v1.0", "is_draft": false}));
    }

    #[test]
    fn test_should_format_with_fields() {
        let data = json!({"name": "test", "extra": 42});
        let result = format_json_with_fields(&data, &["name".to_string()]).unwrap();
        assert!(result.contains("\"name\""));
        assert!(!result.contains("extra"));
    }
}
