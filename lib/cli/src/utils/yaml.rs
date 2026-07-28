//! Format-preserving YAML editing for `app.yaml`.
//!
//! Serializing a `serde_yaml::Value` back to a string drops the user's
//! formatting (comments, key order, blank lines, quoting).
//! [`apply_app_config_to_yaml`] instead edits the original text in place with
//! [`yaml_edit`], rewriting only the nodes whose value changed.

use std::str::FromStr;

use anyhow::Context as _;
use serde_yaml::Value;
use yaml_edit::{Document, Mapping, YamlNode};

/// Apply `target` onto the original app YAML `text`, preserving the formatting
/// of everything that did not change.
///
/// * A node is rewritten only when its value differs from `target`; comments,
///   order, blank lines and quoting are otherwise untouched.
/// * Top-level keys are synced (added, updated, or removed); nested mappings are
///   merged without removing keys. See [`merge_into_mapping`] for why.
/// * A `null` in `target` is never added (avoids `name: null` noise); an
///   existing key can still be set to it.
///
/// When the edit cannot be format-preserved (a structured value changed,
/// see [`set_value`]) or its output does not reparse, falls back to a plain
/// serialization of `target`.
pub(crate) fn apply_app_config_to_yaml(text: &str, target: &Value) -> anyhow::Result<String> {
    match try_format_preserving_edit(text, target) {
        Ok(out) if serde_yaml::from_str::<Value>(&out).is_ok() => Ok(out),
        Ok(_) => {
            tracing::warn!(
                "format-preserving app YAML edit produced invalid YAML; \
                 rewriting the file without preserving formatting"
            );
            Ok(serde_yaml::to_string(target)?)
        }
        Err(err) => {
            tracing::warn!(
                ?err,
                "cannot format-preserve app YAML edit; \
                 rewriting the file without preserving formatting"
            );
            Ok(serde_yaml::to_string(target)?)
        }
    }
}

fn try_format_preserving_edit(text: &str, target: &Value) -> anyhow::Result<String> {
    let doc = Document::from_str(text)
        .map_err(|e| anyhow::anyhow!("could not parse YAML for format-preserving edit: {e}"))?;
    // Second parse, used to detect which keys changed. `yaml_edit` node text
    // cannot be reparsed for this: it is dedented on the first line only, so
    // block values are not valid standalone YAML.
    let original: Value = serde_yaml::from_str(text)
        .map_err(|e| anyhow::anyhow!("could not parse YAML semantically: {e}"))?;

    match (doc.as_mapping(), target, &original) {
        (Some(mapping), Value::Mapping(target_mapping), Value::Mapping(original_mapping)) => {
            merge_into_mapping(&mapping, target_mapping, Some(original_mapping), true)?;
            let out = doc.to_string();
            // Restore the leading comment block that `yaml_edit` drops (see
            // `leading_trivia`).
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

/// Recursively merge `target` into `mapping`, using `original` (the semantic
/// parse of the same subtree) to skip unchanged keys. `remove_missing` (true
/// only at the top level) drops keys absent from `target`.
///
/// Only the top level removes, because `AppConfigV1`'s `#[serde(flatten)] extra`
/// re-emits unknown top-level keys: a key missing from `target` there was cleared
/// on purpose (e.g. `app_id`/`name` on owner change). Nested sub-structs have no
/// such catch-all and drop fields they don't model, so removing nested keys would
/// delete the user's forward-compatible settings.
fn merge_into_mapping(
    mapping: &Mapping,
    target: &serde_yaml::Mapping,
    original: Option<&serde_yaml::Mapping>,
    remove_missing: bool,
) -> anyhow::Result<()> {
    for (key_value, value) in target {
        let Some(key) = scalar_key(key_value) else {
            continue;
        };

        let original_value = original.and_then(|m| m.get(key_value));

        if mapping.get(&key).is_some() {
            if original_value == Some(value) {
                // Semantically identical - keep the original formatting.
                continue;
            }

            // Recurse so we only touch what changed within the nested mapping.
            if let (Some(child), Value::Mapping(target_child)) = (mapping.get_mapping(&key), value)
            {
                let original_child = match original_value {
                    Some(Value::Mapping(m)) => Some(m),
                    // The key changed type; every nested key counts as changed.
                    _ => None,
                };
                merge_into_mapping(&child, target_child, original_child, false)?;
            } else {
                set_value(mapping, &key, value)
                    .with_context(|| format!("could not update app YAML key `{key}`"))?;
            }
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

/// Set `mapping[key] = value`. Only scalars are supported: `yaml_edit`
/// incorrectly renders block values when setting them (as of 0.2, #6803), so
/// structured values error out and the caller falls back to a plain rewrite.
fn set_value(mapping: &Mapping, key: &str, value: &Value) -> anyhow::Result<()> {
    match value {
        Value::Null => mapping.set(key, yaml_edit::ScalarValue::null()),
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
                // Unreachable: `serde_yaml` numbers are always i64/u64/f64.
                anyhow::bail!("cannot represent number for app YAML key `{key}`");
            }
        }
        Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => {
            anyhow::bail!("cannot format-preserve an edit to structured value `{key}`");
        }
    }

    Ok(())
}

/// Return the leading run of blank and comment-only lines at the top of `text`.
///
/// `yaml_edit` has a bug that drops this block (as of 0.2), so we
/// capture it here to splice back after editing.
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

/// Extract a scalar mapping key as a string. Non-scalar keys
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_yaml::to_string(target)?),
        Err(e) => Err(e).with_context(|| format!("could not read YAML file '{}'", path.display())),
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
    fn sets_existing_key_to_null() {
        let original = r#"kind: wasmer.io/App.v0
name: my-app
package: .
"#;
        let mut target = serde_yaml::Mapping::new();
        target.insert("kind".into(), "wasmer.io/App.v0".into());
        target.insert("name".into(), Value::Null);
        target.insert("package".into(), ".".into());

        let out = apply_app_config_to_yaml(original, &Value::Mapping(target)).unwrap();
        assert!(out.contains("name: null"), "{out}");
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

    /// Fails once `yaml_edit` fixes the [`leading_trivia`] bug, cueing us to
    /// drop the workaround.
    #[test]
    fn yaml_edit_still_drops_leading_comments_so_workaround_is_needed() {
        let src = "# leading comment\nkind: wasmer.io/App.v0\nname: my-app\n";
        let roundtripped = Document::from_str(src).unwrap().to_string();

        assert!(
            !roundtripped.contains("# leading comment"),
            "yaml_edit now preserves leading comments on round-trip; drop the \
             `leading_trivia` workaround in `apply_app_config_to_yaml` (and this \
             test).\nGot:\n{roundtripped}"
        );
    }

    /// The workaround must re-attach the dropped leading comment, even when a
    /// value changes.
    #[test]
    fn workaround_reattaches_leading_comment() {
        let original = "# leading comment\nkind: wasmer.io/App.v0\nowner: alice\n";
        let target = parse("kind: wasmer.io/App.v0\nowner: bob\n");

        let out = apply_app_config_to_yaml(original, &target).unwrap();
        assert!(out.starts_with("# leading comment\n"), "{out}");
        assert!(out.contains("owner: bob"), "{out}");
    }

    /// The `app.yaml` from issue #6803.
    const JOBS_APP_YAML: &str = r#"package: .
scaling:
  mode: single_concurrency
jobs:
  - name: post-deployment
    trigger: post-deployment
    action:
      execute:
        package: wasmer/bash
        command: ls
  - name: fetch-every-8-min
    trigger: "*/8 * * * *"
    action:
      fetch:
        path: /
        timeout: 30s
  - name: exec-every-min
    trigger: 1m
    action:
      execute:
        package: wasmer/bash
        command: ls
        cli_args:
          - -lah
kind: wasmer.io/App.v0
annotations:
  shipitcli.com/config:
    commands: {}
    php_version: 8.3.29
    phpix: true
    phpix_worker_threads: 4
    port: 8080
    use_composer: false
  shipitcli.com/provider: php
  shipitcli.com/version: 0.21.6
"#;

    /// #6803: an unchanged `jobs` was falsely detected as changed and then
    /// corrupted to `jobs:- name: ...` on rewrite.
    #[test]
    fn deploy_flow_leaves_indented_jobs_sequence_untouched() {
        use wasmer_config::app::AppConfigV1;

        let mut config = AppConfigV1::parse_yaml(JOBS_APP_YAML).unwrap();
        config.app_id = Some("da_test123".to_string());
        config.name = Some("my-app".to_string());
        config.owner = Some("someowner".to_string());
        let target = config.clone().to_yaml_value().unwrap();

        let out = apply_app_config_to_yaml(JOBS_APP_YAML, &target).unwrap();

        // The unchanged blocks keep their exact formatting.
        assert!(out.contains("jobs:\n  - name: post-deployment"), "{out}");
        assert!(out.contains("    trigger: \"*/8 * * * *\""), "{out}");
        assert!(out.contains("        cli_args:\n          - -lah"), "{out}");
        assert!(
            out.contains("  shipitcli.com/config:\n    commands: {}"),
            "{out}"
        );

        // The intended changes landed and the file is still a valid config.
        let reparsed = AppConfigV1::parse_yaml(&out).unwrap();
        assert_eq!(reparsed.app_id.as_deref(), Some("da_test123"));
        assert_eq!(reparsed.name.as_deref(), Some("my-app"));
        assert_eq!(reparsed.owner.as_deref(), Some("someowner"));
        assert_eq!(reparsed.jobs, config.jobs);
    }

    /// A genuine sequence change currently cannot be format-preserved (see
    /// [`set_value`]); the result must be a valid plain rewrite.
    #[test]
    fn genuine_sequence_change_produces_valid_yaml() {
        use wasmer_config::app::AppConfigV1;

        let mut config = AppConfigV1::parse_yaml(JOBS_APP_YAML).unwrap();
        config.jobs.as_mut().unwrap().remove(2);
        let target = config.clone().to_yaml_value().unwrap();

        let out = apply_app_config_to_yaml(JOBS_APP_YAML, &target).unwrap();

        let reparsed = AppConfigV1::parse_yaml(&out).unwrap();
        assert_eq!(reparsed.jobs, config.jobs);
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
