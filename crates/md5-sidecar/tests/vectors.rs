//! The test vectors of SPEC.md: content -> hash or the reason it is invalid, and the freshness rule.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code"
)]

use std::time::{Duration, SystemTime};

use md5_sidecar::{Invalid, MAX_LEN, Md5Hex, ZERO_BYTE_MD5, is_fresh, parse};

const H: &str = "9e107d9d372bb6826bd81d3542a419d6";

fn ok(content: &[u8], want: &str) {
    let got = parse(content).map(|hash| hash.to_string());
    assert_eq!(
        got.as_deref(),
        Ok(want),
        "content {:?}",
        String::from_utf8_lossy(content)
    );
}

fn bad(content: &[u8], want: Invalid) {
    assert_eq!(
        parse(content),
        Err(want),
        "content {:?}",
        String::from_utf8_lossy(content)
    );
}

#[test]
fn valid_contents() {
    // V1 the bare hash, as every app writes it
    ok(H.as_bytes(), H);
    // V2 md5sum's text line (RustCut up to 0.0.107)
    ok(format!("{H}  clip.mp4\n").as_bytes(), H);
    // V3 md5sum's binary-mode line
    ok(format!("{H} *clip.mp4\n").as_bytes(), H);
    // V4 a UTF-8 byte order mark first
    ok(format!("\u{feff}{H}").as_bytes(), H);
    // V5 md5sum's leading backslash for an escaped name
    ok(format!("\\{H}  a\\nb.mp4\n").as_bytes(), H);
    // V6 byte order mark and backslash
    ok(format!("\u{feff}\\{H}\n").as_bytes(), H);
    // V7 uppercase digits are lowercased
    ok(H.to_ascii_uppercase().as_bytes(), H);
    // V8 white space around, Windows line end
    ok(format!(" \t{H}\r\n").as_bytes(), H);
    // V9 a name that is not UTF-8 after the hash
    let mut content = format!("{H}  ").into_bytes();
    content.extend_from_slice(b"\xff\xfe.mp4\n");
    ok(&content, H);
    // V10 the zero-byte MD5 is a valid hash (rule 8 is about "known", not about reading)
    ok(ZERO_BYTE_MD5.as_bytes(), ZERO_BYTE_MD5);
    // V11 exactly MAX_LEN bytes
    let mut content = H.as_bytes().to_vec();
    content.resize(MAX_LEN, b' ');
    ok(&content, H);
}

#[test]
fn invalid_contents() {
    // I1 empty
    bad(b"", Invalid::Empty);
    // I2 only white space
    bad(b"\n  \r\n", Invalid::Empty);
    // I3 only a byte order mark
    bad("\u{feff}".as_bytes(), Invalid::Empty);
    // I4 text
    bad(b"not a hash", Invalid::NotAHash);
    // I5 31 and 33 digits
    bad(&H.as_bytes()[1..], Invalid::NotAHash);
    bad(format!("{H}0").as_bytes(), Invalid::NotAHash);
    // I6 a letter that is no hex digit
    bad(format!("{}g", &H[..31]).as_bytes(), Invalid::NotAHash);
    // I7 the name glued to the hash
    bad(format!("{H}*clip.mp4").as_bytes(), Invalid::NotAHash);
    // I8 a SHA-1 (40 digits)
    bad(b"2fd4e1c67a2d28fced849ee1bb76e7391b93eb12", Invalid::NotAHash);
    // I9 two backslashes (only one is skipped)
    bad(format!("\\\\{H}").as_bytes(), Invalid::NotAHash);
    // I10 one byte longer than MAX_LEN
    let mut content = H.as_bytes().to_vec();
    content.resize(MAX_LEN + 1, b' ');
    bad(&content, Invalid::TooLong);
}

#[test]
fn freshness() {
    let video = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    // F1 written after the video: fresh
    assert!(is_fresh(video + Duration::from_secs(5), video));
    // F2 the same time (coarse file systems): fresh
    assert!(is_fresh(video, video));
    // F3 one nanosecond older than the video: stale
    assert!(!is_fresh(video - Duration::from_nanos(1), video));
}

#[test]
fn zero_byte_hash_is_marked() {
    assert!(ZERO_BYTE_MD5.parse::<Md5Hex>().is_ok_and(|hash| hash.is_zero_byte()));
    assert!(!H.parse::<Md5Hex>().is_ok_and(|hash| hash.is_zero_byte()));
}
