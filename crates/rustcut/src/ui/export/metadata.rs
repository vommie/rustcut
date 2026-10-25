//! The question before a job without tags or rating is saved (`warnings.missing_metadata`, the Tag & Rate panel's
//! "Warn" switch; user decisions Q-G1, Q-P8b, review 52): one compact modal dialog whose title names what is missing,
//! with the rating bar (a click saves with that rating), "Save without" and "Back to editing". "Don't ask again" is no
//! longer in the dialog (the user's review overrides Q-G1): the setting stays in Settings and on the panel. Asked only
//! while the RustView link and the panel's "Active" switch are on; missing tags only when RustView has tags to choose
//! from.

use std::rc::Rc;

use rustcut_core::settings::WarningSettings;
use slint::ComponentHandle as _;

use super::ExportBridge;
use super::save::Draft;
use crate::ui::notices::{Kind, Notice};
use crate::{Export, MainWindow};

/// What the job lacks that the warnings ask about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Missing {
    pub tags: bool,
    pub rating: bool,
}

impl Missing {
    pub fn any(self) -> bool {
        self.tags || self.rating
    }

    pub fn title(self) -> &'static str {
        match (self.tags, self.rating) {
            (true, true) => "The job has no tags and no rating",
            (true, false) => "The job has no tags",
            _ => "The job has no rating",
        }
    }
}

/// The facts the question is decided on.
#[derive(Debug, Clone, Copy)]
pub struct MetadataFacts {
    /// The RustView link is on.
    pub enabled: bool,
    /// The Tag & Rate panel's "Active" switch is on (off: the job is saved without RustView, Q-P8b).
    pub active: bool,
    /// RustView offered at least one tag.
    pub can_choose_tags: bool,
    pub has_tags: bool,
    pub has_rating: bool,
}

pub fn missing(facts: MetadataFacts, warnings: &WarningSettings) -> Missing {
    if !facts.enabled || !facts.active || !warnings.missing_metadata {
        return Missing::default();
    }
    Missing {
        tags: facts.can_choose_tags && !facts.has_tags,
        rating: !facts.has_rating,
    }
}

/// How the question was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Save with this rating.
    Rated(u8),
    SaveWithout,
    BackToEditing,
}

/// The open question with the job it is about.
pub(super) struct MetaPending {
    draft: Draft,
    missing: Missing,
}

impl ExportBridge {
    /// `Some(draft)` = nothing to ask, save on; `None` = the question is open (it saves or not later).
    pub(super) fn ask_metadata(&self, draft: Draft) -> Option<Draft> {
        let tags = self.tags.upgrade();
        let facts = MetadataFacts {
            enabled: tags.as_ref().is_some_and(|t| t.enabled()),
            active: !draft.spec.metadata.without_rustview,
            can_choose_tags: tags.as_ref().is_some_and(|t| t.can_choose_tags()),
            has_tags: !draft.spec.metadata.tags.is_empty(),
            has_rating: draft.spec.metadata.rating.is_some(),
        };
        let missing = missing(facts, &self.warnings.borrow());
        if !missing.any() {
            return Some(draft);
        }
        tracing::info!(
            tags = missing.tags,
            rating = missing.rating,
            "[Export] The job lacks metadata; asking"
        );
        *self.meta.borrow_mut() = Some(MetaPending { draft, missing });
        self.publish_meta();
        self.publish();
        None
    }

    fn publish_meta(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let e = window.global::<Export<'_>>();
        let meta = self.meta.borrow();
        let Some(pending) = meta.as_ref() else {
            e.set_meta_open(false);
            return;
        };
        e.set_meta_title(pending.missing.title().into());
        e.set_meta_ask_rating(pending.missing.rating);
        e.set_meta_open(true);
    }

    /// Closes the question, and saves unless the answer goes back to editing.
    fn answer_meta(self: &Rc<Self>, answer: Answer) {
        let Some(mut pending) = self.meta.borrow_mut().take() else {
            return;
        };
        self.publish_meta();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        if answer == Answer::BackToEditing {
            Self::tell(
                Notice::new(Kind::SaveCanceled, "Not saved").detail("Choose the tags and the rating, then save again."),
            );
            if let Some(tags) = self.tags.upgrade() {
                tags.reveal();
            }
            self.publish();
            return;
        }
        if let Answer::Rated(rating) = answer {
            pending.draft.spec.metadata.rating = Some(rating);
        }
        self.check_target(pending.draft);
    }
}

pub(super) fn connect(bridge: &Rc<ExportBridge>, window: &MainWindow) {
    let e = window.global::<Export<'_>>();
    let weak = Rc::downgrade(bridge);
    e.on_meta_rate(move |rating| {
        if let (Some(b), Ok(rating)) = (weak.upgrade(), u8::try_from(rating)) {
            b.answer_meta(Answer::Rated(rating.min(5)));
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_meta_save(move || {
        if let Some(b) = weak.upgrade() {
            b.answer_meta(Answer::SaveWithout);
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_meta_back(move || {
        if let Some(b) = weak.upgrade() {
            b.answer_meta(Answer::BackToEditing);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(has_tags: bool, has_rating: bool) -> MetadataFacts {
        MetadataFacts {
            enabled: true,
            active: true,
            can_choose_tags: true,
            has_tags,
            has_rating,
        }
    }

    #[test]
    fn asks_only_for_what_is_missing_and_switched_on() {
        let on = WarningSettings::default();
        assert_eq!(
            missing(facts(false, false), &on),
            Missing {
                tags: true,
                rating: true
            }
        );
        assert_eq!(
            missing(facts(true, false), &on),
            Missing {
                tags: false,
                rating: true
            }
        );
        assert!(!missing(facts(true, true), &on).any());
        let off = MetadataFacts {
            enabled: false,
            ..facts(false, false)
        };
        assert!(
            !missing(off, &on).any(),
            "nothing is asked while RustView is switched off"
        );
        let no_tree = MetadataFacts {
            can_choose_tags: false,
            ..facts(false, true)
        };
        assert!(!missing(no_tree, &on).any(), "no tags to choose: not asked about tags");
        let inactive = MetadataFacts {
            active: false,
            ..facts(false, false)
        };
        assert!(
            !missing(inactive, &on).any(),
            "nothing is asked while the panel's Active switch is off"
        );
        let quiet = WarningSettings {
            missing_metadata: false,
            ..on
        };
        assert!(!missing(facts(false, false), &quiet).any(), "Warn off asks nothing");
    }

    #[test]
    fn titles_name_what_is_missing() {
        assert!(
            Missing {
                tags: true,
                rating: true
            }
            .title()
            .contains("no tags and no rating")
        );
        assert_eq!(
            Missing {
                tags: false,
                rating: true
            }
            .title(),
            "The job has no rating"
        );
    }
}
