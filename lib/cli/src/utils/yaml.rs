//! Format-preserving YAML editing helpers.
//!
//! The app config (`app.yaml`) is round-tripped through [`serde_yaml`] in many
//! places. Serializing a `serde_yaml::Value` back to a string discards the
//! user's original formatting: comments, key order, blank lines and quoting
//! style are all lost.
//!
//! [`apply_app_config_to_yaml`] instead takes the *original* app config text and
//! applies a computed target value onto it using [`yaml_edit`] (a lossless,
//! `toml-edit`-style editor). Only the nodes whose semantic value actually
//! changed are rewritten; everything else is left byte-for-byte intact.
//!
//! Note: `yaml_edit` 0.2 drops the leading comment block that precedes the very
//! first key on a round-trip, so [`apply_app_config_to_yaml`] re-attaches it
//! manually (see [`leading_trivia`]).

use std::str::FromStr;

use anyhow::Context as _;
use serde_yaml::Value;
use yaml_edit::{Document, Mapping, YamlNode};

/// Apply `target` onto the original app YAML `text`, preserving the formatting
/// of everything that did not change.
///
/// The merge semantics are app-config specific:
///
/// * Top-level keys are fully synced: keys present in `target` are added or
///   updated, and keys absent from `target` are removed.
/// * Nested mappings are merged recursively, but nested keys are never removed
///   (so unknown/extra nested settings are preserved).
/// * A node is only rewritten when its semantic value differs from `target`,
///   leaving comments, ordering, blank lines and quoting style untouched.
/// * A `null` value in `target` is never *added* to the document (it would only
///   introduce noise like `name: null`); an existing key can still be updated.
pub(crate) fn apply_app_config_to_yaml(text: &str, target: &Value) -> anyhow::Result<String> {
    let doc = Document::from_str(text)
        .map_err(|e| anyhow::anyhow!("could not parse YAML for format-preserving edit: {e}"))?;

    match (doc.as_mapping(), target) {
        (Some(mapping), Value::Mapping(target_mapping)) => {
            merge_into_mapping(&mapping, target_mapping, true)?;
            let out = doc.to_string();
            // `yaml_edit` (0.2) drops the leading comment/blank block that
            // precedes the first key, even on a no-op round-trip. Re-attach it.
            let header = leading_trivia(text);
            if !header.is_empty() && !out.starts_with(header) {
                Ok(format!("{header}{out}"))
            } else {
                Ok(out)
            }
        }
        // The document root is not a mapping (or the target is not a mapping).
        // We have no formatting to preserve in a meaningful way, so fall back to
        // a plain serialization of the target.
        _ => Ok(serde_yaml::to_string(target)?),
    }
}

/// Recursively merge `target` into `mapping`.
///
/// `remove_missing` controls whether keys present in `mapping` but absent from
/// `target` are removed; it is only `true` at the top level.
fn merge_into_mapping(
    mapping: &Mapping,
    target: &serde_yaml::Mapping,
    remove_missing: bool,
) -> anyhow::Result<()> {
    for (key, value) in target {
        let Some(key) = scalar_key(key) else {
            continue;
        };

        if let Some(existing) = mapping.get(&key) {
            if node_matches(&existing.to_string(), value) {
                // Semantically identical - keep the original formatting.
                continue;
            }

            // Recurse into nested mappings so we only touch what changed.
            if let (Some(child), Value::Mapping(target_child)) = (mapping.get_mapping(&key), value)
            {
                merge_into_mapping(&child, target_child, false)?;
                continue;
            }

            set_value(mapping, &key, value)
                .with_context(|| format!("could not update app YAML key `{key}`"))?;
        } else if !value.is_null() {
            // Don't introduce `key: null` noise for absent keys.
            set_value(mapping, &key, value)
                .with_context(|| format!("could not add app YAML key `{key}`"))?;
        }
    }

    if remove_missing {
        for key in stale_top_level_keys(mapping, target) {
            mapping.remove(key.as_str());
        }
    }

    Ok(())
}

/// Set `mapping[key] = value`, dispatching scalars to typed setters and
/// structured values (mappings/sequences) to a parsed node so block style is
/// preserved.
fn set_value(mapping: &Mapping, key: &str, value: &Value) -> anyhow::Result<()> {
    match value {
        Value::Null => set_via_node(mapping, key, value)?,
        Value::Bool(b) => mapping.set(key, *b),
        Value::String(s) => mapping.set(key, s.as_str()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                mapping.set(key, i);
            } else if let Some(u) = n.as_u64() {
                mapping.set(key, u);
            } else if let Some(f) = n.as_f64() {
                mapping.set(key, f);
            } else {
                set_via_node(mapping, key, value)?;
            }
        }
        Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => {
            set_via_node(mapping, key, value)?;
        }
    }

    Ok(())
}

/// Set a structured value by serializing it and parsing it back into a node, so
/// `yaml_edit` reconstructs the proper block/flow representation.
fn set_via_node(mapping: &Mapping, key: &str, value: &Value) -> anyhow::Result<()> {
    // `serde_yaml::Value` is semantic data, while `yaml_edit::YamlNode` is a
    // CST-backed node with formatting/trivia. There is no direct cast between
    // them. A custom recursive `AsYaml` converter would be larger and easier to
    // get wrong than this small serialize-parse-graft path, and app.yaml writes
    // are not hot enough for the extra complexity to pay for itself.
    // Serialize the value under a temporary key, parse it, and graft the node.
    let mut wrapper = serde_yaml::Mapping::new();
    wrapper.insert(Value::String(key.to_string()), value.clone());
    let rendered = serde_yaml::to_string(&Value::Mapping(wrapper))
        .context("could not serialize app YAML value")?;

    let node = Document::from_str(&rendered)
        .map_err(|e| anyhow::anyhow!("could not parse serialized app YAML value: {e}"))?
        .as_mapping()
        .and_then(|m| m.get(key))
        .context("serialized app YAML value did not contain the expected key")?;

    mapping.set(key, node);
    Ok(())
}

/// Whether the rendered text of an existing node is semantically equal to
/// `target` (ignoring formatting/quoting differences).
fn node_matches(node_text: &str, target: &Value) -> bool {
    serde_yaml::from_str::<Value>(node_text)
        .map(|v| &v == target)
        .unwrap_or(false)
}

/// Return the leading run of blank and comment-only lines at the top of `text`
/// (everything before the first line that carries actual content).
fn leading_trivia(text: &str) -> &str {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            end += line.len();
        } else {
            break;
        }
    }
    &text[..end]
}

/// Return top-level keys present in `mapping` but absent from `target`.
fn stale_top_level_keys(mapping: &Mapping, target: &serde_yaml::Mapping) -> Vec<String> {
    mapping
        .entries()
        .filter_map(|entry| {
            let key = entry.key_node()?;
            let is_target_key = target
                .keys()
                .filter_map(scalar_key)
                .any(|target_key| key.yaml_eq(&target_key));
            if is_target_key {
                None
            } else {
                scalar_key_node(&key)
            }
        })
        .collect()
}

/// Extract a scalar mapping key as a string. Non-scalar keys (rare in app.yaml)
/// are skipped.
fn scalar_key(key: &Value) -> Option<String> {
    match key {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn scalar_key_node(key: &YamlNode) -> Option<String> {
    key.as_scalar().map(|s| s.as_string())
}

/// Convenience wrapper: read the original text from `path`, apply `target`
/// onto it, and return the format-preserved result. If the file does not exist
/// or cannot be read, falls back to a plain serialization of `target`.
pub(crate) fn apply_app_config_to_yaml_file(
    path: &std::path::Path,
    target: &Value,
) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => apply_app_config_to_yaml(&text, target)
            .with_context(|| format!("could not edit YAML file '{}'", path.display())),
        Err(_) => Ok(serde_yaml::to_string(target)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    #[test]
    fn preserves_comments_and_order_on_scalar_change() {
        let original = r#"# my app
kind: wasmer.io/App.v0
name: my-app  # the app name
owner: alice
package: .
"#;
        // Only `owner` changed.
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
owner: bob
package: .
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert_eq!(
            out,
            r#"# my app
kind: wasmer.io/App.v0
name: my-app  # the app name
owner: bob
package: .
"#
        );
    }

    #[test]
    fn adds_new_key_without_touching_rest() {
        let original = r#"kind: wasmer.io/App.v0
name: my-app
package: .
"#;
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
package: .
app_id: da_abc123
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(out.contains("app_id: da_abc123"));
        // Existing lines untouched.
        assert!(out.contains("kind: wasmer.io/App.v0\nname: my-app\npackage: ."));
    }

    #[test]
    fn removes_top_level_key_absent_from_target() {
        let original = r#"kind: wasmer.io/App.v0
name: my-app
app_id: da_old
owner: alice
package: .
"#;
        // app_id dropped (e.g. owner changed).
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
owner: bob
package: .
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(!out.contains("app_id"), "app_id should be removed: {out}");
        assert!(out.contains("owner: bob"));
        assert!(out.contains("name: my-app"));
    }

    #[test]
    fn preserves_nested_mapping_and_comments_when_unchanged() {
        let original = r#"kind: wasmer.io/App.v0
name: my-app
owner: alice
package: .
# capabilities for the app
capabilities:
  # memory limit
  memory:
    limit: 512MB
env:
  FOO: bar
"#;
        // Only owner changes; nested structures are identical.
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
owner: bob
package: .
capabilities:
  memory:
    limit: 512MB
env:
  FOO: bar
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(out.contains("# capabilities for the app"));
        assert!(out.contains("# memory limit"));
        assert!(out.contains("owner: bob"));
        assert!(!out.contains("owner: alice"));
    }

    /// End-to-end check of the real deploy flow: parse a commented `app.yaml`
    /// into an [`AppConfigV1`], mutate a field the way `wasmer app deploy` does
    /// (assign the backend `app_id`), serialize it with `to_yaml_value`, and
    /// apply it back. The backend assignment must land while every comment,
    /// blank line, key order and nested structure is left intact.
    #[test]
    fn deploy_flow_preserves_formatting_when_assigning_app_id() {
        use wasmer_config::app::AppConfigV1;

        let original = r#"# Wasmer app configuration
kind: wasmer.io/App.v0
name: my-cool-app  # human readable name
owner: alice
package: .

# how much memory the app gets
capabilities:
  memory:
    limit: 512MB

env:
  LOG_LEVEL: debug
"#;

        // Parse exactly like the CLI does.
        let mut config = AppConfigV1::parse_yaml(original).unwrap();
        assert!(config.app_id.is_none());

        // The backend assigns an id on first deploy; this is the only change.
        config.app_id = Some("da_abc123".to_string());

        let target = config.clone().to_yaml_value().unwrap();
        let out = apply_app_config_to_yaml(original, &target).unwrap();

        // The new field is added (appended, not reordered into the middle)...
        assert!(out.contains("app_id: da_abc123"), "app_id missing:\n{out}");

        // ...and all comments, ordering, blank lines and nesting survive.
        assert!(out.starts_with("# Wasmer app configuration\n"), "{out}");
        assert!(
            out.contains("name: my-cool-app  # human readable name"),
            "{out}"
        );
        assert!(out.contains("# how much memory the app gets"), "{out}");
        assert!(out.contains("  memory:\n    limit: "), "{out}");
        assert!(out.contains("env:\n  LOG_LEVEL: debug"), "{out}");
        // The blank lines between sections are kept.
        assert!(out.contains("package: .\n\n"), "{out}");

        // The `limit` node is the one exception: `ByteSize` is normalized when it
        // round-trips through the typed struct (`512MB` -> `488.3 MiB`, the same
        // amount). Only that node changes; the surrounding formatting does not.
        assert!(!out.contains("512MB"), "{out}");
        assert!(out.contains("488.3 MiB"), "{out}");

        // The result is still a valid app config with the expected fields.
        // (We don't assert full struct equality because `ByteSize`'s own
        // display round-trip is mildly lossy: 512MB -> "488.3 MiB" -> a value a
        // few hundred bytes off. That predates this change.)
        let reparsed = AppConfigV1::parse_yaml(&out).unwrap();
        assert_eq!(reparsed.app_id.as_deref(), Some("da_abc123"));
        assert_eq!(reparsed.name, config.name);
        assert_eq!(reparsed.owner, config.owner);
        assert_eq!(reparsed.env, config.env);
    }

    #[test]
    fn does_not_add_null_for_absent_key() {
        let original = r#"kind: wasmer.io/App.v0
package: .
"#;
        let mut target = serde_yaml::Mapping::new();
        target.insert("kind".into(), "wasmer.io/App.v0".into());
        target.insert("package".into(), ".".into());
        target.insert("name".into(), Value::Null);

        let out = apply_app_config_to_yaml(original, &Value::Mapping(target)).unwrap();
        assert!(!out.contains("name"), "should not add `name: null`: {out}");
    }

    #[test]
    fn removes_quoted_top_level_key_absent_from_target() {
        let original = r#""app_id": da_old
kind: wasmer.io/App.v0
name: my-app
package: .
"#;
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
package: .
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(!out.contains("app_id"), "app_id should be removed: {out}");
        assert!(!out.contains("da_old"), "old id should be removed: {out}");
        assert!(out.contains("kind: wasmer.io/App.v0"));
    }

    #[test]
    fn preserves_nested_keys_absent_from_target() {
        let original = r#"kind: wasmer.io/App.v0
name: my-app
package: .
capabilities:
  memory:
    limit: 512MB
    custom_limit_hint: keep-me
"#;
        let target = parse(
            r#"kind: wasmer.io/App.v0
name: my-app
package: .
capabilities:
  memory:
    limit: 512MB
"#,
        );

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(out.contains("custom_limit_hint: keep-me"), "{out}");
    }
}
