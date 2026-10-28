//! Settings that apply while RustCut runs (M4.2): after every change through the settings writer, the parts that use a
//! changed value get it. Values used only at start (paths of the programs, hardware decoding, single instance, ...)
//! are marked "Applies at the next start" in the Settings window.

use std::rc::Weak;

use rustcut_core::queue::{QueueCommand, QueueHandle};
use rustcut_core::render::{RenderEnv, spawn_capability_check};
use rustcut_core::rustview::{SyncCommand, SyncHandle};
use rustcut_core::settings::{AppPaths, RustViewSettings, Settings};
use slint::ComponentHandle as _;

use super::SettingsWriter;
use super::logging::LevelSetter;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::editing::JobEditor;
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::filters::FiltersBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::notices::NoticeCenter;
use crate::ui::player::PlayerBridge;
use crate::ui::power::PowerBridge;
use crate::ui::probe::ProbeBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::tags::TagsBridge;
use crate::ui::tray::TrayBridge;
use crate::{HoldConfirm, MainWindow, Queue, TooltipSettings};

/// The parts that follow the settings.
pub struct Parts {
    pub main: slint::Weak<MainWindow>,
    pub editor: Weak<Editor>,
    pub player: Weak<PlayerBridge>,
    pub export: Weak<ExportBridge>,
    pub filters: Weak<FiltersBridge>,
    pub render: Weak<RenderSettingsBridge>,
    pub tags: Weak<TagsBridge>,
    pub editing: Weak<JobEditor>,
    pub queue: QueueHandle,
    pub sync: SyncHandle,
    /// The folder pictures' store (P11.2): told which pictures the tree still uses.
    pub pictures: rustcut_core::folders::pictures::PictureHandle,
    pub paths: AppPaths,
    /// The renders' environment as started (its hardware until the capability check knows better).
    pub env: RenderEnv,
    pub log: LevelSetter,
    pub media: Weak<MediaInfoBridge>,
    pub probe: Weak<ProbeBridge>,
    pub runtime: tokio::runtime::Handle,
    /// Starts (`true`) or stops listening for a second start.
    pub instance: Box<dyn Fn(bool)>,
    pub power: Weak<PowerBridge>,
    pub tray: Weak<TrayBridge>,
    pub notices: Weak<NoticeCenter>,
    pub ui: UiDispatcher,
}

/// The hold time, the tooltips and the queue list's following of the main window.
pub fn look(main: &MainWindow, settings: &Settings) {
    main.global::<HoldConfirm<'_>>()
        .set_duration(i64::from(settings.general.hold_to_confirm_ms));
    let tooltips = main.global::<TooltipSettings<'_>>();
    tooltips.set_enabled(settings.appearance.tooltips);
    tooltips.set_delay(i64::from(settings.appearance.tooltip_delay_ms));
    main.global::<Queue<'_>>()
        .set_follow_new(settings.queue.follow_new_jobs);
}

pub fn connect(writer: &SettingsWriter, parts: Parts) {
    writer.subscribe(move |before, after| apply(&parts, before, after));
}

fn apply(p: &Parts, before: &Settings, after: &Settings) {
    if (before.general.hold_to_confirm_ms != after.general.hold_to_confirm_ms
        || before.appearance != after.appearance
        || before.queue.follow_new_jobs != after.queue.follow_new_jobs)
        && let Some(main) = p.main.upgrade()
    {
        look(&main, after);
    }
    let steps = |s: &Settings| (s.editor.seek_step_secs, s.editor.seek_step_long_secs);
    if (before.keymap != after.keymap || steps(before) != steps(after))
        && let Some(editor) = p.editor.upgrade()
    {
        editor.apply_settings(after);
    }
    if before.player != after.player
        && let Some(player) = p.player.upgrade()
    {
        player.apply_settings(&before.player, &after.player);
    }
    if (before.export != after.export
        || before.warnings != after.warnings
        || before.editor.auto_section != after.editor.auto_section
        || before.dialogs != after.dialogs
        || before.advanced.ffmpeg != after.advanced.ffmpeg
        || before.advanced.ffprobe != after.advanced.ffprobe)
        && let Some(export) = p.export.upgrade()
    {
        export.settings_changed(after);
    }
    if before.filters != after.filters
        && let Some(filters) = p.filters.upgrade()
    {
        filters.set_defaults(after.filters.clone());
    }
    if before.editor.on_job_load != after.editor.on_job_load
        && let Some(editing) = p.editing.upgrade()
    {
        editing.set_on_job_load(after.editor.on_job_load);
    }
    if before.render.profiles != after.render.profiles
        && let Some(render) = p.render.upgrade()
    {
        render.profiles_changed(&after.render.profiles);
    }
    if before.render.hidden_profiles != after.render.hidden_profiles
        && let Some(render) = p.render.upgrade()
    {
        render.hidden_changed(&after.render.hidden_profiles);
    }
    let programs = |s: &Settings| (s.advanced.ffmpeg.clone(), s.advanced.ffprobe.clone());
    let changed_programs = programs(before) != programs(after);
    if changed_programs {
        programs_changed(p, after);
    }
    let process = |s: &Settings| (s.render.nice, s.render.idle_io, s.render.stall_timeout_secs);
    if changed_programs || process(before) != process(after) {
        let hardware = p
            .render
            .upgrade()
            .and_then(|r| r.capabilities())
            .map_or_else(|| p.env.hardware.clone(), |c| c.hardware.clone());
        let env = RenderEnv::new(after, &p.paths, hardware);
        if let Some(probe) = p.probe.upgrade() {
            probe.set_env(env.clone());
        }
        send_queue(&p.queue, QueueCommand::SetRenderEnv(Box::new(env)));
    }
    let known_source = crate::ui::media_info::known_source::wanted(after);
    if crate::ui::media_info::known_source::wanted(before) != known_source
        && let Some(media) = p.media.upgrade()
    {
        media.set_known_source(known_source);
    }
    if before.notices != after.notices
        && let Some(center) = p.notices.upgrade()
    {
        center.apply_settings(&after.notices);
    }
    if before.hashing != after.hashing
        && let Some(media) = p.media.upgrade()
    {
        media.set_write_sidecar(after.hashing.write_sidecar);
    }
    if before.general.single_instance != after.general.single_instance {
        (p.instance)(after.general.single_instance);
    }
    if before.queue.workers != after.queue.workers {
        send_queue(&p.queue, QueueCommand::SetWorkers(after.queue.workers));
    }
    if before.rustview.tag_history_size != after.rustview.tag_history_size
        && let Some(tags) = p.tags.upgrade()
    {
        tags.set_history_size(after.rustview.tag_history_size);
    }
    if before.rustview.compact_tree != after.rustview.compact_tree
        && let Some(tags) = p.tags.upgrade()
    {
        tags.set_compact(after.rustview.compact_tree);
    }
    if before.warnings.missing_metadata != after.warnings.missing_metadata
        && let Some(tags) = p.tags.upgrade()
    {
        tags.set_warn(after.warnings.missing_metadata);
    }
    if link(&before.rustview) != link(&after.rustview)
        && p.sync
            .send(SyncCommand::Reconfigure {
                settings: after.rustview.clone(),
            })
            .is_err()
    {
        tracing::warn!("[Settings] The RustView link has stopped; the change applies at the next start");
    }
    let power = |s: &Settings| {
        (
            s.desktop.clone(),
            s.queue.when_done,
            s.queue.when_done_countdown_secs,
            s.general.confirm_quit_while_rendering,
        )
    };
    if power(before) != power(after)
        && let Some(p) = p.power.upgrade()
    {
        p.apply_settings(after);
    }
    if before.desktop.tray != after.desktop.tray
        && let Some(tray) = p.tray.upgrade()
    {
        tray.apply_settings(&after.desktop.tray);
    }
    if before.advanced.log_level != after.advanced.log_level {
        p.log.apply(&after.advanced.log_level);
    }
    if before.advanced.log_keep_days != after.advanced.log_keep_days
        || before.advanced.log_max_mb != after.advanced.log_max_mb
        || before.advanced.shareable_log != after.advanced.shareable_log
    {
        p.log.reopen(
            &p.paths.log_dir(),
            super::log_files::Retention::new(after.advanced.log_keep_days, after.advanced.log_max_mb),
            after.advanced.shareable_log,
        );
    }
    if before.advanced.redact != after.advanced.redact {
        super::redaction::set_kinds(after.advanced.redact);
    }
    if before.export.folders != after.export.folders {
        super::pictures::retain(&p.pictures, &after.export.folders);
        for folder in after.export.folders.folders() {
            if let Some(path) = &folder.path {
                super::redaction::learn_dir(path);
            }
        }
    }
}

/// Another ffmpeg or ffprobe: the media probe, auto-crop and the interlace check use it for the next file, and the
/// encoders are checked again with the new ffmpeg (the Codecs panel and the profile editor follow).
fn programs_changed(p: &Parts, after: &Settings) {
    tracing::info!("[Settings] Programs changed; checking the encoders again");
    super::tools::check(
        &p.runtime,
        &p.ui,
        after.advanced.ffmpeg.clone(),
        after.advanced.ffprobe.clone(),
    );
    if let Some(media) = p.media.upgrade() {
        media.set_ffprobe(after.advanced.ffprobe.clone());
    }
    if let Some(filters) = p.filters.upgrade() {
        filters.set_ffmpeg(after.advanced.ffmpeg.clone());
    }
    if let Some(render) = p.render.upgrade() {
        let ffmpeg = after
            .advanced
            .ffmpeg
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("ffmpeg"));
        render.set_ffmpeg(
            ffmpeg,
            spawn_capability_check(&p.runtime, after.advanced.ffmpeg.clone()),
        );
    }
}

/// What the RustView link itself uses (the history size and the tree's mode are the panel's).
fn link(s: &RustViewSettings) -> RustViewSettings {
    RustViewSettings {
        tag_history_size: 0,
        compact_tree: false,
        ..s.clone()
    }
}

fn send_queue(queue: &QueueHandle, command: QueueCommand) {
    if queue.send(command).is_err() {
        tracing::warn!("[Settings] The queue has stopped; the change applies at the next start");
    }
}
