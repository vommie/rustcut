//! The source's MD5 from RustView's library (ADR-6: `get-media`'s `file_hash` before reading the file).

use super::models::MediaDetails;

/// Whether `text` is an MD5 as RustView stores it (32 hex digits).
pub fn is_md5(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The MD5 of a file from its RustView record, when the record describes the file as it is now: a complete record
/// (not deleted, not corrupted, not a pending stub, whose hash is a placeholder) with the file's current size and
/// modification time (seconds, as RustView stores them). `None` = read the file instead.
pub fn trusted_hash(media: &MediaDetails, size: u64, modified_secs: i64) -> Option<String> {
    let current = !media.is_deleted
        && !media.is_corrupted
        && media.file_size > 0
        && media.file_size == size
        && media.modification_time == modified_secs;
    (current && is_md5(&media.file_hash)).then(|| media.file_hash.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_current_complete_record_is_trusted() {
        let media = MediaDetails {
            file_size: 10_341_341,
            modification_time: 1_792_780_315,
            file_hash: "CCEC6B36FB7DAF1F41D7EB2437DE81A0".into(),
            ..MediaDetails::default()
        };
        assert_eq!(
            trusted_hash(&media, 10_341_341, 1_792_780_315).as_deref(),
            Some("ccec6b36fb7daf1f41d7eb2437de81a0")
        );
        assert_eq!(
            trusted_hash(&media, 10_341_340, 1_792_780_315),
            None,
            "the file changed"
        );
        assert_eq!(
            trusted_hash(&media, 10_341_341, 1_792_780_316),
            None,
            "the file changed"
        );
        let stub = MediaDetails {
            file_size: 0,
            ..media.clone()
        };
        assert_eq!(
            trusted_hash(&stub, 0, 1_792_780_315),
            None,
            "a pending stub's hash is a placeholder"
        );
        let corrupted = MediaDetails {
            is_corrupted: true,
            ..media.clone()
        };
        assert_eq!(trusted_hash(&corrupted, 10_341_341, 1_792_780_315), None);
        let odd = MediaDetails {
            file_hash: "not-a-hash".into(),
            ..media
        };
        assert_eq!(trusted_hash(&odd, 10_341_341, 1_792_780_315), None);
        assert!(is_md5("8f618d1ebd606025257057ee18536e64") && !is_md5("8f61"));
    }
}
