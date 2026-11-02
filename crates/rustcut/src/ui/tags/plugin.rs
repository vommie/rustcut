//! What plugins see of and do with the Tag & Rate panel (P12 block 8, Q-P12g): the user's tag clicks become events,
//! a plugin's rating gets a mark at the rating bar until the user changes the rating.

use rustcut_core::plugins::Tag;
use slint::ComponentHandle as _;

use super::TagsBridge;
use crate::Playback;
use crate::ui::export::PluginMark;

impl TagsBridge {
    /// `hook` hears every tag the user assigns or removes.
    pub fn set_on_user_tag(&self, hook: impl Fn(u32, bool) + 'static) {
        *self.on_user_tag.borrow_mut() = Some(Box::new(hook));
    }

    /// The user clicked tag `id` (tree or history), after the panel changed.
    pub(super) fn user_clicked_tag(&self, id: u32) {
        let assigned = self.state.borrow().choice().tags.contains(&id);
        if let Some(hook) = self.on_user_tag.borrow().as_ref() {
            hook(id, assigned);
        }
    }

    /// The user set or cleared the rating: a plugin's mark goes.
    pub(super) fn user_rated(&self) {
        if self.rating_mark.replace(None).is_some() {
            self.publish();
        }
    }

    /// A tag with its path, as plugins get it.
    pub fn plugin_tag(&self, id: u32) -> Option<Tag> {
        let state = self.state.borrow();
        let name = state.name(id)?.to_owned();
        Some(Tag {
            id,
            name,
            path: state.path(id),
        })
    }

    /// The rating and the tags (with paths) of the panel's choice.
    pub fn plugin_choice(&self) -> (Option<u8>, Vec<Tag>) {
        let state = self.state.borrow();
        let choice = state.choice();
        let tags = choice
            .tags
            .iter()
            .filter_map(|id| {
                Some(Tag {
                    id: *id,
                    name: state.name(*id)?.to_owned(),
                    path: state.path(*id),
                })
            })
            .collect();
        (choice.rating, tags)
    }

    /// A plugin's rating; `false` when nothing can be rated now (no video, "Active" off, no job edited).
    pub fn plugin_rating(&self, stars: u8, mark: PluginMark) -> bool {
        let has_video = self
            .window
            .upgrade()
            .is_some_and(|w| w.global::<Playback<'_>>().get_has_video());
        let mut state = self.state.borrow_mut();
        if !state.taggable(has_video) {
            return false;
        }
        state.set_rating(stars);
        drop(state);
        self.rating_mark.replace(Some(mark));
        self.publish();
        true
    }

    /// Every tag with its path (the plugin settings' tag lists), and whether a tree arrived at all.
    pub fn tag_paths(&self) -> Option<Vec<(u32, Vec<String>)>> {
        let state = self.state.borrow();
        state.has_tree().then(|| state.tag_paths())
    }

    /// The plugin that set the rating, if one did.
    pub fn rating_plugin(&self) -> Option<String> {
        self.rating_mark.borrow().as_ref().map(|mark| mark.id.clone())
    }

    /// Another video or a saved job: a plugin's rating mark goes with the rating it was about.
    pub fn forget_rating_mark(&self) {
        if self.rating_mark.replace(None).is_some() {
            self.publish();
        }
    }
}
