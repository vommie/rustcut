//! The Tag & Rate panel's state, without Slint: the tag tree as RustView last sent it, the tags and rating chosen for the
//! next job (the draft) or for a queued job being edited (user decision Q-G3), locked tags that stay for the next jobs
//! (Q-G2), hidden branches, the filter, the expanded branches and the tag history. Every operation is a method; the
//! bridge only shows what [`TagPanel::rows`] and the other views say.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use regex::{Regex, RegexBuilder};
use rustcut_core::job::{JobId, JobMetadata};
use rustcut_core::rustview::{TagItem, TagTree};
use rustcut_core::settings::TagPanelState;

use super::history::{TagHistoryItem, TagHistoryManager};
use super::tree::{TagFlattenContext, TagTreeBuilder, TagTreeNode, descendants};
use crate::TagNodeView;

/// Tags and rating of one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    /// In the order they were assigned.
    pub tags: Vec<u32>,
    /// `None` = no rating chosen.
    pub rating: Option<u8>,
}

impl Choice {
    fn toggle(&mut self, id: u32) -> bool {
        if let Some(at) = self.tags.iter().position(|t| *t == id) {
            self.tags.remove(at);
            false
        } else {
            self.tags.push(id);
            true
        }
    }

    fn add(&mut self, ids: &[u32]) {
        for &id in ids {
            if !self.tags.contains(&id) {
                self.tags.push(id);
            }
        }
    }
}

/// A queued job whose RustView entry is being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub id: JobId,
    pub name: String,
    /// What the job holds (its source hash stays).
    pub original: JobMetadata,
    pub choice: Choice,
}

impl Editing {
    fn original_choice(&self) -> Choice {
        Choice {
            tags: self.original.tags.clone(),
            rating: self.original.rating,
        }
    }
}

#[derive(Debug, Default)]
pub struct TagPanel {
    tree: Option<Arc<TagTree>>,
    forest: Vec<TagTreeNode>,
    by_id: HashMap<u32, TagItem>,
    pub expanded: HashSet<u32>,
    pub filter: String,
    pub regex: bool,
    pub history: Vec<u32>,
    pub history_collapsed: bool,
    pub history_height: u32,
    pub hidden: HashSet<u32>,
    /// Tags shown again inside a hidden branch (with everything below them).
    pub shown: HashSet<u32>,
    pub locked: Vec<u32>,
    pub last_tags: Vec<u32>,
    pub last_rating: Option<u8>,
    /// The next job's tags and rating.
    pub draft: Choice,
    pub editing: Option<Editing>,
    /// Choosing the branches to hide (a click hides or shows one).
    pub hide_mode: bool,
    /// The "Active" switch (user decision Q-P8b): off = the next jobs are saved without RustView.
    pub active: bool,
    /// The tag filter window: its folded branches and its size.
    pub filter_collapsed: HashSet<u32>,
    pub filter_size: (u32, u32),
    /// How many tags the history keeps (`rustview.tag_history_size`).
    history_size: usize,
}

impl TagPanel {
    pub fn new(saved: &TagPanelState) -> Self {
        Self {
            expanded: saved.expanded.iter().copied().collect(),
            filter: saved.filter.clone(),
            regex: saved.regex,
            history: saved.history.clone(),
            history_collapsed: saved.history_collapsed,
            history_height: saved.history_height,
            hidden: saved.hidden.iter().copied().collect(),
            shown: saved.shown.iter().copied().collect(),
            locked: saved.locked.clone(),
            last_tags: saved.last_tags.clone(),
            last_rating: saved.last_rating,
            draft: Choice {
                tags: saved.locked.clone(),
                rating: None,
            },
            active: saved.active,
            filter_collapsed: saved.filter_collapsed.iter().copied().collect(),
            filter_size: (saved.filter_width, saved.filter_height),
            history_size: TagPanelState::HISTORY_DEFAULT,
            ..Self::default()
        }
    }

    /// The history keeps this many tags from now on (the oldest go when it is shorter now).
    pub fn set_history_size(&mut self, size: usize) {
        self.history_size = size.clamp(1, TagPanelState::HISTORY_MAX);
        self.history.truncate(self.history_size);
    }

    /// What `ui-state.toml` keeps (sets sorted, so the file does not change on every save).
    pub fn save_into(&self, saved: &mut TagPanelState) {
        let sorted = |set: &HashSet<u32>| {
            let mut v: Vec<u32> = set.iter().copied().collect();
            v.sort_unstable();
            v
        };
        *saved = TagPanelState {
            expanded: sorted(&self.expanded),
            filter: self.filter.clone(),
            regex: self.regex,
            history: self.history.clone(),
            history_collapsed: self.history_collapsed,
            history_height: self.history_height,
            hidden: sorted(&self.hidden),
            shown: sorted(&self.shown),
            locked: self.locked.clone(),
            last_tags: self.last_tags.clone(),
            last_rating: self.last_rating,
            active: self.active,
            filter_collapsed: sorted(&self.filter_collapsed),
            filter_width: self.filter_size.0,
            filter_height: self.filter_size.1,
        };
    }

    /// RustView sent its tags: ids it does not know any more are dropped everywhere (the edited job keeps its own until
    /// it is applied, so nothing changes in it unasked).
    pub fn set_tree(&mut self, tree: Arc<TagTree>) {
        self.forest = TagTreeBuilder::build_hierarchy(&tree.tags);
        self.by_id = tree.tags.iter().map(|t| (t.id, t.clone())).collect();
        let valid: HashSet<u32> = self.by_id.keys().copied().collect();
        for set in [
            &mut self.expanded,
            &mut self.hidden,
            &mut self.shown,
            &mut self.filter_collapsed,
        ] {
            set.retain(|id| valid.contains(id));
        }
        TagHistoryManager::prune_invalid(&mut self.history, &valid);
        for list in [&mut self.locked, &mut self.last_tags, &mut self.draft.tags] {
            list.retain(|id| valid.contains(id));
        }
        self.tree = Some(tree);
    }

    pub fn has_tree(&self) -> bool {
        self.tree.is_some()
    }

    pub fn tag_count(&self) -> usize {
        self.by_id.len()
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(|t| t.name.trim())
    }

    /// The choice the panel edits now.
    pub fn choice(&self) -> &Choice {
        self.editing.as_ref().map_or(&self.draft, |e| &e.choice)
    }

    fn choice_mut(&mut self) -> &mut Choice {
        match &mut self.editing {
            Some(e) => &mut e.choice,
            None => &mut self.draft,
        }
    }

    /// A click on a tag in the tree: assigns or unassigns it, or hides/shows its branch while branches are chosen.
    /// Unassigning a locked tag unlocks it.
    pub fn click_tag(&mut self, id: u32) {
        if !self.by_id.contains_key(&id) {
            return;
        }
        if self.hide_mode {
            if !self.hidden.remove(&id) {
                self.hidden.insert(id);
            }
            return;
        }
        let assigned = self.choice_mut().toggle(id);
        if assigned {
            TagHistoryManager::record_tag_used(&mut self.history, id, self.history_size);
        } else if self.editing.is_none() {
            self.locked.retain(|t| *t != id);
        }
    }

    /// A click on a pill of the tag history (never hides a branch).
    pub fn click_history(&mut self, id: u32) {
        let hide_mode = std::mem::replace(&mut self.hide_mode, false);
        self.click_tag(id);
        self.hide_mode = hide_mode;
    }

    /// A right click locks the tag (and assigns it) or unlocks it (it stays assigned for this job). Only for the draft:
    /// a lock is about the next jobs.
    pub fn toggle_lock(&mut self, id: u32) -> bool {
        if self.editing.is_some() || self.hide_mode || !self.by_id.contains_key(&id) {
            return false;
        }
        if let Some(at) = self.locked.iter().position(|t| *t == id) {
            self.locked.remove(at);
        } else {
            self.locked.push(id);
            if !self.draft.tags.contains(&id) {
                self.draft.tags.push(id);
                TagHistoryManager::record_tag_used(&mut self.history, id, self.history_size);
            }
        }
        true
    }

    /// A click on a rating button: chooses it, or clears it when it was the chosen one.
    pub fn click_rating(&mut self, rating: i32) {
        let rating = u8::try_from(rating).ok().filter(|r| *r <= 5);
        let choice = self.choice_mut();
        choice.rating = if choice.rating == rating { None } else { rating };
    }

    /// Clears tags and rating; the draft keeps its locked tags.
    pub fn clear(&mut self) {
        let keep = if self.editing.is_some() {
            Vec::new()
        } else {
            self.locked.clone()
        };
        *self.choice_mut() = Choice {
            tags: keep,
            rating: None,
        };
    }

    pub fn can_clear(&self) -> bool {
        let choice = self.choice();
        let locked = if self.editing.is_some() { 0 } else { self.locked.len() };
        choice.rating.is_some() || choice.tags.len() > locked
    }

    pub fn apply_last_tags(&mut self) {
        let last = self.last_tags.clone();
        self.choice_mut().add(&last);
    }

    pub fn apply_last_rating(&mut self) {
        if let Some(rating) = self.last_rating {
            self.choice_mut().rating = Some(rating);
        }
    }

    pub fn toggle_expanded(&mut self, id: u32) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    pub fn expand_all(&mut self) {
        self.expanded = self.by_id.keys().copied().collect();
    }

    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    pub fn toggle_hide_mode(&mut self) {
        self.hide_mode = !self.hide_mode;
    }

    /// The filter as a regular expression (case-insensitive) when the regex switch is on; `Err` = it does not compile
    /// and the text is matched as a substring (RustView's fallback).
    pub fn filter_regex(&self) -> Result<Option<Regex>, regex::Error> {
        let text = self.filter.trim();
        if !self.regex || text.is_empty() {
            return Ok(None);
        }
        RegexBuilder::new(text)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .map(Some)
            .inspect_err(|e| {
                tracing::debug!(error = %e, "[Tags] The tag filter is not a valid regular expression; matching as text");
            })
    }

    /// The visible rows of the tree.
    pub fn rows(&self) -> Vec<TagNodeView> {
        let regex = self.filter_regex().ok().flatten();
        let filter = self.filter.trim().to_lowercase();
        let assigned: HashSet<u32> = self.choice().tags.iter().copied().collect();
        let locked: HashSet<u32> = if self.editing.is_some() {
            HashSet::new()
        } else {
            self.locked.iter().copied().collect()
        };
        let ctx = TagFlattenContext {
            expanded_set: &self.expanded,
            filter_text: &filter,
            filter_regex: regex.as_ref(),
            assigned: &assigned,
            locked: &locked,
            hidden: &self.hidden,
            show_hidden: self.hide_mode,
        };
        let mut rows = Vec::new();
        TagTreeBuilder::flatten(&self.forest, 0, &ctx, &mut rows);
        rows
    }

    pub fn history_items(&self) -> Vec<TagHistoryItem> {
        let assigned: HashSet<u32> = self.choice().tags.iter().copied().collect();
        TagHistoryManager::items(&self.history, &self.by_id, &assigned)
    }

    /// Tags of the choice that a hidden branch keeps out of the tree (they are still assigned).
    pub fn assigned_in_hidden(&self) -> usize {
        let Some(tree) = &self.tree else { return 0 };
        let mut hidden_ids = self.hidden.clone();
        for id in &self.hidden {
            hidden_ids.extend(descendants(&tree.tags, *id));
        }
        self.choice().tags.iter().filter(|t| hidden_ids.contains(t)).count()
    }

    /// The draft as the next job's metadata (tags sorted, as RustView stores them).
    pub fn draft_metadata(&self) -> (Vec<u32>, Option<u8>) {
        let mut tags = self.draft.tags.clone();
        tags.sort_unstable();
        (tags, self.draft.rating)
    }

    /// A job was saved with `tags` and `rating`: they become "last", the draft starts again with the locked tags.
    pub fn saved(&mut self, tags: &[u32], rating: Option<u8>) {
        if !tags.is_empty() {
            self.last_tags = tags.to_vec();
        }
        if rating.is_some() {
            self.last_rating = rating;
        }
        self.draft = Choice {
            tags: self.locked.clone(),
            rating: None,
        };
    }

    /// A job loaded into the editor (or a draft put back): its tags and rating become the draft. Locked tags stay
    /// locked; an entry being edited is left.
    pub fn load_draft(&mut self, choice: Choice) {
        self.editing = None;
        self.draft = choice;
    }

    /// Starts editing a queued job's entry; the draft waits meanwhile.
    pub fn begin_edit(&mut self, id: JobId, name: String, metadata: JobMetadata) {
        let choice = Choice {
            tags: metadata.tags.clone(),
            rating: metadata.rating,
        };
        self.hide_mode = false;
        self.editing = Some(Editing {
            id,
            name,
            original: metadata,
            choice,
        });
    }

    /// The edited job's new metadata, when it differs from what the job holds.
    pub fn edited(&self) -> Option<(JobId, JobMetadata)> {
        let e = self.editing.as_ref()?;
        if e.choice == e.original_choice() {
            return None;
        }
        let mut tags = e.choice.tags.clone();
        tags.sort_unstable();
        Some((
            e.id,
            JobMetadata {
                tags,
                rating: e.choice.rating,
                source_hash: e.original.source_hash.clone(),
                without_rustview: e.original.without_rustview,
            },
        ))
    }

    /// Back to the draft (after Apply or Cancel).
    pub fn end_edit(&mut self) -> Option<Editing> {
        self.editing.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Arc<TagTree> {
        let tags = [
            (1, None, "Places"),
            (2, Some(1), "Beach"),
            (3, Some(1), "Alps"),
            (4, None, "Anna"),
        ]
        .into_iter()
        .map(|(id, parent_id, name)| TagItem {
            id,
            parent_id,
            name: name.into(),
        })
        .collect();
        Arc::new(TagTree {
            success: true,
            total_tags: 4,
            tags,
        })
    }

    fn panel() -> TagPanel {
        let mut p = TagPanel::new(&TagPanelState::default());
        p.set_tree(tree());
        p
    }

    #[test]
    fn clicks_assign_record_history_and_rating_toggles() {
        let mut p = panel();
        p.click_tag(2);
        p.click_tag(4);
        p.click_tag(99);
        assert_eq!(p.draft.tags, [2, 4]);
        assert_eq!(p.history, [4, 2]);
        p.click_tag(2);
        assert_eq!((p.draft.tags.as_slice(), p.history.as_slice()), (&[4][..], &[4, 2][..]));
        p.click_rating(3);
        assert_eq!(p.draft.rating, Some(3));
        p.click_rating(3);
        assert_eq!(p.draft.rating, None, "the chosen rating again clears it");
        p.click_rating(7);
        assert_eq!(p.draft.rating, None);
        p.click_history(2);
        assert_eq!(p.draft.tags, [4, 2]);
    }

    #[test]
    fn locked_tags_survive_a_save_and_clear() {
        let mut p = panel();
        p.click_tag(2);
        assert!(p.toggle_lock(4));
        assert_eq!(p.draft.tags, [2, 4], "locking assigns");
        p.click_rating(5);
        assert_eq!(p.draft_metadata(), (vec![2, 4], Some(5)));
        p.saved(&[2, 4], Some(5));
        assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4], None));
        assert_eq!((p.last_tags.clone(), p.last_rating), (vec![2, 4], Some(5)));
        p.apply_last_tags();
        p.apply_last_rating();
        assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4, 2], Some(5)));
        assert!(p.can_clear());
        p.clear();
        assert_eq!(p.draft.tags, [4]);
        assert!(!p.can_clear(), "only the locked tag is left");
        // A save without tags or rating keeps the last ones.
        p.saved(&[], None);
        assert_eq!((p.last_tags.clone(), p.last_rating), (vec![2, 4], Some(5)));
        // Unlocking keeps the tag for this job; unassigning a locked tag unlocks it.
        p.toggle_lock(4);
        assert_eq!((p.locked.is_empty(), p.draft.tags.clone()), (true, vec![4]));
        p.toggle_lock(4);
        p.click_tag(4);
        assert!(p.locked.is_empty() && p.draft.tags.is_empty());
    }

    #[test]
    fn editing_a_job_leaves_the_draft_alone() {
        let mut p = panel();
        p.toggle_lock(4);
        p.click_rating(2);
        let original = JobMetadata {
            tags: vec![3],
            rating: Some(1),
            source_hash: Some("abc".into()),
            without_rustview: false,
        };
        p.begin_edit(JobId(7), "Clip - 01".into(), original);
        assert_eq!(p.choice().tags, [3]);
        assert!(p.edited().is_none(), "nothing changed yet");
        assert!(!p.toggle_lock(2), "no locks while a job is edited");
        p.click_tag(2);
        p.click_rating(4);
        let (id, metadata) = p.edited().unwrap();
        assert_eq!(id, JobId(7));
        assert_eq!(
            metadata,
            JobMetadata {
                tags: vec![2, 3],
                rating: Some(4),
                source_hash: Some("abc".into()),
                without_rustview: false,
            }
        );
        p.clear();
        assert_eq!(
            p.choice(),
            &Choice::default(),
            "clearing an edited job clears everything"
        );
        p.end_edit();
        assert_eq!((p.draft.tags.clone(), p.draft.rating), (vec![4], Some(2)));
    }

    #[test]
    fn hiding_branches_and_a_new_tree_prune_unknown_ids() {
        let mut p = panel();
        p.click_tag(2);
        p.toggle_hide_mode();
        p.click_tag(1);
        p.toggle_hide_mode();
        assert!(p.hidden.contains(&1));
        assert_eq!(p.draft.tags, [2], "hide mode does not assign");
        assert_eq!(p.assigned_in_hidden(), 1);
        assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [4]);
        p.expand_all();
        p.toggle_hide_mode();
        assert_eq!(p.rows().len(), 4, "every tag shows while branches are chosen");
        p.toggle_hide_mode();

        let mut smaller = (*tree()).clone();
        smaller.tags.retain(|t| t.id == 4);
        p.set_tree(Arc::new(smaller));
        assert!(p.hidden.is_empty() && p.draft.tags.is_empty() && p.history.is_empty());
        assert_eq!(p.expanded, [4].into_iter().collect());
    }

    #[test]
    fn regex_filter_falls_back_and_state_round_trips() {
        let mut p = panel();
        p.expand_all();
        p.filter = "be".into();
        assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2]);
        p.regex = true;
        p.filter = "^(al|an)".into();
        assert_eq!(p.rows().iter().map(|r| r.id).collect::<Vec<_>>(), [4, 1, 3]);
        p.filter = "(".into();
        assert!(p.filter_regex().is_err());
        assert!(p.rows().is_empty(), "matched as text: no tag holds a bracket");

        p.toggle_lock(2);
        p.history_height = 120;
        let mut saved = TagPanelState::default();
        p.save_into(&mut saved);
        let restored = TagPanel::new(&saved);
        assert_eq!(restored.draft.tags, [2], "a locked tag is assigned after a restart");
        let mut again = TagPanelState::default();
        restored.save_into(&mut again);
        assert_eq!(again, saved);
    }
}
