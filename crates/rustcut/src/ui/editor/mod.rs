//! The editor: the sections of the open video, the timeline that shows them, the Sections card that lists them, and the
//! keymap that drives all of it. The pure parts live in `ui::sections`, `ui::timeline` and `ui::keymap`; this bridge keeps
//! their state on the UI thread, handles the Slint callbacks (`timeline_input.rs`, `sections_input.rs`, `actions.rs`) and
//! publishes the views.
//!
//! The player's views arrive through `player::set_view_hook` (UI thread, at most at the playhead rate). Every handler
//! changes the state in one short borrow and calls the player only after it, so a view that the player shows while the
//! editor is busy finds the state free (and is skipped if not: the editor publishes itself afterwards).

mod actions;
mod sections_input;
mod timeline_input;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use az::CheckedAs as _;
use rustcut_core::edit::{Section, SectionList};
use rustcut_core::settings::{AutoSection, EditorSettings, Settings, TimelineState};
use rustcut_core::time::{FrameRate, Timestamp};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::ui::export::ExportBridge;
use crate::ui::filters::{FilterKeys, FiltersBridge};
use crate::ui::keymap::{Action, Keymap};
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::player::{self, PlaybackView, PlayerBridge};
use crate::ui::probe::ProbeBridge;
use crate::ui::queue::QueueBridge;
use crate::ui::sections::{Bound, Edge, SectionsDraft, SectionsState};
use crate::ui::status::{self, Level};
use crate::ui::tags::TagsBridge;
use crate::ui::timeline::{Viewport, view};
use crate::util::model::sync;
use crate::{Export, MainWindow, Playback, SectionRowView, Sections, Shortcuts, Timeline, TimelineBlock, TimelineTick};

/// Frame rates outside this range are not a frame grid (Matroska's 1000/1 time base, broken headers).
const PLAUSIBLE_FPS: std::ops::RangeInclusive<f64> = 1.0..=300.0;

/// A press on the timeline that is still held.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Scrubbing: the playhead follows the pointer.
    Scrub,
    /// Moving an edge; `offset` is where the pointer grabbed it, relative to the edge.
    Edge { edge: Edge, offset: f32 },
}

#[derive(Debug)]
struct State {
    sections: SectionsState,
    viewport: Viewport,
    playhead: Timestamp,
    has_video: bool,
    drag: Option<Drag>,
    /// Where the pointer hovers over the timeline.
    hover: Option<f32>,
    /// The pointer is on a draggable edge (resize cursor).
    edge_hovered: bool,
}

pub struct Editor {
    window: slint::Weak<MainWindow>,
    player: Weak<PlayerBridge>,
    media: Weak<MediaInfoBridge>,
    filters: Weak<FiltersBridge>,
    keymap: RefCell<Keymap>,
    seek_step: Cell<f64>,
    seek_step_long: Cell<f64>,
    state: RefCell<State>,
    /// The probe render (set after it is installed; it needs the editor first).
    probe: RefCell<Weak<ProbeBridge>>,
    /// The Tag & Rate panel (Escape ends the editing of a job's tags).
    tags: RefCell<Weak<TagsBridge>>,
    /// Escape goes here first (the power bridge aborts its countdown; `true` = taken).
    escape: RefCell<Option<Box<dyn Fn() -> bool>>>,
    /// The player shows the probe render: the timeline, the marks and the sections wait (user decision Q-E1).
    probe_playing: Cell<bool>,
    /// The export panel that saves jobs (set after it is installed; it needs the editor first).
    export: RefCell<Weak<ExportBridge>>,
    /// The queue panel (the queue keys move its selected jobs).
    queue: RefCell<Weak<QueueBridge>>,
    /// The status line shows a refused edit: the next accepted one replaces it.
    warned: Cell<bool>,
    ticks: Rc<VecModel<TimelineTick>>,
    blocks: Rc<VecModel<TimelineBlock>>,
    rows: Rc<VecModel<SectionRowView>>,
}

impl Editor {
    /// Takes over the timeline, the Sections card and the keys. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        player: &Rc<PlayerBridge>,
        media: &Rc<MediaInfoBridge>,
        filters: &Rc<FiltersBridge>,
        settings: &Settings,
        timeline: &TimelineState,
    ) -> Rc<Self> {
        let (keymap, notes) = Keymap::from_table(&settings.keymap);
        for note in &notes {
            tracing::warn!(note = %note, "[Keymap] Repaired");
        }
        let EditorSettings {
            seek_step_secs,
            seek_step_long_secs,
            ..
        } = settings.editor;
        let editor = Rc::new(Self {
            window: window.as_weak(),
            player: Rc::downgrade(player),
            media: Rc::downgrade(media),
            filters: Rc::downgrade(filters),
            keymap: RefCell::new(keymap),
            seek_step: Cell::new(f64::from(seek_step_secs)),
            seek_step_long: Cell::new(f64::from(seek_step_long_secs)),
            state: RefCell::new(State {
                sections: SectionsState::default(),
                viewport: Viewport::new(Timestamp::ZERO, 0.0, timeline.zoom),
                playhead: Timestamp::ZERO,
                has_video: false,
                drag: None,
                hover: None,
                edge_hovered: false,
            }),
            probe: RefCell::new(Weak::new()),
            tags: RefCell::new(Weak::new()),
            escape: RefCell::new(None),
            probe_playing: Cell::new(false),
            export: RefCell::new(Weak::new()),
            queue: RefCell::new(Weak::new()),
            warned: Cell::new(false),
            ticks: Rc::new(VecModel::default()),
            blocks: Rc::new(VecModel::default()),
            rows: Rc::new(VecModel::default()),
        });
        let tl = window.global::<Timeline<'_>>();
        tl.set_ticks(ModelRc::from(Rc::clone(&editor.ticks)));
        tl.set_blocks(ModelRc::from(Rc::clone(&editor.blocks)));
        window
            .global::<Sections<'_>>()
            .set_rows(ModelRc::from(Rc::clone(&editor.rows)));
        actions::connect(&editor, window);
        timeline_input::connect(&editor, window);
        sections_input::connect(&editor, window);
        let this = Rc::downgrade(&editor);
        player::set_view_hook(move |view| {
            if let Some(editor) = this.upgrade() {
                editor.playback_changed(view);
            }
        });
        editor.publish_menu_keys(false);
        editor.publish();
        editor
    }

    /// Another video was opened: its sections start empty.
    pub fn file_opened(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.sections.clear();
            st.viewport.set_duration(Timestamp::ZERO);
            st.drag = None;
        }
        self.publish();
    }

    /// Escape asks `hook` first (the countdown after the queue is done).
    pub fn set_escape_hook(&self, hook: impl Fn() -> bool + 'static) {
        *self.escape.borrow_mut() = Some(Box::new(hook));
    }

    /// Connects the Tag & Rate panel: Escape cancels the editing of a job's tags.
    pub fn set_tags(&self, tags: &Rc<TagsBridge>) {
        *self.tags.borrow_mut() = Rc::downgrade(tags);
    }

    /// Connects the probe render (its key, Escape while it plays).
    pub fn set_probe(&self, probe: &Rc<ProbeBridge>) {
        *self.probe.borrow_mut() = Rc::downgrade(probe);
    }

    /// Connects the export panel (the Save key).
    pub fn set_export(&self, export: &Rc<ExportBridge>) {
        *self.export.borrow_mut() = Rc::downgrade(export);
    }

    /// Connects the queue panel (the queue keys).
    pub fn set_queue(&self, queue: &Rc<QueueBridge>) {
        *self.queue.borrow_mut() = Rc::downgrade(queue);
    }

    /// The sections a saved job gets: the list, or with an empty list the automatic section of `auto` (PyCutRv's
    /// auto section): the whole video, or the range between the marks.
    pub fn sections_for_save(&self, auto: AutoSection) -> Result<SectionList, &'static str> {
        let st = self.state.borrow();
        if !st.sections.list().is_empty() {
            return Ok(st.sections.list().clone());
        }
        let section = match auto {
            AutoSection::Off => {
                return Err("Add a section first (or switch on the automatic section in the settings).");
            }
            AutoSection::WholeVideo => st
                .sections
                .duration()
                .and_then(|d| Section::new(Timestamp::ZERO, d).ok())
                .ok_or("The video's length is not known yet.")?,
            AutoSection::CurrentRange => st
                .sections
                .current()
                .map_err(|_none| "Add a section first, or set both marks for the automatic section.")?,
        };
        Ok(vec![section].into())
    }

    /// The sections and marks of the draft (set aside while a job is edited).
    pub fn sections_draft(&self) -> SectionsDraft {
        self.state.borrow().sections.draft()
    }

    /// Puts a draft's sections back (after its video opened again).
    pub fn put_sections(&self, draft: SectionsDraft) {
        self.state.borrow_mut().sections.put_draft(draft);
        self.publish();
    }

    /// The sections of a job loaded into the editor (after its source opened).
    pub fn load_sections(&self, list: SectionList) {
        self.state.borrow_mut().sections.load(list);
        self.publish();
    }

    /// Whether the draft holds sections or marks that loading a job would replace.
    pub fn has_work(&self) -> bool {
        self.state.borrow().sections.has_work()
    }

    /// After a save with "clear sections": the list goes, the marks stay (PyCutRv).
    pub fn clear_sections(&self) {
        self.state.borrow_mut().sections.clear_list();
        self.publish();
    }

    /// The player shows the probe render (`true`) or the source again. While the probe plays the timeline keeps
    /// showing the source and takes no input; edits of marks and sections are refused.
    pub fn set_probe_playing(&self, playing: bool) {
        self.probe_playing.set(playing);
        if let Some(window) = self.window.upgrade() {
            window.global::<Timeline<'_>>().set_locked(playing);
        }
        if !playing {
            self.publish();
        }
    }

    /// Whether an edit must wait because the probe render plays (says so in the status line).
    fn probe_blocks(&self) -> bool {
        if self.probe_playing.get() {
            self.status(Level::Info, "The probe render is playing: Esc returns to the source.");
        }
        self.probe_playing.get()
    }

    /// Where a probe render starts from: the playhead, the source's length and the job's length (the sections, else
    /// the whole source). `None` without a video of known length.
    pub fn probe_point(&self) -> Option<(Timestamp, Timestamp, Timestamp)> {
        let st = self.state.borrow();
        let duration = st.sections.duration().filter(|_| st.has_video)?;
        let total = st.sections.list().total_duration();
        let job = if total.as_micros() > 0 { total } else { duration };
        Some((st.playhead, duration, job))
    }

    /// The keymap and the seek steps changed in the Settings window: they hold from the next key press on.
    pub fn apply_settings(&self, settings: &Settings) {
        let (keymap, notes) = Keymap::from_table(&settings.keymap);
        for note in &notes {
            tracing::warn!(note = %note, "[Keymap] Repaired");
        }
        *self.keymap.borrow_mut() = keymap;
        self.seek_step.set(f64::from(settings.editor.seek_step_secs));
        self.seek_step_long.set(f64::from(settings.editor.seek_step_long_secs));
        self.publish_menu_keys(false);
    }

    /// The keys of `action` as the tooltips show them ("" when unbound).
    pub fn key_label(&self, action: Action) -> String {
        self.keymap.borrow().tooltip_keys(action)
    }

    /// The zoom to remember in `ui-state.toml`.
    pub fn timeline_state(&self) -> TimelineState {
        TimelineState {
            zoom: self.state.borrow().viewport.wanted_zoom(),
        }
    }

    /// A new playback view (UI thread): the duration, the playhead, and the view pages to follow a moving playhead.
    fn playback_changed(&self, view: &PlaybackView) {
        if self.probe_playing.get() {
            // The probe's own time and length are not the source's.
            return;
        }
        let Ok(mut st) = self.state.try_borrow_mut() else {
            return;
        };
        let duration = view.duration.filter(|d| d.as_micros() > 0);
        let mut layout_changed = view.has_video != st.has_video;
        st.has_video = view.has_video;
        if duration != st.sections.duration() {
            st.sections.set_duration(duration);
            st.viewport.set_duration(duration.unwrap_or(Timestamp::ZERO));
            layout_changed = true;
        }
        if view.position != st.playhead {
            st.playhead = view.position;
            if st.drag.is_none() && st.viewport.reveal(view.position) {
                layout_changed = true;
            }
        }
        drop(st);
        if layout_changed {
            self.publish();
        } else {
            self.publish_playhead();
        }
    }

    /// The frame grid of the open video (for snapping and the shortest section), when it has a plausible constant rate.
    fn frame_rate(&self) -> Option<FrameRate> {
        let info = self.media.upgrade()?.current()?;
        let (_, video) = info.primary_video()?;
        [video.avg_frame_rate, video.frame_rate]
            .into_iter()
            .flatten()
            .find(|r| PLAUSIBLE_FPS.contains(&r.as_f64()))
    }

    fn status(&self, level: Level, message: &str) {
        self.warned.set(level == Level::Warning);
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }

    /// An edit was accepted: a warning about an earlier refused one is no longer true.
    fn edit_accepted(&self) {
        if self.warned.get() {
            self.status(Level::Info, "Ready");
        }
    }

    /// Runs `f` with the player when it exists.
    fn with_player(&self, f: impl FnOnce(&Rc<PlayerBridge>)) {
        if let Some(player) = self.player.upgrade() {
            f(&player);
        }
    }

    /// The menu items' shortcuts; plain keys are left off while a text field has the keyboard.
    fn publish_menu_keys(&self, editing: bool) {
        let Some(window) = self.window.upgrade() else { return };
        let sc = window.global::<Shortcuts<'_>>();
        let keymap = self.keymap.borrow();
        let keys = |action| keymap.menu_keys(action, editing);
        sc.set_open(keys(Action::Open));
        sc.set_save(keys(Action::Save));
        sc.set_quit(keys(Action::Quit));
        sc.set_settings(keys(Action::OpenSettings));
        sc.set_toggle_pause(keys(Action::TogglePause));
        sc.set_frame_back(keys(Action::FrameBack));
        sc.set_frame_forward(keys(Action::FrameForward));
        sc.set_toggle_mute(keys(Action::ToggleMute));
        sc.set_reload(keys(Action::Reload));
        sc.set_set_start(keys(Action::SetStart));
        sc.set_set_end(keys(Action::SetEnd));
        sc.set_go_to_start(keys(Action::GoToStart));
        sc.set_go_to_end(keys(Action::GoToEnd));
        sc.set_add_section(keys(Action::AddSection));
        sc.set_previous_cut(keys(Action::PreviousCut));
        sc.set_next_cut(keys(Action::NextCut));
        sc.set_zoom_in(keys(Action::ZoomIn));
        sc.set_zoom_out(keys(Action::ZoomOut));
        sc.set_zoom_fit(keys(Action::ZoomFit));
        sc.set_filter_preview(keys(Action::ToggleFilterPreview));
        sc.set_filter_keep(keys(Action::ToggleKeepFilters));
        sc.set_filter_reset(keys(Action::ResetFilters));
        sc.set_probe_render(keys(Action::ProbeRender));
        sc.set_crop_edit(keys(Action::CropEdit));
        sc.set_auto_crop(keys(Action::AutoCrop));
        sc.set_rotate_left(keys(Action::RotateLeft));
        sc.set_rotate_right(keys(Action::RotateRight));
        sc.set_mirror_horizontal(keys(Action::MirrorHorizontal));
        sc.set_mirror_vertical(keys(Action::MirrorVertical));
        let label = |action| keymap.tooltip_keys(action);
        window.global::<Export<'_>>().set_key_save(label(Action::Save).into());
        let playback = window.global::<Playback<'_>>();
        playback.set_key_toggle_pause(label(Action::TogglePause).into());
        playback.set_key_frame_back(label(Action::FrameBack).into());
        playback.set_key_frame_forward(label(Action::FrameForward).into());
        playback.set_key_toggle_mute(label(Action::ToggleMute).into());
        let sections = window.global::<Sections<'_>>();
        sections.set_key_set_start(label(Action::SetStart).into());
        sections.set_key_set_end(label(Action::SetEnd).into());
        sections.set_key_add(label(Action::AddSection).into());
        sections.set_key_delete(label(Action::DeleteSection).into());
        if let Some(filters) = self.filters.upgrade() {
            filters.show_keys(&FilterKeys {
                preview: label(Action::ToggleFilterPreview),
                keep: label(Action::ToggleKeepFilters),
                reset: label(Action::ResetFilters),
                crop_edit: label(Action::CropEdit),
                auto_crop: label(Action::AutoCrop),
                rotate_left: label(Action::RotateLeft),
                rotate_right: label(Action::RotateRight),
                mirror_horizontal: label(Action::MirrorHorizontal),
                mirror_vertical: label(Action::MirrorVertical),
            });
        }
    }

    /// Shows the whole state: timeline and Sections card.
    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let tv = if st.has_video {
            view::build(&st.viewport, &st.sections)
        } else {
            view::TimelineView::default()
        };
        let tl = window.global::<Timeline<'_>>();
        tl.set_active(tv.active);
        sync(
            &self.ticks,
            tv.ticks
                .iter()
                .map(|t| TimelineTick {
                    x: t.x,
                    label: t.label.as_deref().unwrap_or_default().into(),
                })
                .collect(),
        );
        sync(
            &self.blocks,
            tv.blocks
                .iter()
                .map(|b| TimelineBlock {
                    x: b.x,
                    width: b.width,
                    label: b.label.as_str().into(),
                    selected: b.selected,
                    overlapping: b.overlapping,
                })
                .collect(),
        );
        tl.set_mark_start_shown(tv.mark_start.is_some());
        tl.set_mark_start_x(tv.mark_start.unwrap_or_default());
        tl.set_mark_end_shown(tv.mark_end.is_some());
        tl.set_mark_end_x(tv.mark_end.unwrap_or_default());
        tl.set_range_shown(tv.range.is_some());
        let (range_x, range_width) = tv.range.unwrap_or_default();
        tl.set_range_x(range_x);
        tl.set_range_width(range_width);
        tl.set_zoomed(tv.zoomed);
        tl.set_window_start(tv.window.0);
        tl.set_window_width(tv.window.1);
        drop(tl);
        self.publish_sections(&window, &st);
        drop(st);
        self.publish_playhead();
        self.publish_hover();
    }

    fn publish_playhead(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let tl = window.global::<Timeline<'_>>();
        let x = st
            .has_video
            .then(|| view::visible_x(&st.viewport, st.playhead))
            .flatten();
        tl.set_playhead_shown(x.is_some());
        tl.set_playhead_x(x.unwrap_or_default());
        tl.set_overview_playhead(st.viewport.fraction_of(st.playhead));
    }

    fn publish_hover(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let tl = window.global::<Timeline<'_>>();
        let hover = st
            .hover
            .filter(|_| st.has_video)
            .and_then(|x| view::hover(&st.viewport, x));
        tl.set_hover_shown(hover.is_some());
        if let Some((x, text)) = hover {
            tl.set_hover_x(x);
            tl.set_hover_text(text.into());
        }
        tl.set_edge_hovered(st.edge_hovered);
    }

    fn publish_sections(&self, window: &MainWindow, st: &State) {
        // The export card checks the job again (copied audio needs one section, Q-N6).
        if let Some(export) = self.export.borrow().upgrade() {
            export.job_changed();
        }
        let sections = window.global::<Sections<'_>>();
        let s = &st.sections;
        let text = |t: Option<Timestamp>| t.map(|t| t.to_string()).unwrap_or_default();
        sections.set_has_media(st.has_video);
        sections.set_mark_start(text(s.mark(Bound::Start)).into());
        sections.set_mark_end(text(s.mark(Bound::End)).into());
        let current = s.current().ok();
        sections.set_current_length(
            current
                .map(|c| format!("Length {}", c.duration()))
                .unwrap_or_default()
                .into(),
        );
        sections.set_can_add(st.has_video && current.is_some());
        let rows = s.rows();
        let selected = rows.iter().position(|r| r.selected);
        sections.set_can_move_up(selected.is_some_and(|i| i > 0));
        sections.set_can_move_down(selected.is_some_and(|i| i + 1 < rows.len()));
        sections.set_can_delete(selected.is_some());
        sections.set_summary(summary(s).into());
        sections.set_note(
            if rows.iter().any(|r| r.overlapping) {
                "Overlapping sections are rendered twice."
            } else {
                ""
            }
            .into(),
        );
        sync(
            &self.rows,
            rows.iter()
                .map(|r| SectionRowView {
                    number: r.number.checked_as::<i32>().unwrap_or(i32::MAX),
                    start: r.start.to_string().into(),
                    end: r.end.to_string().into(),
                    duration: (r.end - r.start).to_string().into(),
                    selected: r.selected,
                    overlapping: r.overlapping,
                })
                .collect(),
        );
    }
}

/// "2 sections, 0:00:25.000 in total" (empty without sections).
fn summary(sections: &SectionsState) -> String {
    match sections.list().len() {
        0 => String::new(),
        1 => format!("1 section, {}", sections.list().total_duration()),
        n => format!("{n} sections, {} in total", sections.list().total_duration()),
    }
}
