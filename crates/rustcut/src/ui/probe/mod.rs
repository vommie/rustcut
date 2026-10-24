//! The probe render (ADR-15, user decision Q-E1): a few seconds around the playhead with the current filters and the
//! working render settings, rendered on the runtime through the render engine into the cache, then played in the main
//! player in a loop while the timeline waits; Escape or the banner's button returns to the source at the same place.
//! With "exact settings" the probe uses the profile's own speed and estimates the job's size.

pub mod text;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use rustcut_core::edit::FilterKind;
use rustcut_core::render::{
    Control, JobLog, PROBE_JOB, PROBE_SECS, ProbeInput, RenderEnv, RenderProfile, RenderUpdate, RenderedFile,
    clear_probe_dir, estimate_size, probe_env, probe_job, render_job, scale_size,
};
use rustcut_core::settings::{ExportPanelState, ProbeState};
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;
use tokio::sync::{mpsc, watch};

use crate::ui::editor::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::player::PlayerBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::{MainWindow, Probe};

/// What the render task reports to the UI thread.
enum Message {
    Update(RenderUpdate),
    /// The result; an error says whether it was canceled.
    Done(Result<RenderedFile, (bool, String)>),
}

/// A probe that renders.
struct Running {
    control: watch::Sender<Control>,
    window: (Timestamp, Timestamp),
    job: Timestamp,
    exact: bool,
    profile: RenderProfile,
    /// What the running pass does ("Pass 1 of 2: deshake analysis").
    line: String,
}

/// A finished probe.
#[derive(Clone)]
struct Finished {
    path: PathBuf,
    banner: String,
}

/// Where the player goes back to.
struct Source {
    path: PathBuf,
    position: Timestamp,
    paused: bool,
}

pub struct ProbeBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    player: Weak<PlayerBridge>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    render: Weak<RenderSettingsBridge>,
    /// The jobs' render environment (the probe's work folders replace its own).
    env: RefCell<RenderEnv>,
    folder: PathBuf,
    seconds: Cell<u16>,
    exact: Cell<bool>,
    /// Changes with every opened file: results of a probe of another file are dropped.
    generation: Cell<u64>,
    running: RefCell<Option<Running>>,
    finished: RefCell<Option<Finished>>,
    playing: RefCell<Option<Source>>,
    status: RefCell<(String, bool)>,
    progress: Cell<f32>,
    /// The probe-playing banner is shown (posted only when this changes).
    shown_playing: Cell<bool>,
}

/// The bridges a probe needs.
pub struct Parts<'a> {
    pub player: &'a Rc<PlayerBridge>,
    pub editor: &'a Rc<Editor>,
    pub filters: &'a Rc<FiltersBridge>,
    pub render: &'a Rc<RenderSettingsBridge>,
}

impl ProbeBridge {
    /// The programs and the priority of the next probe render (changed in the Settings window).
    pub fn set_env(&self, env: RenderEnv) {
        *self.env.borrow_mut() = env;
    }

    /// Takes over the probe card and the banner. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        runtime: tokio::runtime::Handle,
        parts: &Parts<'_>,
        env: RenderEnv,
        folder: PathBuf,
        saved: ProbeState,
        key: &str,
    ) -> Rc<Self> {
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime,
            player: Rc::downgrade(parts.player),
            editor: Rc::downgrade(parts.editor),
            filters: Rc::downgrade(parts.filters),
            render: Rc::downgrade(parts.render),
            env: RefCell::new(env),
            folder,
            seconds: Cell::new(saved.seconds.clamp(*PROBE_SECS.start(), *PROBE_SECS.end())),
            exact: Cell::new(saved.exact),
            generation: Cell::new(0),
            running: RefCell::new(None),
            finished: RefCell::new(None),
            playing: RefCell::new(None),
            status: RefCell::new((String::new(), false)),
            progress: Cell::new(0.0),
            shown_playing: Cell::new(false),
        });
        let probe = window.global::<Probe<'_>>();
        let help = |id: rustcut_core::render::OptionId| id.help().tooltip;
        probe.set_tip_render(help(rustcut_core::render::OptionId::ProbeRender).into());
        probe.set_tip_seconds(help(rustcut_core::render::OptionId::ProbeLength).into());
        probe.set_tip_exact(help(rustcut_core::render::OptionId::ProbeExact).into());
        probe.set_key_render(key.into());
        connect(&bridge, &probe);
        // Probes of an earlier run are of no use: the folder is emptied in the background.
        let folder = bridge.folder.clone();
        bridge.runtime.spawn(async move { clear_probe_dir(&folder).await });
        bridge.publish();
        bridge
    }

    /// The switches into the UI state (at quit).
    pub fn save_into(&self, export: &mut ExportPanelState) {
        export.probe = ProbeState {
            seconds: self.seconds.get(),
            exact: self.exact.get(),
        };
    }

    /// Another video opens: a running probe stops, the player leaves the probe, the last probe is forgotten.
    pub fn file_opening(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(run) = self.running.borrow_mut().take() {
            run.control.send_replace(Control::Cancel);
            notices::clear(PROGRESS);
        }
        if self.playing.borrow_mut().take().is_some() {
            self.leave_probe_mode();
        }
        self.finished.borrow_mut().take();
        self.set_status(String::new(), false);
        self.publish();
    }

    /// Renders a probe around the playhead (the key, the button, the menu).
    pub fn render(&self) {
        if self.running.borrow().is_some() {
            return;
        }
        match self.start() {
            Ok(()) => {}
            Err(message) => {
                self.set_status(message.clone(), true);
                tracing::info!(why = %message, "[Render] No probe render");
                notices::post(
                    Notice::new(Kind::ProbeFailed, "The probe render cannot start")
                        .detail(message)
                        .level(Level::Warning),
                );
            }
        }
        self.publish();
    }

    fn start(&self) -> Result<(), String> {
        let (Some(player), Some(editor), Some(filters), Some(render)) = (
            self.player.upgrade(),
            self.editor.upgrade(),
            self.filters.upgrade(),
            self.render.upgrade(),
        ) else {
            return Err("The probe render is not ready.".to_owned());
        };
        let source = match &*self.playing.borrow() {
            Some(source) => Some(source.path.clone()),
            None => player.current_file(),
        };
        let (Some(source), Some((playhead, duration, job))) = (source, editor.probe_point()) else {
            return Err("Open a video first: the probe renders a few seconds around the playhead.".to_owned());
        };
        let profile = render.working_profile();
        if !profile.is_usable() {
            return Err("The render settings have errors (see the Render Settings panel).".to_owned());
        }
        let caps = render.capabilities();
        let encoder = profile.video.encoder;
        if caps.as_ref().is_some_and(|c| !c.works(encoder)) {
            return Err(format!("{} does not work here.", encoder.spec().display_name));
        }
        let chain = filters.chain();
        let deshake = chain
            .slots()
            .iter()
            .any(|s| s.enabled && s.filter.kind() == FilterKind::Deshake);
        if deshake && caps.as_ref().is_some_and(|c| !c.vidstab) {
            return Err("Deshake needs ffmpeg with vid.stab (sudo pacman -S vid.stab).".to_owned());
        }
        let exact = self.exact.get();
        let job_spec = probe_job(&ProbeInput {
            source: &source,
            duration,
            playhead,
            seconds: self.seconds.get(),
            filters: &chain,
            profile: &profile,
            exact,
            job_duration: job,
            folder: &self.folder,
        })
        .ok_or_else(|| "The video's length is not known yet.".to_owned())?;
        let mut env = self.env.borrow().clone();
        if let Some(caps) = &caps {
            env.hardware = caps.hardware.clone();
        }
        let env = probe_env(&env, &self.folder);
        let (control, control_rx) = watch::channel(Control::Run);
        let (sender, receiver) = mpsc::unbounded_channel();
        let folder = self.folder.clone();
        let spec = job_spec.spec.clone();
        tracing::info!(
            source = %source.display(),
            start = %text::short(job_spec.window.start()),
            end = %text::short(job_spec.window.end()),
            exact,
            "[Render] Probe render"
        );
        self.runtime.spawn(async move {
            clear_probe_dir(&folder).await;
            let mut log = JobLog::new();
            let updates = sender.clone();
            let result = render_job(PROBE_JOB, &spec, &env, control_rx, &mut log, move |u| {
                // The receiver is gone only when the app quits.
                let _ = updates.send(Message::Update(u));
            })
            .await;
            if let Err(e) = &result
                && !e.is_canceled()
            {
                tracing::warn!(error = %e, log = %log.to_text(), "[Render] Probe render failed");
            }
            let _ = sender.send(Message::Done(result.map_err(|e| (e.is_canceled(), e.to_string()))));
        });
        let generation = self.generation.get();
        *self.running.borrow_mut() = Some(Running {
            control,
            window: (job_spec.window.start(), job_spec.window.end()),
            job,
            exact,
            profile: job_spec.spec.profile,
            line: "Rendering".to_owned(),
        });
        self.progress.set(0.0);
        self.set_status("Starting...".to_owned(), false);
        self.listen(receiver, generation);
        Ok(())
    }

    /// Takes the render task's messages on the UI thread.
    fn listen(&self, mut receiver: mpsc::UnboundedReceiver<Message>, generation: u64) {
        let this = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            while let Some(message) = receiver.recv().await {
                let Some(bridge) = this.upgrade() else { return };
                if bridge.generation.get() != generation {
                    return;
                }
                bridge.message(message);
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Render] The event loop is gone; the probe is not shown");
        }
    }

    fn message(&self, message: Message) {
        match message {
            Message::Update(RenderUpdate::Pass { pass, passes, kind }) => {
                let line = text::pass(pass, passes, kind);
                if let Some(run) = self.running.borrow_mut().as_mut() {
                    run.line.clone_from(&line);
                }
                self.set_status(line, false);
            }
            Message::Update(RenderUpdate::Progress(p)) => {
                self.progress.set(az::cast::<f64, f32>(p.fraction()));
                let line = self.running.borrow().as_ref().map(|r| {
                    let length = r.window.1.saturating_sub(r.window.0);
                    let estimate = estimate_size(&p)
                        .filter(|_| r.exact)
                        .and_then(|bytes| text::job_estimate(bytes, length, r.job));
                    estimate.map_or_else(|| r.line.clone(), |e| format!("{} · {e}", r.line))
                });
                if let Some(line) = line {
                    self.set_status(line, false);
                }
            }
            Message::Update(RenderUpdate::Verifying) => self.set_status("Checking the file...".to_owned(), false),
            Message::Update(RenderUpdate::Paused(_)) => {}
            Message::Done(result) => {
                let Some(run) = self.running.borrow_mut().take() else {
                    return;
                };
                match result {
                    Ok(file) => {
                        let length = run.window.1.saturating_sub(run.window.0);
                        let summary = text::finished(file.bytes, length, run.job, run.exact);
                        tracing::info!(
                            bytes = file.bytes,
                            estimate = ?scale_size(file.bytes, length, run.job),
                            "[Render] Probe render ready"
                        );
                        *self.finished.borrow_mut() = Some(Finished {
                            path: file.path,
                            banner: text::banner(run.window.0, run.window.1, &run.profile),
                        });
                        self.set_status(summary.clone(), false);
                        notices::post(
                            Notice::new(Kind::ProbeRender, "Probe render ready")
                                .detail(summary)
                                .level(Level::Success)
                                .key(PROGRESS),
                        );
                        self.play();
                    }
                    Err((true, _)) => {
                        notices::clear(PROGRESS);
                        self.set_status("Probe render stopped".to_owned(), false);
                    }
                    Err((false, reason)) => {
                        notices::clear(PROGRESS);
                        notices::post(
                            Notice::new(Kind::ProbeFailed, "The probe render failed").detail(reason.as_str()),
                        );
                        self.set_status(format!("Probe render failed: {reason}"), true);
                    }
                }
            }
        }
        self.publish();
    }

    /// Stops a running probe (the button).
    pub fn cancel(&self) {
        if let Some(run) = self.running.borrow().as_ref() {
            run.control.send_replace(Control::Cancel);
        }
    }

    /// Plays the last probe in the player (after it finished, and the button).
    pub fn play(&self) {
        let Some(finished) = self.finished.borrow().clone() else {
            return;
        };
        let (Some(player), Some(editor), Some(filters)) =
            (self.player.upgrade(), self.editor.upgrade(), self.filters.upgrade())
        else {
            return;
        };
        if self.playing.borrow().is_none() {
            let Some(path) = player.current_file() else { return };
            *self.playing.borrow_mut() = Some(Source {
                path,
                position: player.position(),
                paused: player.is_paused(),
            });
            filters.suspend_preview(true);
            editor.set_probe_playing(true);
        }
        if let Err(e) = player.open_probe(&finished.path) {
            tracing::warn!(error = %e, "[Player] The probe render could not be opened");
        }
        self.publish();
    }

    /// Back to the source at the place the probe started from (Escape, the banner's button, the menu).
    pub fn back(&self) {
        let Some(source) = self.playing.borrow_mut().take() else {
            return;
        };
        if let Some(player) = self.player.upgrade()
            && let Err(e) = player.return_to(&source.path, source.position, source.paused)
        {
            tracing::warn!(error = %e, "[Player] The source could not be opened again");
        }
        self.leave_probe_mode();
        self.publish();
    }

    fn leave_probe_mode(&self) {
        if let Some(player) = self.player.upgrade() {
            player.stop_loop();
        }
        if let Some(filters) = self.filters.upgrade() {
            filters.suspend_preview(false);
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.set_probe_playing(false);
        }
    }

    fn set_status(&self, text: String, failed: bool) {
        *self.status.borrow_mut() = (text, failed);
    }

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let probe = window.global::<Probe<'_>>();
        let (status, failed) = self.status.borrow().clone();
        probe.set_running(self.running.borrow().is_some());
        probe.set_progress(self.progress.get());
        probe.set_status(status.as_str().into());
        probe.set_failed(failed);
        let finished = self.finished.borrow();
        probe.set_ready(finished.is_some());
        probe.set_playing(self.playing.borrow().is_some());
        probe.set_banner(finished.as_ref().map(|f| f.banner.as_str()).unwrap_or_default().into());
        probe.set_seconds(self.seconds.get().to_string().into());
        probe.set_exact(self.exact.get());
        drop(finished);
        self.publish_notices();
    }

    /// The running probe's progress (a toast by default, Q-N10; the card always shows it too) and the banner with the
    /// way back while the probe plays.
    fn publish_notices(&self) {
        if let Some(run) = self.running.borrow().as_ref() {
            let weak = Weak::clone(&self.me);
            notices::post(
                Notice::new(Kind::ProbeRender, "Probe render")
                    .detail(format!(
                        "{} · {}",
                        text::span(run.window.0, run.window.1),
                        self.status.borrow().0
                    ))
                    .live(PROGRESS)
                    .progress(self.progress.get())
                    .action_keep("Stop", move || {
                        if let Some(b) = weak.upgrade() {
                            b.cancel();
                        }
                    }),
            );
        }
        let playing = self.playing.borrow().is_some();
        if playing == self.shown_playing.replace(playing) {
            return;
        }
        if playing {
            let title = self
                .finished
                .borrow()
                .as_ref()
                .map(|f| f.banner.clone())
                .unwrap_or_default();
            let weak = Weak::clone(&self.me);
            notices::post(
                Notice::new(Kind::ProbePlaying, title)
                    .detail("The player shows the probe in a loop; the timeline and the marks wait.")
                    .live(PLAYING)
                    .action("Back to the source", move || {
                        if let Some(b) = weak.upgrade() {
                            b.back();
                        }
                    }),
            );
        } else {
            notices::clear(PLAYING);
        }
    }
}

/// The keys of the probe's live notices.
const PROGRESS: &str = "probe_render";
const PLAYING: &str = "probe_playing";

fn connect(bridge: &Rc<ProbeBridge>, probe: &Probe<'_>) {
    let this = Rc::downgrade(bridge);
    probe.on_render(move || {
        if let Some(b) = this.upgrade() {
            b.render();
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_cancel(move || {
        if let Some(b) = this.upgrade() {
            b.cancel();
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_play(move || {
        if let Some(b) = this.upgrade() {
            b.play();
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_back(move || {
        if let Some(b) = this.upgrade() {
            b.back();
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_seconds_typed(move |text| {
        let Some(b) = this.upgrade() else { return false };
        match text.trim().parse::<u16>() {
            Ok(s) if PROBE_SECS.contains(&s) => {
                b.seconds.set(s);
                b.publish();
                true
            }
            _ => false,
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_seconds_stepped(move |steps| {
        if let Some(b) = this.upgrade() {
            let next = i32::from(b.seconds.get()).saturating_add(steps);
            let clamped = next.clamp(i32::from(*PROBE_SECS.start()), i32::from(*PROBE_SECS.end()));
            b.seconds.set(u16::try_from(clamped).unwrap_or(*PROBE_SECS.start()));
            b.publish();
        }
    });
    let this = Rc::downgrade(bridge);
    probe.on_toggle_exact(move || {
        if let Some(b) = this.upgrade() {
            b.exact.set(!b.exact.get());
            b.publish();
        }
    });
}
