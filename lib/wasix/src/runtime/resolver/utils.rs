use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use anyhow::Error;
use http::{HeaderMap, StatusCode};
use semver::Version;
use url::Url;

use crate::http::{HttpResponse, USER_AGENT};

fn cmp_build_metadata(left: &semver::BuildMetadata, right: &semver::BuildMetadata) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }

    left.cmp(right)
}

/// Compare two versions so that, among versions of equal SemVer precedence, a
/// dotted build metadata ordering breaks ties. Numeric identifiers compare
/// numerically, and a version without build metadata ranks above one with it.
pub(crate) fn cmp_versions_with_build(left: &Version, right: &Version) -> Ordering {
    left.cmp_precedence(right)
        .then_with(|| cmp_build_metadata(&left.build, &right.build))
}

/// [`cmp_versions_with_build`] over optionals; `None` orders below any `Some`,
/// matching [`Option`]'s own ordering.
pub(crate) fn cmp_version_with_build(left: Option<&Version>, right: Option<&Version>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => cmp_versions_with_build(left, right),
        (left, right) => left.is_some().cmp(&right.is_some()),
    }
}

/// Polyfill for [`Url::from_file_path()`] that works on `wasm32-unknown-unknown`.
pub(crate) fn url_from_file_path(path: impl AsRef<Path>) -> Option<Url> {
    let path = path.as_ref();

    if !path.is_absolute() {
        return None;
    }

    let mut buffer = String::new();

    for component in path {
        if !buffer.ends_with('/') {
            buffer.push('/');
        }

        buffer.push_str(component.to_str()?);
    }

    buffer.insert_str(0, "file://");

    buffer.parse().ok()
}

pub(crate) fn webc_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Accept", "application/webc".parse().unwrap());
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers
}

pub(crate) fn http_error(response: &HttpResponse) -> Error {
    let status = response.status;

    if status == StatusCode::SERVICE_UNAVAILABLE
        && let Some(retry_after) = response
            .headers
            .get("Retry-After")
            .and_then(|retry_after| retry_after.to_str().ok())
    {
        tracing::debug!(
            %retry_after,
            "Received 503 Service Unavailable while looking up a package. The backend may still be generating the *.webc file.",
        );
        return anyhow::anyhow!("{status} (Retry After: {retry_after})");
    }

    Error::msg(status)
}

pub(crate) fn file_path_from_url(url: &Url) -> Result<PathBuf, Error> {
    debug_assert_eq!(url.scheme(), "file");

    // Note: The Url::to_file_path() method is platform-specific
    cfg_if::cfg_if! {
        if #[cfg(any(unix, windows, target_os = "redox", target_os = "wasi"))] {
            use anyhow::Context;

            if let Ok(path) = url.to_file_path() {
                return Ok(path);
            }

            // Sometimes we'll get a UNC-like path (e.g.
            // "file:///?\\C:/\\/path/to/file.txt") and Url::to_file_path()
            // won't be able to handle the "\\?" so we try to "massage" the URL
            // a bit.
            // See <https://github.com/servo/rust-url/issues/450> for more.
            let modified = url.as_str().replace(r"\\?", "").replace("//?", "").replace('\\', "/");
            Url::parse(&modified)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .context("Unable to extract the file path")
        } else {
            anyhow::bail!("Url::to_file_path() is not supported on this platform");
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    fn version(value: &str) -> Version {
        Version::parse(value).unwrap()
    }

    #[test]
    fn build_metadata_identifiers_follow_prerelease_style_ordering() {
        assert_eq!(
            cmp_versions_with_build(&version("1.0.0+abc.2"), &version("1.0.0+abc.11")),
            Ordering::Less
        );
        assert_eq!(
            cmp_versions_with_build(&version("1.0.0+abc.2"), &version("1.0.0+abc.beta")),
            Ordering::Less
        );
        assert_eq!(
            cmp_versions_with_build(&version("1.0.0+abc"), &version("1.0.0+abc.1")),
            Ordering::Less
        );
    }

    #[test]
    fn bare_version_ranks_above_build_metadata() {
        assert_eq!(
            cmp_versions_with_build(&version("1.0.0"), &version("1.0.0+wasix.10")),
            Ordering::Greater
        );
    }

    #[test]
    fn lifecycle_category_ordering_is_preserved() {
        let ordered = ["1.0.0-alpha+build", "1.0.0-alpha", "1.0.0+build", "1.0.0"];
        for pair in ordered.windows(2) {
            assert_eq!(
                cmp_versions_with_build(&version(pair[0]), &version(pair[1])),
                Ordering::Less
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn from_file_path_behaviour_is_identical() {
        let inputs = [
            "/",
            "/path",
            "/path/to/file.txt",
            "./path/to/file.txt",
            ".",
            "",
        ];

        for path in inputs {
            let got = url_from_file_path(path);
            let expected = Url::from_file_path(path).ok();
            assert_eq!(got, expected, "Mismatch for \"{path}\"");
        }
    }

    #[test]
    #[cfg(windows)]
    fn to_file_path_can_handle_unc_paths() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let url = Url::from_file_path(&path).unwrap();

        let got = file_path_from_url(&url).unwrap();

        assert_eq!(got.canonicalize().unwrap(), path);
    }
}
