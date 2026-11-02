//! The job card of the queue row the pointer rests on (P12 block 10, Q-P12h; `ui/job_card.slint`). The rows report the
//! pointer, [`hover::Hover`] decides when the card shows, [`facts::facts`] what it says and [`pills::place`] where the
//! tags go. The data a summary row lacks comes from the queue off the UI thread: the job's spec (tags, rating, filters,
//! the source's size) is read whenever the card shows a job or its row changed, and its own picture
//! (`QueueCommand::Picture`, made when the job was saved) once per job while RustCut runs. While the card shows, every
//! refresh of the queue (10 Hz while rendering) shows it anew, so it follows the job live.

mod facts;
mod hover;
mod pills;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use rustcut_core::edit::FilterKind;
use rustcut_core::job::{JobId, JobSpec};
use rustcut_core::queue::{JobSummary, QueueCommand};
use rustcut_core::thumbs::{ThumbView, render};
use slint::{ComponentHandle as _, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};

use self::facts::{Facts, Input, Tone};
use self::hover::{Hover, Step};
use super::{BRIDGE, QueueBridge};
use crate::icons::Icon;
use crate::{JobCard, JobCardPill, JobCardRow, JobCardView, MainWindow, TooltipSettings};

/// Pictures kept decoded (a few KiB each).
const PICTURES: usize = 256;
/// The card's content width less the rating's box and gap, and with the "No rating" hint before the pills instead.
const ROOM_FULL: f32 = 282.0;
const ROOM_HINT: f32 = 220.0;

enum Picture {
    Loading,
    Ready(Image),
    Missing,
}

/// The card's state on the UI thread.
#[derive(Default)]
pub(super) struct Card {
    hover: RefCell<Hover>,
    timer: slint::Timer,
    /// The shown job's spec as last read (`None` inside while it is read or unreadable).
    spec: RefCell<Option<(JobId, Option<Box<JobSpec>>)>>,
    /// Counts the spec reads: an older answer is dropped.
    reads: Cell<u64>,
    /// The shown job's row when its spec was read (a changed row reads it again: tags and rating may have changed).
    read_for: RefCell<Option<JobSummary>>,
    pictures: RefCell<HashMap<JobId, Picture>>,
    /// The pills' widths Slint measured, by text.
    widths: RefCell<HashMap<String, f32>>,
    widths_waiting: Cell<bool>,
}

fn with(run: impl FnOnce(&QueueBridge)) {
    BRIDGE.with(|b| {
        if let Some(bridge) = b.borrow().upgrade() {
            run(&bridge);
        }
    });
}

/// Answers the card's callbacks.
pub(super) fn connect(window: &MainWindow) {
    let card = window.global::<JobCard<'_>>();
    card.on_hovered(|id, entered| with(|bridge| bridge.card_hovered(JobId(i64::from(id)), entered)));
    card.on_pressed(|| with(QueueBridge::card_pressed));
    card.on_pill_measured(|text, width| with(|bridge| bridge.card_pill_measured(&text, width)));
    card.on_place(|anchor, list, width, height, area_width, area_height| {
        crate::ui::tooltip::place_beside_row(&anchor, &list, (width, height), (area_width, area_height))
    });
}

impl QueueBridge {
    fn card_delay(&self) -> Duration {
        self.window.upgrade().map_or(Duration::from_millis(600), |w| {
            let ms = w.global::<TooltipSettings<'_>>().get_delay();
            Duration::from_millis(u64::try_from(ms).unwrap_or(0))
        })
    }

    fn card_hovered(&self, id: JobId, entered: bool) {
        let now = Instant::now();
        let step = if entered {
            self.card.hover.borrow_mut().enter(id, now, self.card_delay())
        } else {
            self.card.hover.borrow_mut().leave(id, now)
        };
        self.card_apply(step);
    }

    fn card_pressed(&self) {
        let step = self.card.hover.borrow_mut().press();
        self.card_apply(step);
    }

    fn card_due(&self) {
        let step = self.card.hover.borrow_mut().due(Instant::now(), self.card_delay());
        self.card_apply(step);
    }

    fn card_apply(&self, step: Step) {
        if let Some(wake) = step.wake {
            self.card
                .timer
                .start(slint::TimerMode::SingleShot, wake, || with(QueueBridge::card_due));
        }
        if !step.changed {
            return;
        }
        match step.shown {
            Some(id) => {
                self.card_read(id);
                self.card_publish();
            }
            None => {
                if let Some(window) = self.window.upgrade() {
                    window.global::<JobCard<'_>>().set_shown(false);
                }
            }
        }
    }

    /// After every refresh of the queue: the shown card follows its job (or closes when its row went away).
    pub(super) fn card_refresh(&self) {
        let Some(id) = self.card.hover.borrow().shown() else {
            return;
        };
        if !self.ui.borrow().visible.contains(&id) {
            let step = self.card.hover.borrow_mut().forget(id);
            self.card_apply(step);
            return;
        }
        let row = self.shared.lock().view.row(id).cloned();
        if row != *self.card.read_for.borrow() {
            self.card_read(id);
        }
        self.card_publish();
        // Pictures of jobs that went away are dropped.
        let shared = self.shared.lock();
        self.card
            .pictures
            .borrow_mut()
            .retain(|id, _| shared.view.row(*id).is_some());
    }

    /// Reads the job's spec, and its picture once.
    fn card_read(&self, id: JobId) {
        *self.card.read_for.borrow_mut() = self.shared.lock().view.row(id).cloned();
        let serial = self.card.reads.get().wrapping_add(1);
        self.card.reads.set(serial);
        if self.card.spec.borrow().as_ref().is_none_or(|(shown, _)| *shown != id) {
            *self.card.spec.borrow_mut() = Some((id, None));
        }
        let (handle, ui) = (self.handle.clone(), self.dispatcher.clone());
        self.runtime.spawn(async move {
            let spec = handle
                .request(|reply| QueueCommand::Spec { id, reply })
                .await
                .ok()
                .and_then(Result::ok);
            ui.post(move || with(|bridge| bridge.card_spec_read(serial, id, spec)));
        });
        if self.card.pictures.borrow().contains_key(&id) {
            return;
        }
        {
            let mut pictures = self.card.pictures.borrow_mut();
            if pictures.len() >= PICTURES {
                pictures.retain(|_, p| matches!(p, Picture::Loading));
            }
            pictures.insert(id, Picture::Loading);
        }
        let (handle, ui) = (self.handle.clone(), self.dispatcher.clone());
        self.runtime.spawn(async move {
            let picture = handle
                .request(|reply| QueueCommand::Picture { id, reply })
                .await
                .ok()
                .flatten();
            let pixels = picture.and_then(|p| {
                let rgb = render(&p.webp, &ThumbView::default())?;
                let (width, height) = rgb.dimensions();
                Some(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                    rgb.as_raw(),
                    width,
                    height,
                ))
            });
            ui.post(move || with(|bridge| bridge.card_picture_read(id, pixels)));
        });
    }

    fn card_spec_read(&self, serial: u64, id: JobId, spec: Option<Box<JobSpec>>) {
        if serial != self.card.reads.get() {
            return;
        }
        *self.card.spec.borrow_mut() = Some((id, spec));
        if self.card.hover.borrow().shown() == Some(id) {
            self.card_publish();
        }
    }

    fn card_picture_read(&self, id: JobId, pixels: Option<SharedPixelBuffer<Rgb8Pixel>>) {
        let picture = pixels.map_or(Picture::Missing, |p| Picture::Ready(Image::from_rgb8(p)));
        let mut pictures = self.card.pictures.borrow_mut();
        if !pictures.contains_key(&id) {
            return;
        }
        pictures.insert(id, picture);
        drop(pictures);
        if self.card.hover.borrow().shown() == Some(id) {
            self.card_publish();
        }
    }

    /// Slint measured a pill: the pills are placed again when that changes anything. Later, not here: Slint measures
    /// while it makes the pills, and a view changed at that moment keeps its old values.
    fn card_pill_measured(&self, text: &str, width: f32) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        let old = self.card.widths.borrow_mut().insert(text.to_owned(), width);
        if old.is_none_or(|w| (w - width).abs() > f32::EPSILON) && !self.card.widths_waiting.replace(true) {
            slint::Timer::single_shot(Duration::ZERO, || {
                with(|bridge| {
                    bridge.card.widths_waiting.set(false);
                    bridge.card_publish();
                });
            });
        }
    }

    /// Shows the card of the job it stands for now.
    fn card_publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let Some(id) = self.card.hover.borrow().shown() else {
            return;
        };
        let index = self.ui.borrow().visible.iter().position(|v| *v == id);
        let (Some(index), Some((row, progress, size, in_line))) = (index, {
            let shared = self.shared.lock();
            shared.view.row(id).map(|row| {
                (
                    row.clone(),
                    shared.view.progress.get(&id).copied(),
                    shared.view.size(id),
                    facts::in_line(&shared.view.rows, id),
                )
            })
        }) else {
            return;
        };
        let hooks = self.hooks.borrow();
        let no_names = |_: u32| None;
        let tag_name: &dyn Fn(u32) -> Option<String> = hooks.tag_name.as_deref().unwrap_or(&no_names);
        let spec = self.card.spec.borrow();
        let spec = spec
            .as_ref()
            .filter(|(shown, _)| *shown == id)
            .and_then(|(_, s)| s.as_deref());
        let facts = facts::facts(&Input {
            row: &row,
            progress: progress.as_ref(),
            size,
            spec,
            tag_name,
            in_line,
            now_ms: chrono::Utc::now().timestamp_millis(),
        });
        let view = self.card_view(id, &facts);
        let card = window.global::<JobCard<'_>>();
        card.set_index(i32::try_from(index).unwrap_or(0));
        card.set_view(view);
        card.set_shown(true);
    }

    fn card_view(&self, id: JobId, f: &Facts) -> JobCardView {
        let room = if f.rating.is_some() {
            pills::ROOM
        } else if f.hint.is_empty() {
            ROOM_FULL
        } else {
            ROOM_HINT
        };
        let placed = pills::place(&f.tags, &self.card.widths.borrow(), room);
        let (picture, has_picture, no_picture) = match self.card.pictures.borrow().get(&id) {
            Some(Picture::Ready(image)) => (image.clone(), true, false),
            Some(Picture::Missing) => (Image::default(), false, true),
            Some(Picture::Loading) | None => (Image::default(), false, false),
        };
        let progress = f.progress.as_ref();
        JobCardView {
            id: i32::try_from(id.0).unwrap_or(i32::MAX),
            name: f.name.as_str().into(),
            state: f.state.as_str().into(),
            kind: super::kind_number(f.kind),
            picture,
            has_picture,
            no_picture,
            rating: f.rating.map_or(-1, i32::from),
            hint: f.hint.as_str().into(),
            pills_height: pills::height(&placed),
            pills_width: room,
            pills: ModelRc::new(VecModel::from(
                placed
                    .iter()
                    .map(|p| JobCardPill {
                        text: p.text.as_str().into(),
                        x: p.x,
                        y: p.y,
                        width: p.width,
                        more: p.more,
                    })
                    .collect::<Vec<_>>(),
            )),
            has_progress: progress.is_some(),
            progress: progress.map_or(0.0, |p| p.fraction),
            progress_left: progress.map_or_else(Default::default, |p| p.left.as_str().into()),
            progress_right: progress.map_or_else(Default::default, |p| p.right.as_str().into()),
            error: f.error.as_deref().unwrap_or_default().into(),
            rows: ModelRc::new(VecModel::from(
                f.rows
                    .iter()
                    .map(|r| JobCardRow {
                        icon: r.icon.map_or(crate::IconId::None, Icon::to_slint),
                        text: r.text.as_str().into(),
                        right: r.right.as_str().into(),
                        tone: tone(r.tone),
                        path: r.path,
                        filters: r.filters,
                    })
                    .collect::<Vec<_>>(),
            )),
            filters: ModelRc::new(VecModel::from(
                f.filters.iter().map(|k| filter_icon(*k).to_slint()).collect::<Vec<_>>(),
            )),
            footer: f.footer.as_str().into(),
            note: f.note.as_deref().unwrap_or_default().into(),
        }
    }
}

const fn tone(tone: Tone) -> i32 {
    match tone {
        Tone::Normal => 0,
        Tone::Muted => 1,
        Tone::Error => 2,
        Tone::Accent => 3,
    }
}

const fn filter_icon(kind: FilterKind) -> Icon {
    match kind {
        FilterKind::Deinterlace => Icon::FilterDeinterlace,
        FilterKind::Crop => Icon::FilterCrop,
        FilterKind::Deshake => Icon::FilterDeshake,
        FilterKind::Orientation => Icon::FilterOrientation,
        FilterKind::Resize => Icon::FilterResize,
    }
}
