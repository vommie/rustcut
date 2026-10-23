//! Mirrors of RustView's `crates/rustview-core/src/cli_api/models.rs` (checked 2026-10-23). Decoding is tolerant: every
//! field that RustView might leave out or add later has a default, unknown fields are ignored, so a newer RustView does not
//! break RustCut. The contract tests (M3.1) pin these shapes against fixtures and a test library.

use serde::{Deserialize, Serialize};

/// `tag-tree`: one tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagItem {
    pub id: u32,
    #[serde(default)]
    pub parent_id: Option<u32>,
    pub name: String,
}

/// `tag-tree` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagTree {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub total_tags: usize,
    #[serde(default)]
    pub tags: Vec<TagItem>,
}

/// A media record (`get-media`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaDetails {
    pub id: u64,
    pub folder_id: u64,
    pub folder_path: String,
    pub filename: String,
    pub full_path: String,
    pub file_size: u64,
    pub modification_time: i64,
    pub rating: u8,
    pub tags: Vec<u32>,
    /// MD5 (hex); empty when RustView has none.
    pub file_hash: String,
    pub width: u32,
    pub height: u32,
    pub media_type: String,
    pub format: String,
    pub is_deleted: bool,
    pub is_corrupted: bool,
    /// The app that registered the record (RustCut writes [`super::SOURCE_APP`]).
    pub source: Option<String>,
}

/// `get-media` response (exit code 2 = not found, with `found: false`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GetMedia {
    pub success: bool,
    pub found: bool,
    pub media: Option<MediaDetails>,
    pub message: Option<String>,
}

/// What `set-media` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetMediaAction {
    Created,
    #[default]
    Updated,
    /// A value a newer RustView may report.
    #[serde(other)]
    Other,
}

/// `set-media` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetMedia {
    pub success: bool,
    pub action: SetMediaAction,
    pub media_id: u64,
    pub folder_id: u64,
    pub filename: String,
    pub rating: u8,
    pub tags: Vec<u32>,
    pub file_hash: String,
    pub source: Option<String>,
    pub message: String,
}

/// `delete-media` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeleteMedia {
    pub success: bool,
    pub deleted: bool,
    pub media_id: Option<u64>,
    pub path: String,
    pub message: String,
}

/// `check-hash` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HashCheck {
    pub success: bool,
    pub known: bool,
    pub hash: String,
    pub hash_id: Option<u64>,
    pub date_time: Option<String>,
    /// Files of the library made from this source.
    pub associated_files: Vec<String>,
}

/// One result of `check-hashes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HashCheckItem {
    pub hash: String,
    pub known: bool,
    pub hash_id: Option<u64>,
    pub date_time: Option<String>,
    pub associated_files: Vec<String>,
}

/// `check-hashes` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchHashCheck {
    pub success: bool,
    pub total_requested: usize,
    pub known_count: usize,
    pub unknown_count: usize,
    pub results: Vec<HashCheckItem>,
}

/// `insert-hash` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HashInsert {
    pub success: bool,
    pub is_new: bool,
    pub hash: String,
    pub hash_id: u64,
    pub date_time: String,
    pub message: String,
}

/// `{success: false, error}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiErrorResponse {
    pub success: bool,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_documented_shapes() {
        let tree: TagTree = serde_json::from_str(
            r#"{"success":true,"total_tags":2,"tags":[{"id":1,"parent_id":null,"name":"Places"},{"id":2,"parent_id":1,"name":"Beach"}]}"#,
        )
        .unwrap();
        assert_eq!(
            tree.tags[1],
            TagItem {
                id: 2,
                parent_id: Some(1),
                name: "Beach".into()
            }
        );
        let missing: GetMedia =
            serde_json::from_str(r#"{"success":true,"found":false,"message":"Media not found"}"#).unwrap();
        assert!(!missing.found && missing.media.is_none());
        let set: SetMedia = serde_json::from_str(
            r#"{"success":true,"action":"created","media_id":7,"folder_id":3,"filename":"a.mkv","rating":4,"tags":[1,2],"file_hash":"","message":"ok"}"#,
        )
        .unwrap();
        assert_eq!(
            (set.action, set.media_id, set.source),
            (SetMediaAction::Created, 7, None)
        );
    }

    #[test]
    fn tolerates_additions_and_omissions() {
        let set: SetMedia = serde_json::from_str(r#"{"success":true,"action":"merged","new_field":[1]}"#).unwrap();
        assert_eq!(set.action, SetMediaAction::Other);
        let media: MediaDetails = serde_json::from_str(r#"{"id":1,"filename":"x.mkv"}"#).unwrap();
        assert_eq!(media.full_path, "");
    }
}
