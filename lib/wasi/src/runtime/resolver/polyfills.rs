use std::path::Path;

use url::Url;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn behaviour_is_identical() {
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
}
