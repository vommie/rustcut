//! A folder's `file://` URL encoded the way Dolphin (Qt's `QUrl::toEncoded`) writes it: Dolphin compares folder URLs
//! exactly, and names its view-properties store after the SHA-1 of this text. The `url` crate follows WHATWG and leaves
//! `[`, `]`, `\`, `^` and `|` as they are, which made a folder like `/media/[test]` look closed to Dolphin.

use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

/// What `QUrl` percent-encodes in a path (besides every byte outside ASCII).
const QURL_PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// `file:///home/me/%5Btest%5D%20(1)` for `/home/me/[test] (1)`; `None` for a relative path.
#[must_use]
pub fn file_url(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let bytes = path.as_os_str().as_bytes();
    let encoded: String = match std::str::from_utf8(bytes) {
        Ok(text) => utf8_percent_encode(text, QURL_PATH).collect(),
        Err(_) => percent_encoding::percent_encode(bytes, QURL_PATH).collect(),
    };
    Some(format!("file://{encoded}"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::file_url;

    #[test]
    fn encodes_like_qurl() {
        let url = |p: &str| file_url(Path::new(p)).unwrap_or_default();
        assert_eq!(url("/home/me/[test]/x (1)"), "file:///home/me/%5Btest%5D/x%20(1)");
        assert_eq!(url("/a/Ärger 100%/b#c?d"), "file:///a/%C3%84rger%20100%25/b%23c%3Fd");
        assert_eq!(url("/a/x|y^z`{}\\\""), "file:///a/x%7Cy%5Ez%60%7B%7D%5C%22");
        assert_eq!(url("/a/(b)!$&'*+,;=:@~"), "file:///a/(b)!$&'*+,;=:@~");
        assert!(file_url(Path::new("relative/x")).is_none());
    }
}
