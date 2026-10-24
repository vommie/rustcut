//! The pictures and descriptions of the conflict dialog (M5.4, user decisions Q-X6 and the requests that followed): the
//! existing target first, then every file of the name's series and the similar names, read by the core's
//! `PreviewService` on the runtime (`parallel_pictures` at a time, cached). Each result fills its tile in place as it
//! arrives; until then a tile shows a placeholder, and a file without a picture says why. Closing the dialog drops the
//! batch, which kills the running ffprobe and ffmpeg; a late result of an older dialog is ignored (generation).

mod tiles;

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use rustcut_core::media::{Preview, PreviewBatch, PreviewEvent, PreviewRequest, PreviewSlot};
use rustcut_core::naming::ExistingFile;
use rustcut_core::settings::PictureScope;
use slint::{ComponentHandle as _, Image, Model as _, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use self::tiles::{Slot, tile_text};
use super::ExportBridge;
use crate::{Export, PreviewView, TipRow};

/// Where a file of the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Existing,
    Series(usize),
    Other(usize),
    /// A file of the known-source dialog.
    Known(usize),
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
    /// The known-source dialog's files (absolute paths) and their previews.
    known: Vec<(super::dialog::KnownFile, Slot)>,
    /// The rows of the two "Similar files" groups and the known files, updated in place as the previews arrive.
    series_model: Rc<VecModel<PreviewView>>,
    others_model: Rc<VecModel<PreviewView>>,
    known_model: Rc<VecModel<PreviewView>>,
    pictures: PictureScope,
    asked: usize,
    done: usize,
    started: Option<Instant>,
}

impl DialogPreviews {
    /// The path of a listed file (`group` 0 = the existing file, 1 = the series, 2 = the other names).
    pub(super) fn path(&self, group: i32, index: usize) -> Option<PathBuf> {
        if group == 3 {
            return self.known.get(index).map(|(file, _)| file.path.clone());
        }
        let name = match group {
            0 => self.existing.as_ref().map(|(name, ..)| name.as_str()),
            1 => self.series.get(index).map(|l| l.name.as_str()),
            _ => self.others.get(index).map(|l| l.name.as_str()),
        }?;
        Some(self.folder.join(name))
    }

    fn known_view(&self, index: usize) -> Option<PreviewView> {
        let (file, slot) = self.known.get(index)?;
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = file.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        Some(PreviewView {
            group: 3,
            index: i32::try_from(index).unwrap_or(i32::MAX),
            folder: folder.into(),
            rating: file.rating.map_or(-1, i32::from),
            tags: file.tags.as_str().into(),
            missing: file.missing,
            job: file.job.as_str().into(),
            ..view(&name, "", false, slot, None, self.pictures != PictureScope::Off)
        })
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
            Place::Known(i) => self.known_view(i).map(|v| (&self.known_model, i, v)),
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
            heading: false,
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
        folder: slint::SharedString::new(),
        rating: -1,
        tags: slint::SharedString::new(),
        missing: false,
        job: slint::SharedString::new(),
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
        let (folder, file, exists, series, others, known) = {
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
                dialog.known.as_ref().map(|k| k.files.clone()).unwrap_or_default(),
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
        // The known files come first: they are what the known-source dialog is about.
        let known: Vec<(super::dialog::KnownFile, Slot)> = known
            .into_iter()
            .enumerate()
            .map(|(i, file)| {
                let slot = if file.missing {
                    Slot::Gone("The file is missing".to_owned())
                } else {
                    rest_places.push(Place::Known(i));
                    rest.push(PreviewRequest {
                        path: file.path.clone(),
                        picture: pictures != PictureScope::Off,
                    });
                    Slot::Waiting
                };
                (file, slot)
            })
            .collect();
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
            p.known = known;
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
        p.known.clear();
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
                Place::Known(i) => {
                    if let Some(k) = p.known.get_mut(i) {
                        k.1 = slot;
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
        p.known_model
            .set_vec((0..p.known.len()).filter_map(|i| p.known_view(i)).collect::<Vec<_>>());
        e.set_dialog_series(ModelRc::from(Rc::clone(&p.series_model)));
        e.set_dialog_others(ModelRc::from(Rc::clone(&p.others_model)));
        e.set_dialog_known(ModelRc::from(Rc::clone(&p.known_model)));
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

    /// A listed file (`group` 0 = the existing file, 1 = the series, 2 = the other names, 3 = a known file): played in the default
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
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use rustcut_core::media::decode;
    use rustcut_core::time::Timestamp;

    use super::tiles::{TileState, codec_label, length_text};
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
