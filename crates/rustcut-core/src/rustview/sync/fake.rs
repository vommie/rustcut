//! An in-memory RustView for the sync tests: the API's semantics as `crates/rustview-core/src/cli_api/handlers.rs`
//! implements them (upsert by path, unknown tag ids left out, a new record without `--source` gets "CLI-API", a hash is
//! known through the hash list or a record with that hash), every call logged, and failures on demand.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::rustview::{
    BatchHashCheck, DeleteMedia, GetMedia, HashCheck, HashInsert, MediaDetails, MoveMedia, MoveMediaAction,
    RustViewClient, RustViewError, SOURCE_APP, SetMedia, SetMediaAction, TagItem, TagTree,
};

#[derive(Debug, Default)]
pub(crate) struct Library {
    pub media: BTreeMap<PathBuf, MediaDetails>,
    pub hashes: Vec<String>,
    pub tags: Vec<u32>,
    /// Every call as `command path-or-hash`.
    pub calls: Vec<String>,
    /// Errors for the next calls, in order.
    pub failures: VecDeque<RustViewError>,
    /// While set, every call fails as if RustView were not installed.
    pub unreachable: bool,
    /// While set, every call waits forever (a hanging RustView).
    pub hang: bool,
    next_id: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FakeRustView(pub Arc<Mutex<Library>>);

impl FakeRustView {
    /// A library that knows the tags 1 to 10.
    pub fn new() -> Self {
        let fake = Self::default();
        fake.0.lock().tags = (1..=10).collect();
        fake
    }

    pub fn calls(&self) -> Vec<String> {
        self.0.lock().calls.clone()
    }

    /// The calls that change the library.
    pub fn writes(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| {
                c.starts_with("set") || c.starts_with("delete") || c.starts_with("insert") || c.starts_with("move")
            })
            .collect()
    }

    pub fn media(&self, path: &str) -> Option<MediaDetails> {
        self.0.lock().media.get(Path::new(path)).cloned()
    }

    /// A record that existed before RustCut (e.g. a file RustView scanned).
    pub fn put(&self, path: &str, tags: &[u32], rating: u8, source: Option<&str>) {
        let mut lib = self.0.lock();
        lib.next_id += 1;
        let id = lib.next_id;
        lib.media.insert(
            path.into(),
            MediaDetails {
                id,
                full_path: path.into(),
                tags: tags.to_vec(),
                rating,
                file_hash: format!("{id:032x}"),
                source: source.map(str::to_owned),
                ..MediaDetails::default()
            },
        );
    }

    pub fn set_unreachable(&self, unreachable: bool) {
        self.0.lock().unreachable = unreachable;
    }

    pub fn fail_next(&self, error: RustViewError) {
        self.0.lock().failures.push_back(error);
    }

    /// Logs the call and returns the failure it should end with.
    async fn call(&self, call: String) -> Result<(), RustViewError> {
        let (answer, hang) = {
            let mut lib = self.0.lock();
            lib.calls.push(call);
            let answer = if lib.unreachable {
                Err(RustViewError::Unavailable(
                    "rustview is not installed (or not in PATH)".into(),
                ))
            } else {
                lib.failures.pop_front().map_or(Ok(()), Err)
            };
            (answer, lib.hang)
        };
        if hang {
            std::future::pending::<()>().await;
        }
        answer
    }
}

impl RustViewClient for FakeRustView {
    async fn tag_tree(&self) -> Result<TagTree, RustViewError> {
        self.call("tag-tree".into()).await?;
        let tags: Vec<TagItem> = self
            .0
            .lock()
            .tags
            .iter()
            .map(|&id| TagItem {
                id,
                parent_id: None,
                name: format!("Tag {id}"),
            })
            .collect();
        Ok(TagTree {
            success: true,
            total_tags: tags.len(),
            tags,
        })
    }

    async fn get_media(&self, path: &Path) -> Result<GetMedia, RustViewError> {
        self.call(format!("get-media {}", path.display())).await?;
        let media = self.0.lock().media.get(path).cloned();
        Ok(GetMedia {
            success: true,
            found: media.is_some(),
            media,
            message: None,
        })
    }

    async fn set_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> Result<SetMedia, RustViewError> {
        self.call(format!(
            "set-media {} rating={rating:?} tags={tags:?} hash={hash:?}",
            path.display()
        ))
        .await?;
        Ok(self.upsert(path, rating, tags, hash, None))
    }

    async fn set_own_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> Result<SetMedia, RustViewError> {
        self.call(format!(
            "set-own-media {} rating={rating:?} tags={tags:?} hash={hash:?}",
            path.display()
        ))
        .await?;
        Ok(self.upsert(path, rating, tags, hash, Some(SOURCE_APP)))
    }

    async fn delete_media(&self, path: &Path) -> Result<DeleteMedia, RustViewError> {
        self.call(format!("delete-media {}", path.display())).await?;
        let removed = self.0.lock().media.remove(path);
        Ok(DeleteMedia {
            success: true,
            deleted: removed.is_some(),
            media_id: removed.map(|m| m.id),
            path: path.display().to_string(),
            message: String::new(),
        })
    }

    async fn move_media(&self, from: &Path, to: &Path, replace: bool) -> Result<MoveMedia, RustViewError> {
        self.call(format!("move-media {} {}", from.display(), to.display()))
            .await?;
        let mut lib = self.0.lock();
        let source = lib.media.get(from).cloned();
        let target = lib.media.get(to).cloned();
        let (action, record, replaced) = match (source, target) {
            (None, None) => (MoveMediaAction::NotFound, None, None),
            (None, Some(there)) => (MoveMediaAction::AlreadyMoved, Some(there), None),
            (Some(rec), Some(there)) if rec.id == there.id => (MoveMediaAction::Unchanged, Some(rec), None),
            (Some(_), Some(there)) if !replace => {
                return Err(RustViewError::Api(format!(
                    "Media #{} is registered at the target path already",
                    there.id
                )));
            }
            (Some(mut rec), there) => {
                lib.media.remove(from);
                rec.full_path = to.display().to_string();
                lib.media.insert(to.to_path_buf(), rec.clone());
                (MoveMediaAction::Moved, Some(rec), there.map(|t| t.id))
            }
        };
        Ok(MoveMedia {
            success: true,
            action,
            media_id: record.as_ref().map(|r| r.id),
            from: from.display().to_string(),
            to: to.display().to_string(),
            replaced_media_id: replaced,
            rating: record.as_ref().map(|r| r.rating),
            tags: record.as_ref().map(|r| r.tags.clone()).unwrap_or_default(),
            file_hash: record.as_ref().map(|r| r.file_hash.clone()),
            source: record.and_then(|r| r.source),
            message: String::new(),
        })
    }

    async fn check_hash(&self, md5: &str) -> Result<HashCheck, RustViewError> {
        self.call(format!("check-hash {md5}")).await?;
        let lib = self.0.lock();
        let listed = lib.hashes.iter().position(|h| h == md5);
        let associated_files: Vec<String> = lib
            .media
            .iter()
            .filter(|(_, m)| m.file_hash == md5)
            .map(|(p, _)| p.display().to_string())
            .collect();
        Ok(HashCheck {
            success: true,
            known: listed.is_some() || !associated_files.is_empty(),
            hash: md5.into(),
            hash_id: listed.and_then(|i| u64::try_from(i).ok()).map(|i| i + 1),
            date_time: listed.map(|_| "2026-10-23 12:00:00".into()),
            associated_files,
        })
    }

    async fn check_hashes(&self, md5s: &[String]) -> Result<BatchHashCheck, RustViewError> {
        self.call(format!("check-hashes {}", md5s.join(","))).await?;
        Ok(BatchHashCheck::default())
    }

    async fn insert_hash(&self, md5: &str) -> Result<HashInsert, RustViewError> {
        self.call(format!("insert-hash {md5}")).await?;
        let mut lib = self.0.lock();
        let is_new = !lib.hashes.iter().any(|h| h == md5);
        if is_new {
            lib.hashes.push(md5.into());
        }
        Ok(HashInsert {
            success: true,
            is_new,
            hash: md5.into(),
            ..HashInsert::default()
        })
    }
}

impl FakeRustView {
    fn upsert(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
        source: Option<&str>,
    ) -> SetMedia {
        let mut lib = self.0.lock();
        let known: Vec<u32> = tags
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|t| lib.tags.contains(t))
            .collect();
        let created = !lib.media.contains_key(path);
        if created {
            lib.next_id += 1;
        }
        let id = lib.next_id;
        let media = lib.media.entry(path.into()).or_insert_with(|| MediaDetails {
            id,
            full_path: path.display().to_string(),
            source: Some("CLI-API".into()),
            file_hash: format!("{id:032x}"),
            ..MediaDetails::default()
        });
        if let Some(rating) = rating {
            media.rating = rating.min(5);
        }
        if tags.is_some() {
            media.tags = known;
        }
        if let Some(hash) = hash {
            media.file_hash = hash.into();
        }
        if let Some(source) = source {
            media.source = Some(source.into());
        }
        SetMedia {
            success: true,
            action: if created {
                SetMediaAction::Created
            } else {
                SetMediaAction::Updated
            },
            media_id: media.id,
            rating: media.rating,
            tags: media.tags.clone(),
            file_hash: media.file_hash.clone(),
            source: media.source.clone(),
            ..SetMedia::default()
        }
    }
}
