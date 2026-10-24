//! The pictures and descriptions of the conflict dialog (M5.4, user decisions Q-X6 and the requests that followed): the
//! existing target first, then every file of the name's series and the similar names, read by the core's
//! `PreviewService` on the runtime (`parallel_pictures` at a time, cached). Each result fills its tile in place as it
//! arrives; until then a tile shows a placeholder, and a file without a picture says why. Closing the dialog drops the
//! batch, which kills the running ffprobe and ffmpeg; a late result of an older dialog is ignored (generation).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use rustcut_core::media::{
    MediaInfo, Preview, PreviewBatch, PreviewEvent, PreviewRequest, PreviewSlot, format_bit_rate, format_frame_rate,
    format_size,
};
use rustcut_core::naming::ExistingFile;
use rustcut_core::settings::PictureScope;
use rustcut_core::time::Timestamp;
use slint::{ComponentHandle as _, Image, Model as _, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use super::ExportBridge;
use crate::{Export, PreviewView, TipRow};

/// What a tile shows (the numbers are `PreviewView.state` in Slint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TileState {
    Loading = 0,
    Picture = 1,
    /// Described, but without a picture (pictures off, audio only, ffmpeg failed): the note says why.
    NoPicture = 2,
    /// Gone or unreadable.
    Failed = 3,
}

/// What the dialog knows about one file.
#[derive(Clone, Default)]
pub(super) enum Slot {
    /// Asked, no answer yet.
    #[default]
    Waiting,
    Ready(Arc<Preview>, Option<Image>),
    Gone(String),
}

/// The texts of a tile or of the existing file's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TileText {
    pub state: TileState,
    /// "812 MB · 4:12 · 1920×1080".
    pub facts: String,
    /// "812 MB · 4:12".
    pub short: String,
    /// "2026-10-20 14:32".
    pub date: String,
    /// Why there is no picture, in two words for the picture (empty with a picture or while loading); the details
    /// tooltip has the whole reason as its "Problem" row.
    pub note: String,
    /// The details tooltip: label and value per row.
    pub rows: Vec<(String, String)>,
}

/// The texts for `slot`; `known` is what the folder check found before any preview (size and date of the target).
pub(super) fn tile_text(slot: &Slot, known: Option<ExistingFile>, pictures: bool) -> TileText {
    let mut text = TileText {
        state: TileState::Loading,
        facts: String::new(),
        short: String::new(),
        date: String::new(),
        note: String::new(),
        rows: Vec::new(),
    };
    if let Some(known) = known {
        text.short = format_size(known.bytes);
        text.facts.clone_from(&text.short);
        text.date = known.modified.map(date_text).unwrap_or_default();
    }
    match slot {
        Slot::Waiting => {}
        Slot::Gone(reason) => {
            text.state = TileState::Failed;
            "Not readable".clone_into(&mut text.note);
            text.rows.push(("Problem".to_owned(), reason.clone()));
        }
        Slot::Ready(preview, image) => {
            let info = preview.info.as_ref();
            let length = info.and_then(MediaInfo::duration).map(length_text);
            let size = info
                .and_then(|i| i.primary_video())
                .and_then(|(_, v)| v.oriented_size())
                .map(|(w, h)| format!("{w}×{h}"));
            let bytes = format_size(preview.bytes);
            text.short = [Some(bytes.clone()), length.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            text.facts = [Some(bytes.clone()), length.clone(), size]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            text.date = preview.modified.map(date_text).unwrap_or_default();
            text.rows = rows(preview, &bytes, &text.date, length);
            text.state = if image.is_some() {
                TileState::Picture
            } else {
                TileState::NoPicture
            };
            if image.is_none() {
                let (note, why) = no_picture_note(preview, pictures);
                note.clone_into(&mut text.note);
                if let Some(why) = why {
                    text.rows.push(("Problem".to_owned(), why));
                }
            }
        }
    }
    text
}

/// Why a described file has no picture: two words for the picture and, when there is more to say, the whole reason.
fn no_picture_note(preview: &Preview, pictures: bool) -> (&'static str, Option<String>) {
    let Some(info) = preview.info.as_ref() else {
        let why = preview.problem.as_deref().unwrap_or("ffprobe could not read it");
        return ("Not a video", Some(why.to_owned()));
    };
    if info.primary_video().is_none() {
        return (if info.has_audio() { "Audio only" } else { "No video" }, None);
    }
    if !pictures {
        return (
            "Pictures off",
            Some("Pictures are off for this file (Settings > Dialogs)".to_owned()),
        );
    }
    let why = preview.problem.as_deref().unwrap_or("ffmpeg wrote none");
    ("No picture", Some(why.to_owned()))
}

/// The details tooltip: modified, size, length, video, audio, bit rate.
fn rows(preview: &Preview, bytes: &str, date: &str, length: Option<String>) -> Vec<(String, String)> {
    let mut rows = vec![
        ("Modified".to_owned(), date.to_owned()),
        ("Size".to_owned(), bytes.to_owned()),
    ];
    rows.retain(|(_, value)| !value.is_empty());
    let Some(info) = preview.info.as_ref() else {
        return rows;
    };
    if let Some(length) = length {
        rows.push(("Length".to_owned(), length));
    }
    if let Some((stream, video)) = info.primary_video() {
        let parts: Vec<String> = [
            video.oriented_size().map(|(w, h)| format!("{w}×{h}")),
            stream.codec.as_deref().map(codec_label),
            video.avg_frame_rate.or(video.frame_rate).map(format_frame_rate),
        ]
        .into_iter()
        .flatten()
        .collect();
        rows.push(("Video".to_owned(), parts.join(" · ")));
    }
    let audio: Vec<_> = info.audio_streams().collect();
    let audio_text = match audio.as_slice() {
        [] => "None".to_owned(),
        [(stream, track)] => [
            stream.codec.as_deref().map(codec_label),
            track
                .channel_layout
                .clone()
                .or_else(|| track.channels.map(|n| format!("{n} channels"))),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
        [(first, _), ..] => format!(
            "{} tracks · {}",
            audio.len(),
            first.codec.as_deref().map_or_else(String::new, codec_label)
        ),
    };
    rows.push(("Audio".to_owned(), audio_text));
    if let Some(rate) = info.container.bit_rate {
        rows.push(("Bit rate".to_owned(), format_bit_rate(rate)));
    }
    rows
}

/// ffmpeg's short codec names as people know them (`h264` -> `H.264`).
pub(super) fn codec_label(codec: &str) -> String {
    match codec {
        "h264" => "H.264".to_owned(),
        "hevc" => "HEVC".to_owned(),
        "av1" => "AV1".to_owned(),
        "vp8" => "VP8".to_owned(),
        "vp9" => "VP9".to_owned(),
        "mpeg4" => "MPEG-4".to_owned(),
        "mpeg2video" => "MPEG-2".to_owned(),
        "mpeg1video" => "MPEG-1".to_owned(),
        "prores" => "ProRes".to_owned(),
        "aac" => "AAC".to_owned(),
        "opus" => "Opus".to_owned(),
        "mp3" => "MP3".to_owned(),
        "flac" => "FLAC".to_owned(),
        "vorbis" => "Vorbis".to_owned(),
        "ac3" => "AC-3".to_owned(),
        "eac3" => "E-AC-3".to_owned(),
        c if c.starts_with("pcm_") => "PCM".to_owned(),
        other => other.to_uppercase(),
    }
}

/// `4:12`, `1:02:03` (whole seconds).
pub(super) fn length_text(length: Timestamp) -> String {
    let secs = length.as_micros().max(0) / Timestamp::MICROS_PER_SECOND;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn date_text(time: std::time::SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = time.into();
    local.format("%Y-%m-%d %H:%M").to_string()
}

/// Where a file of the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Existing,
    Series(usize),
    Other(usize),
}

/// One listed file.
pub(super) struct Listed {
    pub name: String,
    /// The number in the series ("" for the other names).
    pub counter: String,
    /// The target file itself (its preview is the existing file's).
    pub target: bool,
    pub slot: Slot,
}

/// The previews of the open dialog.
#[derive(Default)]
pub(super) struct DialogPreviews {
    /// Counts dialogs; a result counts only for the dialog it was asked for.
    generation: u64,
    batch: Option<PreviewBatch>,
    folder: PathBuf,
    /// What `PreviewSlot::Rest(i)` is for.
    rest: Vec<Place>,
    existing: Option<(String, Option<ExistingFile>, Slot)>,
    series: Vec<Listed>,
    others: Vec<Listed>,
    /// The rows of the two "Similar files" groups, updated in place as the previews arrive.
    series_model: Rc<VecModel<PreviewView>>,
    others_model: Rc<VecModel<PreviewView>>,
    pictures: PictureScope,
    asked: usize,
    done: usize,
    started: Option<Instant>,
}

impl DialogPreviews {
    /// The path of a listed file (`group` 0 = the existing file, 1 = the series, 2 = the other names).
    pub(super) fn path(&self, group: i32, index: usize) -> Option<PathBuf> {
        let name = match group {
            0 => self.existing.as_ref().map(|(name, ..)| name.as_str()),
            1 => self.series.get(index).map(|l| l.name.as_str()),
            _ => self.others.get(index).map(|l| l.name.as_str()),
        }?;
        Some(self.folder.join(name))
    }

    fn view(&self, listed: &Listed, group: i32, index: usize) -> PreviewView {
        let slot = if listed.target {
            self.existing.as_ref().map_or(&listed.slot, |(_, _, slot)| slot)
        } else {
            &listed.slot
        };
        let pictures = self.pictures == PictureScope::All || listed.target && self.pictures != PictureScope::Off;
        PreviewView {
            group,
            index: i32::try_from(index).unwrap_or(i32::MAX),
            ..view(&listed.name, &listed.counter, listed.target, slot, None, pictures)
        }
    }

    /// The model and the row of a file.
    fn row(&self, place: Place) -> Option<(&Rc<VecModel<PreviewView>>, usize, PreviewView)> {
        match place {
            Place::Existing => None,
            Place::Series(i) => self.series.get(i).map(|l| (&self.series_model, i, self.view(l, 1, i))),
            Place::Other(i) => self.others.get(i).map(|l| (&self.others_model, i, self.view(l, 2, i))),
        }
    }

    fn existing_view(&self) -> PreviewView {
        self.existing
            .as_ref()
            .map_or_else(PreviewView::default, |(name, known, slot)| {
                view(name, "", true, slot, *known, self.pictures != PictureScope::Off)
            })
    }

    fn progress(&self) -> String {
        if self.done >= self.asked {
            String::new()
        } else {
            format!("Reading {} of {} files", self.done, self.asked)
        }
    }
}

fn view(
    name: &str,
    counter: &str,
    target: bool,
    slot: &Slot,
    known: Option<ExistingFile>,
    pictures: bool,
) -> PreviewView {
    let text = tile_text(slot, known, pictures);
    let picture = match slot {
        Slot::Ready(_, Some(image)) => image.clone(),
        _ => Image::default(),
    };
    let rows: Vec<TipRow> = text
        .rows
        .into_iter()
        .map(|(label, value)| TipRow {
            label: label.into(),
            value: value.into(),
        })
        .collect();
    PreviewView {
        group: 0,
        index: 0,
        name: name.into(),
        counter: counter.into(),
        is_target: target,
        state: text.state as i32,
        picture,
        facts: text.facts.into(),
        short: text.short.into(),
        date: text.date.into(),
        note: text.note.into(),
        tip_rows: ModelRc::new(VecModel::from(rows)),
    }
}

fn image_of(preview: &Preview) -> Option<Image> {
    let t = preview.thumbnail.as_ref()?;
    let expected = usize::try_from(u64::from(t.width) * u64::from(t.height) * 4).ok()?;
    (t.rgba.len() == expected).then(|| {
        Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &t.rgba, t.width, t.height,
        ))
    })
}

impl ExportBridge {
    /// The dialog opened: lists the series and the similar names and starts reading them, the existing file first.
    pub(super) fn start_previews(self: &Rc<Self>) {
        self.stop_previews();
        let settings = self.dialog_settings.borrow().clone();
        let (folder, file, exists, series, others) = {
            let pending = self.pending.borrow();
            let Some(dialog) = pending.as_ref().map(|p| &p.dialog) else {
                return;
            };
            (
                dialog.folder.clone(),
                dialog.file.clone(),
                dialog.conflicts.exists,
                dialog
                    .series
                    .iter()
                    .map(|f| (f.name.clone(), format!("{:02}", f.counter)))
                    .collect::<Vec<_>>(),
                dialog
                    .others
                    .iter()
                    .map(|n| (n.clone(), String::new()))
                    .collect::<Vec<_>>(),
            )
        };
        let pictures = settings.pictures;
        let first = exists.map(|_| PreviewRequest {
            path: folder.join(&file),
            picture: pictures != PictureScope::Off,
        });
        let mut rest_places = Vec::new();
        let mut rest = Vec::new();
        // The target's tile shows the existing file's preview; every other file is read once.
        let listed = |names: Vec<(String, String)>, series: bool, rest_places: &mut Vec<Place>, rest: &mut Vec<_>| {
            names
                .into_iter()
                .enumerate()
                .map(|(i, (name, counter))| {
                    let target = name == file && exists.is_some();
                    if !target {
                        rest_places.push(if series { Place::Series(i) } else { Place::Other(i) });
                        rest.push(PreviewRequest {
                            path: folder.join(&name),
                            picture: pictures == PictureScope::All,
                        });
                    }
                    Listed {
                        counter,
                        target,
                        slot: Slot::Waiting,
                        name,
                    }
                })
                .collect::<Vec<_>>()
        };
        let series = listed(series, true, &mut rest_places, &mut rest);
        let others = listed(others, false, &mut rest_places, &mut rest);
        let asked = rest.len() + usize::from(first.is_some());
        tracing::info!(files = asked, "[Export] Conflict dialog: reading the previews");
        let (batch, mut events) = self.preview_service.batch(&self.runtime, first, rest);
        let generation = {
            let mut p = self.previews.borrow_mut();
            p.generation = p.generation.wrapping_add(1);
            p.batch = Some(batch);
            p.folder = folder;
            p.rest = rest_places;
            p.existing = exists.map(|known| (file.clone(), Some(known), Slot::Waiting));
            p.series = series;
            p.others = others;
            p.pictures = pictures;
            p.asked = asked;
            p.done = 0;
            p.started = Some(Instant::now());
            p.generation
        };
        self.publish_previews_all();
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            while let Some(event) = events.recv().await {
                let Some(bridge) = weak.upgrade() else { return };
                if !bridge.preview_arrived(generation, event) {
                    return;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; no previews");
        }
    }

    /// The dialog closed: the running reads stop (their programs are killed); late results are ignored.
    pub(super) fn stop_previews(&self) {
        let mut p = self.previews.borrow_mut();
        if p.batch.take().is_some() && p.done < p.asked {
            tracing::info!(
                done = p.done,
                asked = p.asked,
                "[Export] Conflict dialog closed; previews stopped"
            );
        }
        p.generation = p.generation.wrapping_add(1);
        p.series.clear();
        p.others.clear();
        p.existing = None;
        p.rest.clear();
    }

    /// One result; `false` when it belongs to a dialog that is gone (the listener stops).
    fn preview_arrived(&self, generation: u64, event: PreviewEvent) -> bool {
        let place = {
            let mut p = self.previews.borrow_mut();
            if p.generation != generation {
                return false;
            }
            p.done = p.done.saturating_add(1);
            if p.done == p.asked
                && let Some(started) = p.started
            {
                tracing::info!(
                    files = p.asked,
                    ms = started.elapsed().as_millis(),
                    "[Export] Previews read"
                );
            }
            let place = match event.slot {
                PreviewSlot::First => Place::Existing,
                PreviewSlot::Rest(i) => match p.rest.get(i) {
                    Some(place) => *place,
                    None => return true,
                },
            };
            let slot = match event.result {
                Ok(preview) => {
                    let image = image_of(&preview);
                    Slot::Ready(preview, image)
                }
                Err(e) => Slot::Gone(e.to_string()),
            };
            match place {
                Place::Existing => {
                    if let Some(existing) = p.existing.as_mut() {
                        existing.2 = slot;
                    }
                }
                Place::Series(i) => {
                    if let Some(l) = p.series.get_mut(i) {
                        l.slot = slot;
                    }
                }
                Place::Other(i) => {
                    if let Some(l) = p.others.get_mut(i) {
                        l.slot = slot;
                    }
                }
            }
            place
        };
        self.publish_preview(place);
        true
    }

    /// Every model and the existing file's card, when the dialog opens.
    fn publish_previews_all(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let e = window.global::<Export<'_>>();
        let p = self.previews.borrow();
        p.series_model.set_vec(
            p.series
                .iter()
                .enumerate()
                .map(|(i, l)| p.view(l, 1, i))
                .collect::<Vec<_>>(),
        );
        p.others_model.set_vec(
            p.others
                .iter()
                .enumerate()
                .map(|(i, l)| p.view(l, 2, i))
                .collect::<Vec<_>>(),
        );
        e.set_dialog_series(ModelRc::from(Rc::clone(&p.series_model)));
        e.set_dialog_others(ModelRc::from(Rc::clone(&p.others_model)));
        e.set_dialog_existing(p.existing_view());
        e.set_dialog_previews_progress(p.progress().into());
    }

    /// One file's row (and the series row of the target with the existing file), in place.
    fn publish_preview(&self, place: Place) {
        let Some(window) = self.window.upgrade() else { return };
        let e = window.global::<Export<'_>>();
        let p = self.previews.borrow();
        let mut changed = vec![place];
        if place == Place::Existing {
            e.set_dialog_existing(p.existing_view());
            changed.extend(
                p.series
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.target)
                    .map(|(i, _)| Place::Series(i)),
            );
        }
        for place in changed {
            if let Some((model, at, row)) = p.row(place)
                && at < model.row_count()
            {
                model.set_row_data(at, row);
            }
        }
        e.set_dialog_previews_progress(p.progress().into());
    }

    /// A listed file (`group` 0 = the existing file, 1 = the series, 2 = the other names): played in the default
    /// player, or shown in the file manager (off the UI thread).
    pub(super) fn open_listed(&self, group: i32, index: i32, reveal: bool) {
        let Some(path) = self
            .previews
            .borrow()
            .path(group, usize::try_from(index).unwrap_or(usize::MAX))
        else {
            return;
        };
        self.runtime.spawn_blocking(move || {
            let result = if reveal {
                opener::reveal(&path)
            } else {
                opener::open(&path)
            };
            if let Err(e) = result {
                tracing::warn!(file = %path.display(), error = %e, "[Export] A file of the dialog could not be opened");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use rustcut_core::media::decode;

    use super::*;

    const VIDEO: &str = r#"{"streams":[
        {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"r_frame_rate":"25/1",
         "avg_frame_rate":"25/1"},
        {"index":1,"codec_type":"audio","codec_name":"aac","channels":2,"channel_layout":"stereo"}],
        "format":{"format_name":"mov","duration":"252.4","bit_rate":"4140000"}}"#;
    const AUDIO: &str = r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"opus","channels":2}],
        "format":{"format_name":"ogg","duration":"30"}}"#;

    fn preview(json: Option<&str>, problem: Option<&str>) -> Arc<Preview> {
        Arc::new(Preview {
            bytes: 812_000_000,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)),
            info: json.map(|j| decode(j.as_bytes()).unwrap()),
            thumbnail: None,
            problem: problem.map(str::to_owned),
        })
    }

    #[test]
    fn a_described_video_has_facts_and_details() {
        let slot = Slot::Ready(preview(Some(VIDEO), None), None);
        let t = tile_text(&slot, None, true);
        assert_eq!(t.facts, "812 MB · 4:12 · 1920×1080");
        assert_eq!(t.short, "812 MB · 4:12");
        assert!(!t.date.is_empty());
        let rows: Vec<(&str, &str)> = t.rows.iter().map(|(l, v)| (l.as_str(), v.as_str())).collect();
        assert_eq!(
            rows[1..rows.len() - 1],
            [
                ("Size", "812 MB"),
                ("Length", "4:12"),
                ("Video", "1920×1080 · H.264 · 25 fps"),
                ("Audio", "AAC · stereo"),
                ("Bit rate", "4.14 Mbit/s"),
            ]
        );
        assert_eq!(rows[0].0, "Modified");
        assert_eq!(t.state, TileState::NoPicture, "described, picture not there");
        assert_eq!(t.note, "No picture");
        assert_eq!(t.rows.last().map(|(l, _)| l.as_str()), Some("Problem"));
    }

    #[test]
    fn placeholders_say_why_there_is_no_picture() {
        let waiting = tile_text(
            &Slot::Waiting,
            Some(ExistingFile {
                bytes: 2_000_000,
                modified: None,
            }),
            true,
        );
        assert_eq!((waiting.state, waiting.short.as_str()), (TileState::Loading, "2 MB"));
        assert!(waiting.note.is_empty(), "loading needs no words");
        let audio = tile_text(&Slot::Ready(preview(Some(AUDIO), None), None), None, true);
        assert_eq!((audio.state, audio.note.as_str()), (TileState::NoPicture, "Audio only"));
        let off = tile_text(&Slot::Ready(preview(Some(VIDEO), None), None), None, false);
        assert_eq!(off.note, "Pictures off");
        assert!(
            off.rows
                .iter()
                .any(|(l, v)| l == "Problem" && v.contains("Settings > Dialogs"))
        );
        let broken = tile_text(
            &Slot::Ready(preview(None, Some("Invalid data found when processing input")), None),
            None,
            true,
        );
        assert_eq!(broken.note, "Not a video", "two words fit even a small picture");
        assert!(broken.rows.contains(&(
            "Problem".to_owned(),
            "Invalid data found when processing input".to_owned()
        )));
        assert!(broken.rows.iter().any(|(l, _)| l == "Size"), "size and date still help");
        let gone = tile_text(&Slot::Gone("No such file or directory".into()), None, true);
        assert_eq!(gone.state, TileState::Failed);
        assert_eq!(gone.note, "Not readable");
        assert!(gone.rows.iter().any(|(_, v)| v.contains("No such file")));
    }

    #[test]
    fn rows_know_their_group_and_index() {
        let listed = |name: &str| Listed {
            name: name.to_owned(),
            counter: String::new(),
            target: false,
            slot: Slot::Waiting,
        };
        let p = DialogPreviews {
            folder: PathBuf::from("/out"),
            series: vec![listed("Clip - 01.mkv"), listed("Clip - 02.mp4")],
            others: vec![listed("Clip.mkv")],
            ..DialogPreviews::default()
        };
        let (_, at, row) = p.row(Place::Series(1)).unwrap();
        assert_eq!(
            (at, row.group, row.index, row.name.as_str()),
            (1, 1, 1, "Clip - 02.mp4")
        );
        let (_, at, row) = p.row(Place::Other(0)).unwrap();
        assert_eq!((at, row.group, row.index), (0, 2, 0));
        assert!(p.row(Place::Other(1)).is_none());
        assert_eq!(p.path(1, 1), Some(PathBuf::from("/out/Clip - 02.mp4")));
        assert_eq!(p.path(2, 5), None);
    }

    #[test]
    fn lengths_and_codec_names() {
        assert_eq!(length_text(Timestamp::from_secs(59)), "0:59");
        assert_eq!(length_text(Timestamp::from_secs(3723)), "1:02:03");
        assert_eq!(length_text(Timestamp::from_micros(-1)), "0:00");
        assert_eq!(codec_label("hevc"), "HEVC");
        assert_eq!(codec_label("pcm_s16le"), "PCM");
        assert_eq!(codec_label("theora"), "THEORA");
    }
}
