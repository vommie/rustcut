//! Settings that apply while RustCut runs (M4.2): after every change through the settings writer, the parts that use a
//! changed value get it. Values used only at start (paths of the programs, hardware decoding, single instance, ...)
//! are marked "Applies at the next start" in the Settings window.

use std::rc::Weak;

use rustcut_core::queue::{QueueCommand, QueueHandle};
use rustcut_core::render::RenderEnv;
use rustcut_core::rustview::{SyncCommand, SyncHandle};
use rustcut_core::settings::{AppPaths, RustViewSettings, Settings};
use slint::ComponentHandle as _;

use super::SettingsWriter;
use super::logging::LevelSetter;
use crate::ui::editing::JobEditor;
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::filters::FiltersBridge;
use crate::ui::player::PlayerBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::tags::TagsBridge;
use crate::{HoldConfirm, MainWindow, TooltipSettings};

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
    pub paths: AppPaths,
    /// The renders' environment as started (the programs stay those of the start).
    pub env: RenderEnv,
    pub log: LevelSetter,
}

/// The hold time and the tooltips of the main window.
pub fn look(main: &MainWindow, settings: &Settings) {
    main.global::<HoldConfirm<'_>>()
        .set_duration(i64::from(settings.general.hold_to_confirm_ms));
    let tooltips = main.global::<TooltipSettings<'_>>();
    tooltips.set_enabled(settings.appearance.tooltips);
    tooltips.set_delay(i64::from(settings.appearance.tooltip_delay_ms));
}

pub fn connect(writer: &SettingsWriter, parts: Parts) {
    writer.subscribe(move |before, after| apply(&parts, before, after));
}

fn apply(p: &Parts, before: &Settings, after: &Settings) {
    if (before.general.hold_to_confirm_ms != after.general.hold_to_confirm_ms || before.appearance != after.appearance)
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
        player.apply_settings(&after.player);
    }
    if (before.export != after.export
        || before.warnings != after.warnings
        || before.editor.auto_section != after.editor.auto_section)
        && let Some(export) = p.export.upgrade()
    {
        export.settings_changed(after);
    }
    if before.filters != after.filters
        && let Some(filters) = p.filters.upgrade()
    {
        filters.set_defaults(after.filters.clone());
    }
    if before.editor.draft_on_load != after.editor.draft_on_load
        && let Some(editing) = p.editing.upgrade()
    {
        editing.set_draft_on_load(after.editor.draft_on_load);
    }
    if before.render.profiles != after.render.profiles
        && let Some(render) = p.render.upgrade()
    {
        render.profiles_changed(&after.render.profiles);
    }
    let process = |s: &Settings| (s.render.nice, s.render.idle_io, s.render.stall_timeout_secs);
    if process(before) != process(after) {
        let hardware = p
            .render
            .upgrade()
            .and_then(|r| r.capabilities())
            .map_or_else(|| p.env.hardware.clone(), |c| c.hardware.clone());
        let fresh = RenderEnv::new(after, &p.paths, hardware);
        let env = RenderEnv {
            ffmpeg: p.env.ffmpeg.clone(),
            probe: p.env.probe.clone(),
            ..fresh
        };
        send_queue(&p.queue, QueueCommand::SetRenderEnv(Box::new(env)));
    }
    if before.queue.workers != after.queue.workers {
        send_queue(&p.queue, QueueCommand::SetWorkers(after.queue.workers));
    }
    if before.rustview.tag_history_size != after.rustview.tag_history_size
        && let Some(tags) = p.tags.upgrade()
    {
        tags.set_history_size(after.rustview.tag_history_size);
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
    if before.advanced.log_level != after.advanced.log_level {
        p.log.apply(&after.advanced.log_level);
    }
}

/// What the RustView link itself uses (the history size is the panel's).
fn link(s: &RustViewSettings) -> RustViewSettings {
    RustViewSettings {
        tag_history_size: 0,
        ..s.clone()
    }
}

fn send_queue(queue: &QueueHandle, command: QueueCommand) {
    if queue.send(command).is_err() {
        tracing::warn!("[Settings] The queue has stopped; the change applies at the next start");
    }
}
