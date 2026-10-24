//! The question before a job without tags or rating is saved (`warnings.no_tags` / `warnings.no_rating`; user decision
//! Q-G1): one modal dialog that names what is missing, offers the rating bar (a click saves with that rating), "Save
//! without", "Back to tagging" and "Don't ask again". Asked only while the RustView link is on; missing tags only when
//! RustView has tags to choose from.

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

    pub fn text(self) -> &'static str {
        match (self.tags, self.rating) {
            (true, true) => {
                "RustView gets the clip without tags and unrated. Choose a rating to save with it, save without, or go \
                 back to tagging."
            }
            (true, false) => "RustView gets the clip without tags. Save without them, or go back to tagging.",
            _ => "RustView gets the clip unrated. Choose a rating to save with it, or save without one.",
        }
    }

    /// The warnings "Don't ask again" switches off: the ones that asked.
    pub fn silence(self, warnings: &mut WarningSettings) {
        if self.tags {
            warnings.no_tags = false;
        }
        if self.rating {
            warnings.no_rating = false;
        }
    }
}

/// The facts the question is decided on.
#[derive(Debug, Clone, Copy)]
pub struct MetadataFacts {
    /// The RustView link is on.
    pub enabled: bool,
    /// RustView offered at least one tag.
    pub can_choose_tags: bool,
    pub has_tags: bool,
    pub has_rating: bool,
}

pub fn missing(facts: MetadataFacts, warnings: &WarningSettings) -> Missing {
    if !facts.enabled {
        return Missing::default();
    }
    Missing {
        tags: warnings.no_tags && facts.can_choose_tags && !facts.has_tags,
        rating: warnings.no_rating && !facts.has_rating,
    }
}

/// How the question was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Save with this rating.
    Rated(u8),
    SaveWithout,
    BackToTagging,
}

/// The open question with the job it is about.
pub(super) struct MetaPending {
    draft: Draft,
    missing: Missing,
    dont_ask: bool,
}

impl ExportBridge {
    /// `Some(draft)` = nothing to ask, save on; `None` = the question is open (it saves or not later).
    pub(super) fn ask_metadata(&self, draft: Draft) -> Option<Draft> {
        let tags = self.tags.upgrade();
        let facts = MetadataFacts {
            enabled: tags.as_ref().is_some_and(|t| t.enabled()),
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
        *self.meta.borrow_mut() = Some(MetaPending {
            draft,
            missing,
            dont_ask: false,
        });
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
        e.set_meta_text(pending.missing.text().into());
        e.set_meta_ask_rating(pending.missing.rating);
        e.set_meta_dont_ask(pending.dont_ask);
        e.set_meta_open(true);
    }

    /// Closes the question, and saves unless the answer goes back to tagging.
    fn answer_meta(self: &Rc<Self>, answer: Answer) {
        let Some(mut pending) = self.meta.borrow_mut().take() else {
            return;
        };
        self.publish_meta();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        if answer == Answer::BackToTagging {
            Self::tell(
                Notice::new(Kind::SaveCanceled, "Not saved").detail("Choose the tags and the rating, then save again."),
            );
            if let Some(tags) = self.tags.upgrade() {
                tags.reveal();
            }
            self.publish();
            return;
        }
        if pending.dont_ask {
            pending.missing.silence(&mut self.warnings.borrow_mut());
            let warnings = self.warnings.borrow().clone();
            if !self.writer.update(|s| s.warnings = warnings) {
                Self::tell(
                    Notice::new(Kind::SettingsReadOnly, "Not saved")
                        .detail("config.toml is from a newer RustCut: the choice is kept for this run only."),
                );
            }
        }
        if let Answer::Rated(rating) = answer {
            pending.draft.spec.metadata.rating = Some(rating);
        }
        self.check_target(pending.draft);
    }

    fn toggle_meta_dont_ask(&self) {
        if let Some(pending) = self.meta.borrow_mut().as_mut() {
            pending.dont_ask = !pending.dont_ask;
        }
        self.publish_meta();
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
            b.answer_meta(Answer::BackToTagging);
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_meta_toggle_dont_ask(move || {
        if let Some(b) = weak.upgrade() {
            b.toggle_meta_dont_ask();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(has_tags: bool, has_rating: bool) -> MetadataFacts {
        MetadataFacts {
            enabled: true,
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
        let mut quiet = WarningSettings { no_rating: false, ..on };
        assert_eq!(
            missing(facts(false, false), &quiet),
            Missing {
                tags: true,
                rating: false
            }
        );
        Missing {
            tags: true,
            rating: false,
        }
        .silence(&mut quiet);
        assert!(!quiet.no_tags && !quiet.no_rating && quiet.target_exists);
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
        assert!(
            Missing {
                tags: false,
                rating: true
            }
            .text()
            .contains("unrated")
        );
    }
}
