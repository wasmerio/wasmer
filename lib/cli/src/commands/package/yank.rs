//! Yank packages from the registry.

use wasmer_backend_api::{WasmerClient, types::Id};
use wasmer_sdk::package::yank::{YankOptions, YankedPackageVersion, yank_package_versions};

use crate::{commands::AsyncCliCommand, config::WasmerEnv};

/// Split `ns/pkg@selector` into the package name and version selector.
///
/// Looks for the `@` after the last `/`, so a namespace is never mistaken for
/// a version. An empty selector is rejected.
fn split_package_and_selector(spec: &str) -> Result<(&str, &str), anyhow::Error> {
    let name_end = spec.rfind('/').map(|index| index + 1).unwrap_or(0);
    let Some(at) = spec[name_end..].find('@').map(|index| index + name_end) else {
        anyhow::bail!(
            "`{spec}` does not specify a version. Pass an exact version \
             (`{spec}@1.2.3`) or a semver range (`{spec}@'>=1.0, <1.3'`)."
        );
    };

    let (name, selector) = (&spec[..at], &spec[at + 1..]);
    if name.is_empty() {
        anyhow::bail!("`{spec}` does not name a package.");
    }
    if selector.is_empty() {
        anyhow::bail!(
            "`{spec}` has an empty version selector. Pass an exact version \
             or a semver range."
        );
    }
    Ok((name, selector))
}

fn render_versions(versions: &[YankedPackageVersion]) -> String {
    versions
        .iter()
        .map(|version| version.version.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Match `selector` against a package's `(id, version)` list, returning the ids
/// to yank.
///
/// A fully specified version (`X.Y.Z`) yanks only that exact version, even when
/// it is absent, so a mistyped version yanks nothing rather than widening to a
/// range. Anything else is a semver range, so `1.2` becomes `^1.2`.
fn match_selector<'a, I>(versions: I, selector: &str) -> Result<Vec<Id>, anyhow::Error>
where
    I: IntoIterator<Item = (&'a Id, &'a str)>,
{
    let versions: Vec<(&Id, &str)> = versions.into_iter().collect();

    if semver::Version::parse(selector).is_ok() {
        return Ok(versions
            .iter()
            .filter(|(_, version)| *version == selector)
            .map(|(id, _)| (*id).clone())
            .collect());
    }

    let req = semver::VersionReq::parse(selector)
        .map_err(|err| anyhow::anyhow!("`{selector}` is not a valid version or range: {err}"))?;
    Ok(versions
        .iter()
        .filter(|(_, version)| {
            semver::Version::parse(version)
                .map(|parsed| req.matches(&parsed))
                .unwrap_or(false)
        })
        .map(|(id, _)| (*id).clone())
        .collect())
}

/// Resolve `ns/pkg@<selector>` into the version node ids to yank by listing the
/// package's versions and matching `selector` locally.
async fn resolve_version_ids(
    client: &WasmerClient,
    package_name: &str,
    selector: &str,
) -> Result<Vec<Id>, anyhow::Error> {
    let versions =
        wasmer_backend_api::query::get_package_version_ids(client, package_name.to_string())
            .await?
            .ok_or_else(|| anyhow::anyhow!("Package '{package_name}' was not found."))?;
    match_selector(
        versions.iter().map(|(id, version)| (id, version.as_str())),
        selector,
    )
}

/// Yank a package version, or a range of versions, from the registry.
///
/// A yanked version is still downloadable when pinned exactly, so existing
/// lockfiles keep working. It is skipped by `latest` and by semver-range
/// resolution. Pass `--undo` to reverse a yank.
#[derive(clap::Parser, Debug)]
pub struct PackageYank {
    #[clap(flatten)]
    env: WasmerEnv,

    /// Why the version is being yanked. Shown to users who still pin it.
    #[clap(long)]
    reason: Option<String>,

    /// Restore previously yanked versions instead of yanking them.
    #[clap(long)]
    undo: bool,

    /// The package and version to yank, as `<namespace>/<name>@<version>`.
    ///
    /// The version may be exact (`ns/pkg@1.2.3`) or a semver range
    /// (`ns/pkg@'>=1.0, <1.3'`), in which case every matching version is
    /// yanked.
    package: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageYank {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let (package_name, version_selector) = split_package_and_selector(&self.package)?;
        let client = self.env.client()?;
        let action = if self.undo { "unyank" } else { "yank" };

        // Selector->ids expansion is client-side (see `match_selector`).
        let version_ids = resolve_version_ids(&client, package_name, version_selector).await?;
        if version_ids.is_empty() {
            eprintln!("No versions of '{package_name}' match '{version_selector}'.");
            return Ok(());
        }

        let versions = yank_package_versions(
            &client,
            YankOptions {
                version_ids,
                reason: self.reason.clone(),
                undo: self.undo,
            },
        )
        .await?;

        if versions.is_empty() {
            eprintln!(
                "The matching versions of '{package_name}' were already in that state; \
                 nothing to {action}."
            );
            return Ok(());
        }

        eprintln!(
            "{}ed {} of '{package_name}': {}",
            // Capitalised for the summary line.
            if self.undo { "Unyank" } else { "Yank" },
            if versions.len() == 1 {
                "1 version".to_string()
            } else {
                format!("{} versions", versions.len())
            },
            render_versions(&versions),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions() -> Vec<(Id, String)> {
        [("pkv_1", "1.0.0"), ("pkv_2", "1.1.0"), ("pkv_3", "2.0.0")]
            .into_iter()
            .map(|(id, v)| (Id::new(id), v.to_string()))
            .collect()
    }

    fn match_ids(selector: &str) -> Vec<String> {
        let versions = versions();
        match_selector(versions.iter().map(|(id, v)| (id, v.as_str())), selector)
            .expect("selector parses")
            .into_iter()
            .map(|id| id.into_inner())
            .collect()
    }

    /// A full version yanks that version and only that version.
    #[test]
    fn an_exact_version_yanks_only_that_version() {
        assert_eq!(match_ids("1.0.0"), vec!["pkv_1"]);
    }

    /// A full version never widens to a range, so an absent one yanks nothing
    /// instead of everything matching its caret.
    #[test]
    fn an_absent_exact_version_yanks_nothing() {
        assert!(match_ids("1.0.5").is_empty());
    }

    #[test]
    fn an_exact_build_can_select_a_superseded_rebuild() {
        let versions = [
            (Id::new("old"), "1.0.0+wasix.2".to_string()),
            (Id::new("current"), "1.0.0+wasix.11".to_string()),
        ];
        let ids = match_selector(
            versions.iter().map(|(id, version)| (id, version.as_str())),
            "1.0.0+wasix.2",
        )
        .expect("selector parses");

        assert_eq!(ids, vec![Id::new("old")]);
    }

    /// A non-exact selector is a range, so it can yank several versions.
    #[test]
    fn a_range_yanks_every_matching_version() {
        assert_eq!(match_ids(">=1.0.0, <2.0.0"), vec!["pkv_1", "pkv_2"]);
    }

    #[test]
    fn an_unparsable_selector_is_rejected() {
        let versions = versions();
        let err = match_selector(
            versions.iter().map(|(id, v)| (id, v.as_str())),
            "not a selector",
        )
        .expect_err("invalid selector");
        assert!(
            err.to_string().contains("not a valid version or range"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn splits_a_package_and_version() {
        assert_eq!(
            split_package_and_selector("wasmer/bash@1.2.3").expect("splits"),
            ("wasmer/bash", "1.2.3")
        );
        assert_eq!(
            split_package_and_selector("bash@1.2.3").expect("splits"),
            ("bash", "1.2.3")
        );
    }

    /// The version separator is the `@` after the last path segment, so an `@`
    /// earlier in the spec stays part of the name.
    #[test]
    fn only_the_last_segment_at_separates_the_version() {
        assert_eq!(
            split_package_and_selector("some@owner/bash@1.2.3").expect("splits"),
            ("some@owner/bash", "1.2.3")
        );
    }

    #[test]
    fn a_spec_without_a_version_is_rejected() {
        let err = split_package_and_selector("wasmer/bash").expect_err("no version");
        assert!(
            err.to_string().contains("does not specify a version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn an_empty_version_is_rejected() {
        let err = split_package_and_selector("wasmer/bash@").expect_err("empty version");
        assert!(
            err.to_string().contains("empty version selector"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn an_empty_package_name_is_rejected() {
        let err = split_package_and_selector("@1.2.3").expect_err("no package");
        assert!(
            err.to_string().contains("does not name a package"),
            "unexpected error: {err}"
        );
    }
}
