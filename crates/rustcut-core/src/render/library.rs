//! The render profiles a user can pick (built-ins plus their own, `Settings.render.profiles`) and the working copy the
//! Render Settings panel edits (user decision Q-E4): changes go to the copy, which is marked modified until it is saved
//! as a new profile or into the user's own profile; built-ins never change. The copy is what the next job gets and
//! survives restarts (`UiState.export.render`).

use super::{RenderProfile, builtin_profiles, is_builtin_id};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryError {
    #[error("built-in profiles cannot be changed; save a copy instead")]
    BuiltIn,
    #[error("there is no profile `{0}`")]
    Unknown(String),
    #[error("the profile needs a name")]
    NoName,
}

/// Built-in and user profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLibrary {
    builtin: Vec<RenderProfile>,
    user: Vec<RenderProfile>,
}

impl ProfileLibrary {
    /// The built-ins plus `user` (as stored; reserved and duplicate ids are dropped by the settings' sanitizing).
    pub fn new(user: Vec<RenderProfile>) -> Self {
        Self {
            builtin: builtin_profiles(),
            user,
        }
    }

    /// Every profile: the built-ins first, then the user's in their order.
    pub fn all(&self) -> impl Iterator<Item = &RenderProfile> {
        self.builtin.iter().chain(&self.user)
    }

    pub fn user(&self) -> &[RenderProfile] {
        &self.user
    }

    pub fn get(&self, id: &str) -> Option<&RenderProfile> {
        self.all().find(|p| p.id == id)
    }

    pub fn is_user(&self, id: &str) -> bool {
        self.user.iter().any(|p| p.id == id)
    }

    /// Adds `settings` as a new user profile named `name`; returns its id (made from the name, unique).
    pub fn save_as(&mut self, name: &str, settings: &RenderProfile) -> Result<String, LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::NoName);
        }
        let id = self.unique_id(name);
        self.user.push(RenderProfile {
            id: id.clone(),
            name: name.to_owned(),
            ..settings.clone()
        });
        Ok(id)
    }

    /// Replaces the settings of the user profile `id` (its name stays).
    pub fn overwrite(&mut self, id: &str, settings: &RenderProfile) -> Result<(), LibraryError> {
        let profile = self.user_mut(id)?;
        profile.container = settings.container;
        profile.video = settings.video.clone();
        profile.audio = settings.audio;
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::NoName);
        }
        name.clone_into(&mut self.user_mut(id)?.name);
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<RenderProfile, LibraryError> {
        let index = self.user_index(id)?;
        Ok(self.user.remove(index))
    }

    fn user_index(&self, id: &str) -> Result<usize, LibraryError> {
        if is_builtin_id(id) {
            return Err(LibraryError::BuiltIn);
        }
        self.user
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| LibraryError::Unknown(id.to_owned()))
    }

    fn user_mut(&mut self, id: &str) -> Result<&mut RenderProfile, LibraryError> {
        let index = self.user_index(id)?;
        self.user
            .get_mut(index)
            .ok_or_else(|| LibraryError::Unknown(id.to_owned()))
    }

    /// An id from `name` (`My AV1!` -> `my-av1`) that no profile uses, with `-2`, `-3`, ... when taken.
    fn unique_id(&self, name: &str) -> String {
        let mut base: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        base = base.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
        if base.is_empty() {
            "profile".clone_into(&mut base);
        }
        let taken = |id: &str| is_builtin_id(id) || self.all().any(|p| p.id == id);
        if !taken(&base) {
            return base;
        }
        (2..=u32::from(u16::MAX))
            .map(|n| format!("{base}-{n}"))
            .find(|id| !taken(id))
            .unwrap_or(base)
    }
}

/// The panel's selection: the profile it started from and the working copy of its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Id of the profile the copy came from.
    pub base: String,
    /// The settings the next job gets; `id` and `name` are the base's.
    pub working: RenderProfile,
}

impl Selection {
    /// Restores the selection saved in the UI state: `base` (else `default_id`, else the first profile) with the
    /// `working` copy when there is one.
    pub fn restore(
        library: &ProfileLibrary,
        base: Option<&str>,
        working: Option<RenderProfile>,
        default_id: &str,
    ) -> Self {
        let profile = base
            .and_then(|id| library.get(id))
            .or_else(|| library.get(default_id))
            .or_else(|| library.all().next())
            .cloned()
            .unwrap_or_else(|| builtin_profiles().remove(0));
        let working = working.map_or_else(
            || profile.clone(),
            |w| RenderProfile {
                id: profile.id.clone(),
                name: profile.name.clone(),
                ..w
            },
        );
        Self {
            base: profile.id,
            working,
        }
    }

    /// The working copy differs from its profile (or the profile is gone).
    pub fn is_modified(&self, library: &ProfileLibrary) -> bool {
        library.get(&self.base).is_none_or(|p| !p.same_settings(&self.working))
    }

    /// Starts over from profile `id` (unknown ids change nothing).
    pub fn select(&mut self, library: &ProfileLibrary, id: &str) -> bool {
        let Some(profile) = library.get(id) else { return false };
        self.base.clone_from(&profile.id);
        self.working = profile.clone();
        true
    }

    /// Drops the changes: the working copy becomes its profile again.
    pub fn reset(&mut self, library: &ProfileLibrary) {
        let base = self.base.clone();
        self.select(library, &base);
    }

    /// The settings of a job loaded into the editor: its profile becomes the base when the library still has it (with
    /// the name it has now), else the base stays and the copy is marked modified.
    pub fn load(&mut self, library: &ProfileLibrary, settings: &RenderProfile) {
        if let Some(profile) = library.get(&settings.id) {
            self.base.clone_from(&profile.id);
        }
        let (id, name) = library.get(&self.base).map_or_else(
            || (settings.id.clone(), settings.name.clone()),
            |p| (p.id.clone(), p.name.clone()),
        );
        self.working = RenderProfile {
            id,
            name,
            ..settings.clone()
        };
    }

    /// The working copy after its profile was renamed or saved: the name follows.
    pub fn follow(&mut self, library: &ProfileLibrary) {
        if let Some(profile) = library.get(&self.base) {
            self.working.name = profile.name.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{DEFAULT_PROFILE_ID, RateControl, VideoEncoder};

    #[test]
    fn save_as_overwrite_rename_delete() {
        let mut lib = ProfileLibrary::new(Vec::new());
        let mut sel = Selection::restore(&lib, None, None, DEFAULT_PROFILE_ID);
        assert_eq!(sel.base, DEFAULT_PROFILE_ID);
        assert!(!sel.is_modified(&lib));

        sel.working.set_encoder(VideoEncoder::X265);
        assert!(sel.is_modified(&lib));
        assert_eq!(
            lib.overwrite(DEFAULT_PROFILE_ID, &sel.working),
            Err(LibraryError::BuiltIn)
        );
        let id = lib.save_as("  My H.265!  ", &sel.working).unwrap();
        assert_eq!(id, "my-h-265");
        assert!(sel.select(&lib, &id));
        assert_eq!(sel.working.name, "My H.265!");
        assert!(!sel.is_modified(&lib));

        sel.working.video.rate_control = RateControl::Quality { value: 18 };
        lib.overwrite(&id, &sel.working).unwrap();
        assert!(!sel.is_modified(&lib));
        lib.rename(&id, "Mine").unwrap();
        sel.follow(&lib);
        assert_eq!(sel.working.name, "Mine");
        assert_eq!(lib.save_as("My H.265!", &sel.working).unwrap(), "my-h-265-2");
        assert_eq!(lib.save_as(" ", &sel.working), Err(LibraryError::NoName));
        assert_eq!(
            lib.save_as("AV1 balanced", &sel.working).unwrap(),
            "av1-balanced-2",
            "built-in ids are reserved"
        );

        assert_eq!(lib.delete(DEFAULT_PROFILE_ID), Err(LibraryError::BuiltIn));
        lib.delete(&id).unwrap();
        assert!(sel.is_modified(&lib), "a deleted profile leaves a modified copy");
        assert_eq!(lib.delete("gone"), Err(LibraryError::Unknown("gone".into())));
    }

    #[test]
    fn loading_a_jobs_settings() {
        let mut lib = ProfileLibrary::new(Vec::new());
        let mut sel = Selection::restore(&lib, None, None, DEFAULT_PROFILE_ID);
        let mut job = lib.get("x265-quality").unwrap().clone();
        job.name = "x265 quality (old name)".into();
        sel.load(&lib, &job);
        assert_eq!(
            (sel.base.as_str(), sel.working.name.as_str()),
            ("x265-quality", "x265 quality")
        );
        assert!(!sel.is_modified(&lib), "the job's profile as it is in the library");
        job.video.keyint = Some(123);
        sel.load(&lib, &job);
        assert!(sel.is_modified(&lib));
        assert_eq!(sel.working.video.keyint, Some(123));

        // A user profile deleted since: the base stays, the job's settings are the modified copy.
        let id = lib.save_as("Mine", &sel.working).unwrap();
        let mine = lib.get(&id).unwrap().clone();
        lib.delete(&id).unwrap();
        sel.load(&lib, &mine);
        assert_eq!(sel.base, "x265-quality");
        assert!(sel.working.same_settings(&mine));
    }

    #[test]
    fn restoring_keeps_the_working_copy() {
        let lib = ProfileLibrary::new(Vec::new());
        let mut copy = lib.get("x264-fast-share").unwrap().clone();
        copy.video.preset = Some("slow".into());
        copy.name = "stale name".into();
        let sel = Selection::restore(&lib, Some("x264-fast-share"), Some(copy), DEFAULT_PROFILE_ID);
        assert_eq!(sel.working.name, "x264 fast share", "the name comes from the profile");
        assert!(sel.is_modified(&lib));
        let mut sel = Selection::restore(&lib, Some("deleted"), None, "also-gone");
        assert_eq!(sel.base, DEFAULT_PROFILE_ID);
        sel.working.video.preset = Some("4".into());
        sel.reset(&lib);
        assert!(!sel.is_modified(&lib));
    }
}
