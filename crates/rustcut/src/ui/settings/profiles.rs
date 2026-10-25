//! The Render Profiles page (pure): the list of every profile, which one new jobs start with, and the editor of the
//! selected one. It reuses the Codecs panel's `PanelState` (rows, choices, validation) with its own library copy: a
//! built-in profile is shown read-only, an own profile is saved with every change (D-S2), so the page never holds
//! unsaved settings.

use rustcut_core::render::{LibraryError, ProfileLibrary, RenderProfile, Selection};

use crate::ui::render_settings::state::{Encoders, PanelState};
use crate::ui::render_settings::view::{self, PanelView};

/// One entry of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileItem {
    pub name: String,
    pub summary: String,
    pub builtin: bool,
    pub default: bool,
}

#[derive(Debug, Clone)]
pub struct ProfilesPage {
    pub state: PanelState,
}

impl ProfilesPage {
    /// The page with the user's `profiles`, showing `selected` (else the default profile).
    pub fn new(profiles: Vec<RenderProfile>, selected: Option<&str>, default: &str) -> Self {
        let library = ProfileLibrary::new(profiles);
        let selection = Selection::restore(&library, selected, None, default);
        Self {
            state: PanelState::new(library, selection),
        }
    }

    pub fn selected_id(&self) -> &str {
        &self.state.selection.base
    }

    pub fn is_builtin(&self) -> bool {
        !self.state.library.is_user(self.selected_id())
    }

    pub fn set_encoders(&mut self, encoders: Encoders) {
        self.state.encoders = encoders;
    }

    /// The user's profiles changed elsewhere (the Codecs panel saved one): the list follows, the selection stays when
    /// the profile is still there.
    pub fn follow(&mut self, user: &[RenderProfile], default: &str) -> bool {
        if self.state.library.user() == user {
            return false;
        }
        let selected = self.selected_id().to_owned();
        let library = ProfileLibrary::new(user.to_vec());
        self.state.selection = Selection::restore(&library, Some(&selected), None, default);
        self.state.library = library;
        true
    }

    /// Shows the profile at `index` of the list.
    pub fn select(&mut self, index: usize) -> bool {
        let state = &mut self.state;
        let Some(id) = state.library.all().nth(index).map(|p| p.id.clone()) else {
            return false;
        };
        id != state.selection.base && state.selection.select(&state.library, &id)
    }

    /// An own copy of the selected profile ("<name> copy"), selected afterwards; returns the new id.
    pub fn duplicate(&mut self) -> Result<String, LibraryError> {
        let state = &mut self.state;
        let name = format!("{} copy", state.selection.working.name);
        let id = state.library.save_as(&name, &state.selection.working)?;
        state.selection.select(&state.library, &id);
        Ok(id)
    }

    pub fn rename(&mut self, name: &str) -> Result<bool, LibraryError> {
        let state = &mut self.state;
        if name.trim() == state.selection.working.name {
            return Ok(false);
        }
        let id = state.selection.base.clone();
        state.library.rename(&id, name)?;
        state.selection.follow(&state.library);
        Ok(true)
    }

    /// Deletes the selected own profile; the default profile is shown afterwards.
    pub fn delete(&mut self, default: &str) -> Result<RenderProfile, LibraryError> {
        let state = &mut self.state;
        let id = state.selection.base.clone();
        let deleted = state.library.delete(&id)?;
        state.selection = Selection::restore(&state.library, Some(default), None, default);
        Ok(deleted)
    }

    /// After an edit of the shown settings: an own profile keeps them at once (`true` = the profiles changed and must
    /// be saved); a built-in one goes back to its settings (its rows are read-only, so this only catches a stray edit).
    pub fn edited(&mut self) -> bool {
        let state = &mut self.state;
        let id = state.selection.base.clone();
        if state.library.overwrite(&id, &state.selection.working).is_ok() {
            state.selection.follow(&state.library);
            true
        } else {
            state.selection.reset(&state.library);
            false
        }
    }

    pub fn items(&self, default: &str) -> Vec<ProfileItem> {
        self.state
            .library
            .all()
            .map(|p| ProfileItem {
                name: p.name.clone(),
                summary: summary(p),
                builtin: !self.state.library.is_user(&p.id),
                default: p.id == default,
            })
            .collect()
    }

    /// The list position of profile `id`.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.state.library.all().position(|p| p.id == id)
    }

    pub fn index(&self) -> usize {
        self.state
            .library
            .all()
            .position(|p| p.id == self.state.selection.base)
            .unwrap_or(0)
    }

    /// The editor's rows and issues (read-only for a built-in profile).
    pub fn view(&self) -> PanelView {
        let mut panel = view::panel(&self.state);
        if self.is_builtin() {
            for row in panel.video.iter_mut().chain(&mut panel.audio).chain(&mut panel.expert) {
                row.enabled = false;
            }
        }
        panel
    }
}

/// "AV1 (SVT-AV1) · MKV · Opus 128 kbit/s".
pub fn summary(p: &RenderProfile) -> String {
    let audio = p.audio.codec.spec();
    let audio = match p.audio.bitrate_kbps.or(audio.default_bitrate) {
        Some(kbps) if p.audio.codec.takes_bitrate() => format!("{} {kbps} kbit/s", audio.display_name),
        _ => audio.display_name.to_owned(),
    };
    format!(
        "{} · {} · {audio}",
        p.video.encoder.spec().display_name,
        p.container.display_name()
    )
}

#[cfg(test)]
mod tests {
    use rustcut_core::render::{DEFAULT_PROFILE_ID, VideoEncoder};

    use super::*;

    #[test]
    fn built_ins_are_read_only_and_own_profiles_save_every_change() {
        let mut page = ProfilesPage::new(Vec::new(), Some("gone"), DEFAULT_PROFILE_ID);
        assert_eq!(page.selected_id(), DEFAULT_PROFILE_ID);
        assert!(page.is_builtin());
        assert!(page.view().video.iter().all(|r| !r.enabled));
        let builtins = page.items(DEFAULT_PROFILE_ID).len();
        assert!(page.items(DEFAULT_PROFILE_ID).iter().any(|i| i.default && i.builtin));

        let id = page.duplicate().unwrap();
        assert_eq!(page.selected_id(), id);
        assert!(!page.is_builtin() && page.view().video.iter().all(|r| r.enabled));
        assert_eq!(page.items(DEFAULT_PROFILE_ID).len(), builtins + 1);
        assert_eq!(page.index(), builtins);

        page.state.selection.working.set_encoder(VideoEncoder::X264);
        assert!(page.edited());
        let saved = page.state.library.get(&id).unwrap();
        assert_eq!(saved.video.encoder, VideoEncoder::X264);
        assert!(!page.state.is_modified());
        assert!(page.items(&id).last().unwrap().summary.starts_with("H.264"));

        assert_eq!(page.rename("  Shared clips "), Ok(true));
        assert_eq!(page.state.library.get(&id).unwrap().name, "Shared clips");
        assert!(page.rename(" ").is_err());

        // The Codecs panel saved another profile: the list follows and keeps the selection.
        let mut user = page.state.library.user().to_vec();
        let mut other = user.first().unwrap().clone();
        other.id = "other".into();
        user.push(other);
        assert!(page.follow(&user, DEFAULT_PROFILE_ID));
        assert_eq!(page.selected_id(), id);
        assert!(!page.follow(&user, DEFAULT_PROFILE_ID));

        assert!(page.select(0));
        assert!(page.delete(DEFAULT_PROFILE_ID).is_err(), "built-in");
        assert!(page.select(builtins));
        assert_eq!(page.delete(DEFAULT_PROFILE_ID).unwrap().id, id);
        assert_eq!(page.selected_id(), DEFAULT_PROFILE_ID);
    }

    #[test]
    fn a_stray_edit_of_a_built_in_is_dropped() {
        let mut page = ProfilesPage::new(Vec::new(), None, DEFAULT_PROFILE_ID);
        page.state.selection.working.set_encoder(VideoEncoder::X265);
        assert!(page.state.is_modified());
        assert!(!page.edited());
        assert!(!page.state.is_modified());
    }
}
