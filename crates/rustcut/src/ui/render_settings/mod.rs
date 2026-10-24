//! The Render Settings panel (M2.5, ADR-14): profile quick selection with the working copy (user decision Q-E4), the
//! simple view (codec, codec-neutral quality slider and speeds, container, audio; Q-E2/Q-E3), the expert view per codec
//! and the issues of the settings. The pure parts are `state` (every edit) and `view` (the rows); this bridge keeps the
//! state on the UI thread, learns which encoders work from the capability check, has the real encoder check the working
//! copy in the background (debounced, the stale check aborted), and saves the user's profiles into `config.toml`
//! through the settings writer.

mod choices;
mod input;
pub mod rows;
pub mod state;
pub mod view;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use rustcut_core::render::{
    Capabilities, HardwareContext, OptionId, ProfileLibrary, RenderProfile, Selection, check_profile,
};
use rustcut_core::settings::{ExportPanelState, Settings};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
use tokio::sync::watch;

use self::state::{Check, CheckStatus, Encoders, PanelState};
use self::view::RowView;
use crate::app::SettingsWriter;
use crate::ui::status::{self, Level};
use crate::util::model::sync;
use crate::{MainWindow, OptionRowView, RenderIssueView, RenderSettings};

/// Quiet time after the last change before the real encoder checks the settings.
const CHECK_DELAY: Duration = Duration::from_millis(500);

pub struct RenderSettingsBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    state: RefCell<PanelState>,
    /// The settings as last written (the user's profiles change in it).
    settings: RefCell<Settings>,
    writer: SettingsWriter,
    ffmpeg: RefCell<PathBuf>,
    /// Where the check writes its few frames.
    check_dir: PathBuf,
    check_timer: slint::Timer,
    check_task: RefCell<Option<tokio::task::AbortHandle>>,
    profiles: Rc<VecModel<SharedString>>,
    issues: Rc<VecModel<RenderIssueView>>,
    video: Rc<VecModel<OptionRowView>>,
    audio: Rc<VecModel<OptionRowView>>,
    expert: Rc<VecModel<OptionRowView>>,
}

impl RenderSettingsBridge {
    /// Takes over the Render Settings panel. `encoders` is the capability check started at app start. The app owns the
    /// only strong reference.
    pub fn install(
        window: &MainWindow,
        runtime: tokio::runtime::Handle,
        settings: &Settings,
        export: &ExportPanelState,
        writer: SettingsWriter,
        cache_dir: &std::path::Path,
        encoders: watch::Receiver<Option<Arc<Capabilities>>>,
    ) -> Rc<Self> {
        let library = ProfileLibrary::new(settings.render.profiles.clone());
        let selection = Selection::restore(
            &library,
            export.profile.as_deref(),
            export.render.clone(),
            &settings.render.default_profile,
        );
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime,
            state: RefCell::new(PanelState::new(library, selection)),
            settings: RefCell::new(settings.clone()),
            writer,
            ffmpeg: RefCell::new(
                settings
                    .advanced
                    .ffmpeg
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("ffmpeg")),
            ),
            check_dir: cache_dir.join("check"),
            check_timer: slint::Timer::default(),
            check_task: RefCell::new(None),
            profiles: Rc::new(VecModel::default()),
            issues: Rc::new(VecModel::default()),
            video: Rc::new(VecModel::default()),
            audio: Rc::new(VecModel::default()),
            expert: Rc::new(VecModel::default()),
        });
        let rs = window.global::<RenderSettings<'_>>();
        rs.set_profiles(ModelRc::from(Rc::clone(&bridge.profiles)));
        rs.set_issues(ModelRc::from(Rc::clone(&bridge.issues)));
        rs.set_video_rows(ModelRc::from(Rc::clone(&bridge.video)));
        rs.set_audio_rows(ModelRc::from(Rc::clone(&bridge.audio)));
        rs.set_expert_rows(ModelRc::from(Rc::clone(&bridge.expert)));
        rs.set_profile_tooltip(OptionId::Profile.help().tooltip.into());
        input::connect(&bridge, window);
        bridge.watch_encoders(encoders);
        bridge.publish();
        bridge
    }

    /// The settings the next job and the probe render get.
    pub fn working_profile(&self) -> RenderProfile {
        self.state.borrow().profile().clone()
    }

    /// The profile and its working copy as they are (a draft set aside while a job is edited).
    pub fn selection(&self) -> Selection {
        self.state.borrow().selection.clone()
    }

    /// Puts a draft's selection back.
    pub fn put_selection(&self, selection: Selection) {
        self.state.borrow_mut().selection = selection;
        self.changed(false);
    }

    /// The settings of a job loaded into the editor become the working copy.
    pub fn load_profile(&self, profile: &RenderProfile) {
        {
            let mut st = self.state.borrow_mut();
            let st = &mut *st;
            st.selection.load(&st.library, profile);
        }
        self.changed(false);
    }

    /// Every profile (id, name) for "Render again with".
    pub fn profiles(&self) -> Vec<(String, String)> {
        self.state
            .borrow()
            .library
            .all()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect()
    }

    /// The profile of `id`; `""` is the working copy (the panel's current settings).
    pub fn profile(&self, id: &str) -> Option<RenderProfile> {
        let st = self.state.borrow();
        if id.is_empty() {
            return Some(st.profile().clone());
        }
        st.library.get(id).cloned()
    }

    /// The user's profiles changed elsewhere (the Settings window): the list follows; the working copy keeps its
    /// settings (marked modified when its profile is gone or differs now).
    pub fn profiles_changed(&self, user: &[RenderProfile]) {
        {
            let mut st = self.state.borrow_mut();
            if st.library.user() == user {
                return;
            }
            st.library = ProfileLibrary::new(user.to_vec());
            let state = &mut *st;
            state.selection.follow(&state.library);
        }
        self.settings.borrow_mut().render.profiles = user.to_vec();
        self.changed(false);
    }

    /// Another ffmpeg (the Settings window): the encoders are checked again with it (`encoders` is the new check) and
    /// the settings check uses it.
    pub fn set_ffmpeg(&self, ffmpeg: PathBuf, encoders: watch::Receiver<Option<Arc<Capabilities>>>) {
        *self.ffmpeg.borrow_mut() = ffmpeg;
        self.state.borrow_mut().encoders = Encoders::Checking;
        self.state.borrow_mut().check = None;
        self.watch_encoders(encoders);
        self.changed(false);
    }

    /// What the capability check knows about the encoders (the Settings window's profile editor).
    pub fn encoders(&self) -> Encoders {
        self.state.borrow().encoders.clone()
    }

    /// The capability check's result once known (encoder availability, the VAAPI device, vid.stab).
    pub fn capabilities(&self) -> Option<Arc<Capabilities>> {
        match &self.state.borrow().encoders {
            Encoders::Known(caps) => Some(Arc::clone(caps)),
            Encoders::Checking | Encoders::Unchecked => None,
        }
    }

    /// The working copy into the UI state (at quit): the profile it came from, and the copy only while it differs.
    pub fn save_into(&self, export: &mut ExportPanelState) {
        let st = self.state.borrow();
        export.profile = Some(st.selection.base.clone());
        export.render = st.is_modified().then(|| st.profile().clone());
    }

    /// Follows the capability check: `Known` once it reports, `Unchecked` when it ended without a result.
    fn watch_encoders(&self, mut encoders: watch::Receiver<Option<Arc<Capabilities>>>) {
        let this = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            loop {
                let known = encoders.borrow_and_update().clone();
                if let Some(caps) = known {
                    if let Some(bridge) = this.upgrade() {
                        bridge.encoders_known(Encoders::Known(caps));
                    }
                    return;
                }
                if encoders.changed().await.is_err() {
                    let late = encoders.borrow().clone();
                    if let Some(bridge) = this.upgrade() {
                        bridge.encoders_known(late.map_or(Encoders::Unchecked, Encoders::Known));
                    }
                    return;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Render] The event loop is gone; the panel does not learn the encoders");
        }
    }

    fn encoders_known(&self, encoders: Encoders) {
        self.state.borrow_mut().encoders = encoders;
        self.changed(false);
    }

    /// After every change: show it, check the new settings soon, and save the profiles when they changed.
    fn changed(&self, library_changed: bool) {
        if library_changed {
            let profiles = self.state.borrow().library.user().to_vec();
            self.settings.borrow_mut().render.profiles.clone_from(&profiles);
            if !self.writer.update(|settings| settings.render.profiles = profiles) {
                self.status(
                    Level::Warning,
                    "config.toml is from a newer RustCut: the profiles are not saved.",
                );
            }
        }
        self.schedule_check();
        self.publish();
    }

    /// Starts the real encoder's check after a quiet moment, when the working copy needs one; a running check of
    /// older settings stops.
    fn schedule_check(&self) {
        let stale = {
            let st = self.state.borrow();
            st.checking && st.check.as_ref().is_none_or(|c| !c.profile.same_settings(st.profile()))
        };
        if stale && let Some(task) = self.check_task.borrow_mut().take() {
            task.abort();
            self.state.borrow_mut().checking = false;
        }
        if self.state.borrow().check_status() != CheckStatus::Pending {
            self.check_timer.stop();
            return;
        }
        let this = Weak::clone(&self.me);
        self.check_timer
            .start(slint::TimerMode::SingleShot, CHECK_DELAY, move || {
                if let Some(bridge) = this.upgrade() {
                    bridge.start_check();
                }
            });
    }

    fn start_check(&self) {
        let (profile, hardware) = {
            let mut st = self.state.borrow_mut();
            if st.check_status() != CheckStatus::Pending {
                return;
            }
            st.checking = true;
            st.check_failed = None;
            let hardware = match &st.encoders {
                Encoders::Known(caps) => caps.hardware.clone(),
                Encoders::Checking | Encoders::Unchecked => HardwareContext::default(),
            };
            (st.profile().clone(), hardware)
        };
        let (ffmpeg, dir) = (self.ffmpeg.borrow().clone(), self.check_dir.clone());
        let checked = profile.clone();
        let task = self
            .runtime
            .spawn(async move { check_profile(&ffmpeg, &checked, &hardware, &dir).await });
        *self.check_task.borrow_mut() = Some(task.abort_handle());
        let this = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let result = task.await;
            let Some(bridge) = this.upgrade() else { return };
            {
                let mut st = bridge.state.borrow_mut();
                st.checking = false;
                match result {
                    Ok(Ok(issues)) => {
                        tracing::debug!(issues = issues.len(), "[Render] Settings checked by ffmpeg");
                        st.check = Some(Check { profile, issues });
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "[Render] The settings could not be checked");
                        st.check_failed = Some(e.to_string());
                    }
                    // Aborted for newer settings: their own check follows.
                    Err(_) => return,
                }
            }
            bridge.check_task.borrow_mut().take();
            bridge.publish();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Render] The event loop is gone; no check");
        }
        self.publish();
    }

    fn status(&self, level: Level, message: &str) {
        tracing::info!("[Render] {message}");
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }

    /// Shows the whole panel.
    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let (panel, name) = {
            let st = self.state.borrow();
            let name = st
                .library
                .get(&st.selection.base)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            (view::panel(&st), name)
        };
        let rs = window.global::<RenderSettings<'_>>();
        sync(
            &self.profiles,
            panel.profiles.iter().map(|p| SharedString::from(p.as_str())).collect(),
        );
        rs.set_profile_index(index(panel.profile_index));
        rs.set_profile_name(name.as_str().into());
        rs.set_modified(panel.modified);
        rs.set_user_profile(panel.user_profile);
        sync(
            &self.issues,
            panel
                .issues
                .iter()
                .map(|i| RenderIssueView {
                    error: i.error,
                    text: i.text.as_str().into(),
                })
                .collect(),
        );
        rs.set_check_text(panel.check_text.as_str().into());
        rs.set_check_running(panel.check_running);
        rs.set_encoders_note(panel.encoders_note.as_str().into());
        sync(&self.video, panel.video.iter().map(row).collect());
        sync(&self.audio, panel.audio.iter().map(row).collect());
        sync(&self.expert, panel.expert.iter().map(row).collect());
    }
}

/// A list index for Slint.
fn index(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(0)
}

fn row(r: &RowView) -> OptionRowView {
    OptionRowView {
        key: r.row.key().into(),
        kind: r.kind as i32,
        label: r.label.as_str().into(),
        tooltip: r.tooltip.as_str().into(),
        items: ModelRc::new(VecModel::from(
            r.items
                .iter()
                .map(|i| SharedString::from(i.as_str()))
                .collect::<Vec<_>>(),
        )),
        index: index(r.index),
        text: r.text.as_str().into(),
        placeholder: r.placeholder.as_str().into(),
        checked: r.checked,
        steps: i32::from(r.steps),
        position: i32::from(r.position),
        enabled: r.enabled,
        mark: r.mark as i32,
    }
}
