//! Startup and shutdown of the application: command line, profile, logging, single instance, runtime, Slint backend, main
//! window, event loop, and the one quit path (`shutdown`).

mod cli;
mod dialogs;
mod live;
pub mod log_book;
mod logging;
mod persistence;
mod services;
mod shutdown;
mod tools;
pub mod window;

use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use clap::Parser as _;
use rustcut_core::app_identity::{APP_NAME, VERSION};
use rustcut_core::hashing::spawn_hash_service;
use rustcut_core::queue::SyncLink;
use rustcut_core::render::{HardwareContext, RenderEnv, probe_dir, spawn_capability_check};
use rustcut_core::rustview::{CliClient, SyncCommand, spawn_sync};
use rustcut_core::settings::{AppPaths, Settings};
use slint::ComponentHandle as _;

pub use cli::Cli;
pub use persistence::SettingsWriter;

use self::services::{
    forward, listen, open_store, start_queue, start_runtime, stop_hashing, stop_player, stop_queue, stop_store,
    stop_sync,
};
use crate::MainWindow;
use crate::desktop::instance;
use crate::desktop::service::spawn_desktop;
use crate::ui::dispatcher::{SlintSink, UiDispatcher};
use crate::ui::dock::{DockBridge, PanelId, persist as dock_persist};
use crate::ui::editing::{EditingParts, JobEditor};
use crate::ui::editor::Editor;
use crate::ui::export::{ExportBridge, ExportParts};
use crate::ui::filters::FiltersBridge;
use crate::ui::inspector::InspectorBridge;
use crate::ui::jump::JumpBridge;
use crate::ui::keymap::{Action, Keymap};
use crate::ui::logs::LogsBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::menu::ContextMenuBridge;
use crate::ui::notices;
use crate::ui::open::{FileOpener, OpenerParts};
use crate::ui::player::{PlayerBridge, PlayerBridgeError};
use crate::ui::power::PowerBridge;
use crate::ui::probe::{self, ProbeBridge};
use crate::ui::problems::{ProblemParts, ProblemsBridge};
use crate::ui::queue::{MenuKeys, QueueBridge};
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::settings::SettingsBridge;
use crate::ui::tags::TagsBridge;
use crate::ui::tray::TrayBridge;

/// How long the runtime may take to finish its tasks at quit.
const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(2);

/// Errors that end the application before or while the event loop runs.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("the window system could not be initialized: {0}")]
    Platform(#[from] slint::PlatformError),
    #[error("no home folder is known; start with --home <dir>")]
    NoHome,
    #[error("the async runtime could not be started: {0}")]
    Runtime(std::io::Error),
    #[error("the player could not be started: {0}")]
    Player(#[from] PlayerBridgeError),
}

/// How a run ended.
enum Outcome {
    Quit,
    /// A running instance of the same profile took the files.
    Forwarded,
}

/// Runs the application and returns the process exit code.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let log = logging::init();
    tracing::info!(version = VERSION, "[App] {APP_NAME} starting");
    let result = run(&cli, &log);
    let code = match result {
        Ok(Outcome::Quit) => {
            tracing::info!("[App] Exited normally");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Forwarded) => {
            tracing::info!("[App] Handed over to the running instance");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "[App] Exited with an error");
            ExitCode::FAILURE
        }
    };
    // Flushes the log file.
    drop(log);
    code
}

fn run(cli: &Cli, log: &logging::LogControl) -> Result<Outcome, AppError> {
    if let Some(home) = &cli.home {
        tracing::info!(home = %home.display(), "[App] Isolated profile requested");
    }
    let paths = AppPaths::resolve(cli.home.as_deref()).ok_or(AppError::NoHome)?;
    let loaded = persistence::load_all(paths);
    log.apply_setting(&loaded.settings.advanced.log_level);
    log.open_file(&loaded.paths.log_dir(), loaded.settings.advanced.log_keep_days);

    let single_instance = loaded.settings.general.single_instance;
    let socket = instance::socket_path(&loaded.paths.config_dir);
    if single_instance && forward(&socket, cli) {
        return Ok(Outcome::Forwarded);
    }

    let runtime = start_runtime()?;
    let ui = UiDispatcher::new(SlintSink, runtime.handle().clone());
    window::select_backend()?;
    let main = MainWindow::new()?;
    main.set_app_name(APP_NAME.into());
    main.set_app_version(VERSION.into());
    window::restore(&main, &loaded.ui_state.window);
    dialogs::restore(&main, &loaded.ui_state.dialogs);
    let quit = shutdown::QuitPath::install(&main, &loaded.ui_state.window);
    let dock = DockBridge::install(&main, dock_persist::from_table(loaded.ui_state.dock.as_ref()));
    let player = PlayerBridge::start(&main, &ui, &loaded.settings.player, &loaded.ui_state.export)?;
    let (hashing, _hash_task) = spawn_hash_service(runtime.handle());
    // Which encoders work here (ADR-14); the Render Settings panel offers only those.
    let encoders = spawn_capability_check(runtime.handle(), loaded.settings.advanced.ffmpeg.clone());
    let writer = SettingsWriter::start(
        runtime.handle(),
        loaded.paths.config_dir.clone(),
        loaded.settings_read_only,
        &loaded.settings,
    );
    let (store, store_thread, store_problem) = open_store(&loaded.paths);
    // The RustView link (ADR-6): one `rustview api` call at a time on the runtime; the queue tells it about the jobs.
    let (sync, sync_records, _sync_task) =
        spawn_sync(runtime.handle(), loaded.settings.rustview.clone(), CliClient::new);
    let sync_link = SyncLink {
        handle: sync.handle.clone(),
        records: sync_records,
    };
    let queue_service = start_queue(&runtime, &store, &loaded, &encoders, sync_link);
    live::look(&main, &loaded.settings);
    crate::ui::tooltip::install(&main);
    crate::ui::controls::install(&main);
    crate::ui::drag::install(&main);
    let menu = ContextMenuBridge::install(&main);
    crate::ui::menu::bar::install(&main);
    // Every message goes through the notice center (M6.5); what came before it is shown now.
    let notice_center = notices::install(
        &main,
        notices::Parts {
            ui: &ui,
            runtime: runtime.handle().clone(),
            writer: writer.clone(),
            menu: &menu,
            settings: &loaded.settings.notices,
        },
    );
    tools::check(
        runtime.handle(),
        &ui,
        loaded.settings.advanced.ffmpeg.clone(),
        loaded.settings.advanced.ffprobe.clone(),
    );
    let (keys, save_key) = queue_keys(&loaded.settings);
    let queue = QueueBridge::install(
        &main,
        &ui,
        runtime.handle(),
        &queue_service,
        &loaded.ui_state.queue,
        &menu,
        keys,
        &save_key,
    );
    // The queue's pause is saved at once, so a crash keeps it (user decision Q-P8b).
    let ui_writer = Rc::new(persistence::UiStateWriter::start(runtime.handle(), &loaded));
    {
        let writer = Rc::clone(&ui_writer);
        queue.hooks().paused_changed = Some(Box::new(move |paused| writer.update(|s| s.queue.paused = paused)));
    }
    let tags = TagsBridge::install(
        &main,
        (&ui, runtime.handle().clone(), &sync, queue_service.handle.clone()),
        writer.clone(),
        loaded.settings.rustview.enabled,
        &loaded.ui_state.tags,
    );
    {
        let dock_for_queue = Rc::downgrade(&dock);
        let dock = Rc::downgrade(&dock);
        tags.set_on_reveal(move || {
            if let Some(dock) = dock.upgrade() {
                dock.reveal(PanelId::TagRate);
            }
        });
        let tags_weak = Rc::downgrade(&tags);
        queue.hooks().edit_tags = Some(Box::new(move |id, name| {
            if let Some(tags) = tags_weak.upgrade() {
                tags.edit_job(id, name);
            }
        }));
        let dock = dock_for_queue;
        queue.hooks().reveal = Some(Box::new(move |panel| {
            if let Some(dock) = dock.upgrade() {
                dock.reveal(panel);
            }
        }));
    }
    let inspector = InspectorBridge::install(
        &main,
        &ui,
        runtime.handle().clone(),
        &queue,
        &tags,
        &loaded.ui_state.job.collapsed,
    );
    let logs = LogsBridge::install(
        &main,
        &ui,
        runtime.handle().clone(),
        &queue,
        &loaded.ui_state.logs,
        log.book(),
    );
    // The bell's "Show all": the Logs panel with the messages of this session.
    {
        let (dock, logs) = (Rc::downgrade(&dock), Rc::downgrade(&logs));
        notice_center.hooks().show_all = Some(Box::new(move || {
            if let Some(dock) = dock.upgrade() {
                dock.reveal(PanelId::Logs);
            }
            if let Some(logs) = logs.upgrade() {
                logs.show_messages();
            }
        }));
    }
    {
        let (dock, logs) = (Rc::downgrade(&dock), Rc::downgrade(&logs));
        notice_center.hooks().show_log = Some(Box::new(move || {
            if let Some(dock) = dock.upgrade() {
                dock.reveal(PanelId::Logs);
            }
            if let Some(logs) = logs.upgrade() {
                logs.show_app_log();
            }
        }));
    }
    {
        let (inspector, logs) = (Rc::downgrade(&inspector), Rc::downgrade(&logs));
        queue.hooks().refreshed = Some(Box::new(move || {
            if let Some(inspector) = inspector.upgrade() {
                inspector.follow_queue();
            }
            if let Some(logs) = logs.upgrade() {
                logs.follow_queue();
            }
        }));
    }
    let render = RenderSettingsBridge::install(
        &main,
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
        writer.clone(),
        &loaded.paths.cache_dir,
        encoders,
    );
    {
        let (a, b) = (Rc::downgrade(&render), Rc::downgrade(&render));
        let mut hooks = queue.hooks();
        hooks.profiles = Some(Box::new(move || a.upgrade().map(|r| r.profiles()).unwrap_or_default()));
        hooks.profile = Some(Box::new(move |id| b.upgrade().and_then(|r| r.profile(id))));
    }
    let media = MediaInfoBridge::install(
        &main,
        &ui,
        runtime.handle().clone(),
        hashing.clone(),
        Some(sync.handle.clone()),
        &loaded.settings,
    );
    media.set_write_sidecar(loaded.settings.hashing.write_sidecar);
    let filters = FiltersBridge::install(
        &main,
        &player,
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
    );
    let editor = Editor::install(
        &main,
        &player,
        &media,
        &filters,
        &loaded.settings,
        &loaded.ui_state.timeline,
    );
    let probe = ProbeBridge::install(
        &main,
        runtime.handle().clone(),
        &probe::Parts {
            player: &player,
            editor: &editor,
            filters: &filters,
            render: &render,
        },
        RenderEnv::new(&loaded.settings, &loaded.paths, HardwareContext::default()),
        probe_dir(&loaded.paths.cache_dir),
        loaded.ui_state.export.probe,
        &editor.key_label(Action::ProbeRender),
    );
    {
        let editor = Rc::downgrade(&editor);
        filters.set_sections_source(move || editor.upgrade().map(|e| e.section_ranges()).unwrap_or_default());
    }
    editor.set_probe(&probe);
    editor.set_tags(&tags);
    editor.set_queue(&queue);
    editor.set_menu(&menu);
    let export = ExportBridge::install(
        &main,
        &ExportParts {
            player: &player,
            editor: &editor,
            filters: &filters,
            media: &media,
            render: &render,
            queue: &queue,
            tags: &tags,
        },
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
        writer.clone(),
    );
    {
        // The known-source banner's "Show N files" (M6.5d).
        let export = Rc::downgrade(&export);
        crate::ui::media_info::known_source::install_dialog(move |list| {
            if let Some(export) = export.upgrade() {
                export.show_known(list);
            }
        });
    }
    editor.set_export(&export);
    // The card checks the job again when the render settings or the source's description change (Q-N6).
    {
        let weak = Rc::downgrade(&export);
        render.set_changed_hook(move || {
            if let Some(export) = weak.upgrade() {
                export.job_changed();
            }
        });
        let weak = Rc::downgrade(&export);
        crate::ui::media_info::add_probe_hook(move |_| {
            if let Some(export) = weak.upgrade() {
                export.job_changed();
            }
        });
    }
    let opener = FileOpener::install(
        &main,
        &OpenerParts {
            player: &player,
            media: &media,
            editor: &editor,
            filters: &filters,
            probe: &probe,
            export: &export,
        },
        runtime.handle().clone(),
        loaded.ui_state.last_open_dir.clone(),
    );
    let editing = JobEditor::install(
        &main,
        &ui,
        runtime.handle().clone(),
        &EditingParts {
            editor: &editor,
            filters: &filters,
            render: &render,
            export: &export,
            tags: &tags,
            queue: &queue,
            inspector: &inspector,
            opener: &opener,
        },
        loaded.settings.editor.on_job_load,
    );
    export.set_editing(&editing);
    opener.set_editing(&editing);
    {
        let editing = Rc::downgrade(&editing);
        queue.hooks().edit = Some(Box::new(move |id| {
            if let Some(editing) = editing.upgrade() {
                editing.load(id);
            }
        }));
    }
    // The single-instance socket: listening while `general.single_instance` is on (also switched while running).
    let server = Rc::new(std::cell::RefCell::new(
        single_instance
            .then(|| listen(&socket, runtime.handle(), &ui, &main.as_weak()))
            .flatten(),
    ));
    let instance: Box<dyn Fn(bool)> = {
        let (server, runtime, ui, main) = (Rc::clone(&server), runtime.handle().clone(), ui.clone(), main.as_weak());
        Box::new(move |on| {
            let mut server = server.borrow_mut();
            if !on {
                if server.take().is_some() {
                    tracing::info!("[Instance] No longer listening: a second start opens its own window");
                }
            } else if server.is_none() {
                *server = listen(&socket, &runtime, &ui, &main);
            }
        })
    };
    tags.set_history_size(loaded.settings.rustview.tag_history_size);
    // The desktop (P5): the sleep lock and the taskbar entry follow the queue; "when done" and the close question.
    let desktop = spawn_desktop(
        runtime.handle(),
        loaded.settings.desktop.inhibit_sleep,
        loaded.settings.desktop.taskbar_progress,
    );
    let power = PowerBridge::install(
        &main,
        (runtime.handle(), &queue, &menu),
        desktop,
        &loaded.settings,
        loaded
            .ui_state
            .queue
            .when_done
            .unwrap_or(loaded.settings.queue.when_done),
    );
    {
        let power = Rc::downgrade(&power);
        editor.set_escape_hook(move || power.upgrade().is_some_and(|p| p.escape()));
    }
    let tray = TrayBridge::install(&main, &queue, &player, &loaded.settings.desktop.tray);
    let settings_window = SettingsBridge::install(
        &main,
        runtime.handle().clone(),
        writer.clone(),
        loaded.paths.clone(),
        &loaded.ui_state.settings,
    );
    // Jump marks lead from messages to the options (Q-N6); held until the event loop ends.
    let _jump = JumpBridge::install(&main, &dock, &settings_window, &queue);
    {
        // The Job panel's profile row (P9.11): a jump to the job's profile when the library holds it unchanged.
        let writer = writer.clone();
        let settings = Rc::downgrade(&settings_window);
        inspector.set_profile_hooks(
            move |profile| {
                rustcut_core::render::ProfileLibrary::new(writer.current().render.profiles).get(&profile.id)
                    == Some(profile)
            },
            move |id| {
                if let Some(settings) = settings.upgrade() {
                    settings.show_profile(id);
                }
            },
        );
    }
    {
        let mut hooks = settings_window.hooks();
        let sync_handle = sync.handle.clone();
        hooks.probe_rustview = Some(Box::new(move || {
            let _ = sync_handle.send(SyncCommand::Probe);
        }));
        let render = Rc::downgrade(&render);
        hooks.encoders = Some(Box::new(move || {
            render.upgrade().map(|r| r.encoders()).unwrap_or_default()
        }));
    }
    live::connect(
        &writer,
        live::Parts {
            main: main.as_weak(),
            editor: Rc::downgrade(&editor),
            player: Rc::downgrade(&player),
            export: Rc::downgrade(&export),
            filters: Rc::downgrade(&filters),
            render: Rc::downgrade(&render),
            tags: Rc::downgrade(&tags),
            editing: Rc::downgrade(&editing),
            queue: queue_service.handle.clone(),
            sync: sync.handle.clone(),
            paths: loaded.paths.clone(),
            env: RenderEnv::new(&loaded.settings, &loaded.paths, HardwareContext::default()),
            log: log.level_setter(),
            media: Rc::downgrade(&media),
            probe: Rc::downgrade(&probe),
            runtime: runtime.handle().clone(),
            instance,
            power: Rc::downgrade(&power),
            tray: Rc::downgrade(&tray),
            notices: Rc::downgrade(&notice_center),
            ui: ui.clone(),
        },
    );
    shutdown::watch_signals(runtime.handle(), ui, main.as_weak());

    // A newer settings file or an unusable queue database: their dialogs come first (M6.5d).
    let _problems = ProblemsBridge::install(
        &main,
        ProblemParts {
            runtime: runtime.handle().clone(),
            paths: &loaded.paths,
            writer,
            ui_read_only: Rc::clone(&loaded.ui_read_only),
            store: store.clone(),
            queue: queue_service.handle.clone(),
            queue_bridge: &queue,
            power: &power,
        },
        loaded.newer.clone(),
        store_problem,
    );
    if let Some(first) = cli.files.first() {
        opener.open(first.clone());
    }
    if cli.files.len() > 1 {
        tracing::info!(
            count = cli.files.len(),
            "[App] Only the first file on the command line is opened"
        );
    }
    // Not `main.run()`: the event loop has to go on while the window is hidden in the tray; the quit path ends it.
    main.show()?;
    slint::run_event_loop_until_quit()?;
    let _ = main.hide();

    // The single quit path: save what the user arranged, then stop the services.
    let mut ui_state = loaded.ui_state.clone();
    ui_state.window = quit.geometry(&main);
    ui_state.dock = dock_persist::to_table(&dock.layout());
    (ui_state.export.volume, ui_state.export.muted) = player.volume_and_mute();
    ui_state.last_open_dir = opener.last_open_dir();
    ui_state.timeline = editor.timeline_state();
    filters.save_into(&mut ui_state.export);
    render.save_into(&mut ui_state.export);
    probe.save_into(&mut ui_state.export);
    export.save_into(&mut ui_state.export);
    tags.close_filter();
    tags.save_into(&mut ui_state.tags);
    queue.save_into(&mut ui_state.queue);
    power.save_into(&mut ui_state.queue);
    ui_state.job.collapsed = inspector.collapsed();
    dialogs::capture(&main, &mut ui_state.dialogs);
    logs.save_into(&mut ui_state.logs);
    settings_window.save_into(&mut ui_state.settings);
    // A write of the pause that still runs ends first, and none follows the full state.
    ui_writer.close();
    persistence::save_ui_state(&loaded, &ui_state);
    settings_window.shut_down();
    // Stop listening now (the settings' listener holds the holder until the end of `run`).
    drop(server.borrow_mut().take());
    drop(settings_window);
    drop(editing);
    drop(opener);
    drop(export);
    drop(logs);
    drop(inspector);
    // The desktop actor ends with its last handle: the sleep lock goes, the taskbar entry is cleared.
    drop(tray);
    drop(power);
    drop(queue);
    drop(menu);
    drop(tags);
    drop(probe);
    drop(render);
    drop(editor);
    drop(filters);
    drop(media);
    drop(main);
    stop_hashing(&hashing);
    stop_sync(&sync);
    stop_queue(&queue_service);
    stop_store(&store, store_thread);
    stop_player(player);
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
    Ok(Outcome::Quit)
}

/// The queue's key labels for its tooltips and menu (the keymap as the editor reads it), and the Save key.
fn queue_keys(settings: &Settings) -> (MenuKeys, String) {
    let (keymap, _notes) = Keymap::from_table(&settings.keymap);
    (
        MenuKeys {
            move_up: keymap.tooltip_keys(Action::QueueMoveUp),
            move_down: keymap.tooltip_keys(Action::QueueMoveDown),
            move_top: keymap.first_label(Action::QueueMoveTop),
            move_bottom: keymap.first_label(Action::QueueMoveBottom),
            menu_up: keymap.first_label(Action::QueueMoveUp),
            menu_down: keymap.first_label(Action::QueueMoveDown),
            select_all: keymap.first_label(Action::QueueSelectAll),
        },
        keymap.first_label(Action::Save),
    )
}
