//! Previews of media files (M5.4): what a file is (size, date, ffprobe's description) and one picture of it, made by
//! ffmpeg on the runtime. The conflict dialog shows them for the existing target and the similar names in its folder;
//! the timeline's filmstrip can use the same service later (HANDOFF section 13).
//!
//! A few files at a time (`PreviewOptions::parallel`, settings `dialogs.conflict.parallel_pictures`); a batch reads its
//! first file (the existing target) before the others. The picture is the keyframe at or before the chosen place,
//! decoded alone on one thread with its timestamp passed through (40-50 ms and 0.2-2 MB read per file here, where an
//! exact decode reads up to 6.5 MB), with an exact decode and then the file's start as fallbacks. ffprobe and ffmpeg run
//! at a low CPU and drive priority, so the player and the desktop never wait for them. Results are cached by path, size, modification
//! time and place (a changed file is read again); two requests for the same file share one read. Dropping a request
//! kills its ffprobe or ffmpeg.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use quick_cache::Weighter;
use quick_cache::sync::Cache;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};

use super::model::MediaInfo;
use super::probe::{ProbeOptions, probe, run_as};
use crate::time::Timestamp;

/// Width and height of a picture (16:9; a picture of another shape is fitted inside and the rest left transparent).
pub const THUMB_WIDTH: u32 = 256;
pub const THUMB_HEIGHT: u32 = 144;
/// Files read at the same time unless the options say otherwise.
pub const DEFAULT_PARALLEL: usize = 3;
/// How long ffprobe may describe a file and ffmpeg may take for its picture.
const PROBE_LIMIT: Duration = Duration::from_secs(10);
const PICTURE_LIMIT: Duration = Duration::from_secs(15);
/// What the cache may hold, counted in picture bytes (about 450 files).
const CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// The programs a preview runs (`None` = from `PATH`; settings `advanced.ffmpeg` and `advanced.ffprobe`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewPrograms {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

/// How previews are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewOptions {
    pub programs: PreviewPrograms,
    /// Where in a video the picture is taken, in percent of its length (the very first frame is often black).
    pub picture_at_percent: u8,
    /// Files read at the same time (at least 1).
    pub parallel: usize,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            programs: PreviewPrograms::default(),
            picture_at_percent: 10,
            parallel: DEFAULT_PARALLEL,
        }
    }
}

/// One picture: `width * height` pixels as RGBA, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What is known about a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    /// ffprobe's description (`None` when it could not read the file, see `problem`).
    pub info: Option<MediaInfo>,
    /// `None` for a file without a video picture or when ffmpeg failed.
    pub thumbnail: Option<Thumbnail>,
    /// Why the description or the picture is missing (ffprobe's or ffmpeg's words).
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    /// The path is not a readable file (gone, a folder, no access).
    #[error("{0}")]
    NotAFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    bytes: u64,
    modified: Option<SystemTime>,
    /// With a picture (at `picture_at_percent`) or only the description (then the place is 0).
    picture: bool,
    picture_at_percent: u8,
}

#[derive(Clone)]
struct PictureBytes;

impl Weighter<Key, Arc<Preview>> for PictureBytes {
    fn weight(&self, _key: &Key, value: &Arc<Preview>) -> u64 {
        // Every entry weighs a little, so descriptions without a picture are evicted too.
        let picture = value.thumbnail.as_ref().map_or(0, |t| t.rgba.len());
        u64::try_from(picture).unwrap_or(u64::MAX).saturating_add(4096)
    }
}

/// The service; clones share the cache and the turns.
#[derive(Clone)]
pub struct PreviewService {
    inner: Arc<Inner>,
}

struct Inner {
    cache: Cache<Key, Arc<Preview>, PictureBytes>,
    /// The options and the turns that go with them (a new number of turns starts a new semaphore; reads holding a
    /// turn of the old one finish on it).
    state: Mutex<(PreviewOptions, Arc<Semaphore>)>,
}

impl PreviewService {
    pub fn new(options: PreviewOptions) -> Self {
        let turns = Arc::new(Semaphore::new(options.parallel.max(1)));
        Self {
            inner: Arc::new(Inner {
                cache: Cache::with_weighter(256, CACHE_BYTES, PictureBytes),
                state: Mutex::new((options, turns)),
            }),
        }
    }

    /// Other options for the next previews (cached previews of another picture place are not used).
    pub fn set_options(&self, options: PreviewOptions) {
        let mut state = self.inner.state.lock();
        if state.0.parallel != options.parallel {
            state.1 = Arc::new(Semaphore::new(options.parallel.max(1)));
        }
        state.0 = options;
    }

    pub fn options(&self) -> PreviewOptions {
        self.inner.state.lock().0.clone()
    }

    /// The preview of `path` with its picture, from the cache while the file has the same size and modification time.
    ///
    /// # Errors
    /// [`PreviewError::NotAFile`] when `path` is not a readable file. A file ffprobe or ffmpeg cannot read is no error:
    /// its preview says so in `problem`.
    pub async fn preview(&self, path: &Path) -> Result<Arc<Preview>, PreviewError> {
        self.get(&PreviewRequest::picture(path)).await
    }

    /// What `request` asks for; a description-only request takes a cached preview with a picture when there is one.
    ///
    /// # Errors
    /// See [`Self::preview`].
    pub async fn get(&self, request: &PreviewRequest) -> Result<Arc<Preview>, PreviewError> {
        let path = request.path.as_path();
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| PreviewError::NotAFile(e.to_string()))?;
        if !metadata.is_file() {
            return Err(PreviewError::NotAFile("not a file".to_owned()));
        }
        let (options, turns) = {
            let state = self.inner.state.lock();
            (state.0.clone(), Arc::clone(&state.1))
        };
        let key = Key {
            path: path.to_path_buf(),
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
            picture: request.picture,
            picture_at_percent: if request.picture { options.picture_at_percent } else { 0 },
        };
        if !key.picture {
            let pictured = Key {
                picture: true,
                picture_at_percent: options.picture_at_percent,
                ..key.clone()
            };
            if let Some(cached) = self.inner.cache.get(&pictured) {
                return Ok(cached);
            }
        }
        let guard = match self.inner.cache.get_value_or_guard_async(&key).await {
            Ok(cached) => return Ok(cached),
            Err(guard) => guard,
        };
        let preview = {
            // The semaphore is never closed; without a turn the file would be read anyway.
            let _turn = turns.acquire().await.ok();
            let started = std::time::Instant::now();
            let preview = read(path, &key, &options).await;
            tracing::debug!(
                path = %path.display(),
                ms = started.elapsed().as_millis(),
                picture = preview.thumbnail.is_some(),
                "[Media] Preview read"
            );
            Arc::new(preview)
        };
        // Another request may have filled the place meanwhile (the guard was dropped by a canceled request).
        let _ = guard.insert(Arc::clone(&preview));
        Ok(preview)
    }
}

/// Which file of a batch a result is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSlot {
    First,
    /// The index in `rest`.
    Rest(usize),
}

#[derive(Debug, Clone)]
pub struct PreviewEvent {
    pub slot: PreviewSlot,
    pub result: Result<Arc<Preview>, PreviewError>,
}

/// A batch of previews on the runtime (see [`PreviewService::batch`]). Dropping it stops every read that has not
/// finished and kills its ffprobe or ffmpeg; results already sent stay in the receiver.
#[derive(Debug)]
pub struct PreviewBatch {
    task: JoinHandle<()>,
}

impl PreviewBatch {
    /// The batch sent every result (or was stopped).
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for PreviewBatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One file of a batch, with its picture or only its description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub picture: bool,
}

impl PreviewRequest {
    pub fn picture(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            picture: true,
        }
    }

    pub fn describe(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            picture: false,
        }
    }
}

impl PreviewService {
    /// Reads `first` before anything else (the conflict dialog's existing target), then the files of `rest` (in their
    /// order, `parallel` at a time). Every result arrives on the receiver as soon as it is there; the batch ends
    /// when all are sent or the receiver is dropped.
    pub fn batch(
        &self,
        runtime: &tokio::runtime::Handle,
        first: Option<PreviewRequest>,
        rest: Vec<PreviewRequest>,
    ) -> (PreviewBatch, mpsc::UnboundedReceiver<PreviewEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let service = self.clone();
        let task = runtime.spawn(async move {
            if let Some(request) = first {
                let result = service.get(&request).await;
                if tx
                    .send(PreviewEvent {
                        slot: PreviewSlot::First,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            // Dropping the set (the batch was stopped) aborts every read in it.
            let mut reads = JoinSet::new();
            for (index, request) in rest.into_iter().enumerate() {
                let service = service.clone();
                reads.spawn(async move { (index, service.get(&request).await) });
            }
            while let Some(joined) = reads.join_next().await {
                let Ok((index, result)) = joined else { continue };
                if tx
                    .send(PreviewEvent {
                        slot: PreviewSlot::Rest(index),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
        });
        (PreviewBatch { task }, rx)
    }
}

/// Reads the description and the picture of one file.
async fn read(path: &Path, key: &Key, options: &PreviewOptions) -> Preview {
    let programs = &options.programs;
    let probe_options = ProbeOptions {
        program: programs.ffprobe.clone(),
        timeout: PROBE_LIMIT,
        background: true,
        ..ProbeOptions::default()
    };
    let mut preview = Preview {
        bytes: key.bytes,
        modified: key.modified,
        info: None,
        thumbnail: None,
        problem: None,
    };
    match probe(path, &probe_options).await {
        Ok(info) => preview.info = Some(info),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "[Media] No description for the preview");
            preview.problem = Some(e.to_string());
            return preview;
        }
    }
    let Some(info) = preview.info.as_ref().filter(|_| key.picture) else {
        return preview;
    };
    // The video a player shows, never cover art.
    let Some((stream, _)) = info.primary_video() else {
        return preview;
    };
    let (index, at) = (stream.index, picture_time(info.duration(), options.picture_at_percent));
    let ffmpeg = programs.ffmpeg.as_deref().unwrap_or_else(|| Path::new("ffmpeg"));
    // The keyframe alone first; an exact decode for a stream without marked keyframes; the start of a file whose
    // duration is wrong (the place may lie past its last frame).
    let mut attempts = vec![(at, true), (at, false)];
    if at > Timestamp::ZERO {
        attempts.push((Timestamp::ZERO, false));
    }
    let mut result = Err(String::new());
    for (when, fast) in attempts {
        result = picture(ffmpeg, path, index, when, fast).await;
        if result.is_ok() {
            break;
        }
    }
    match result {
        Ok(thumbnail) => preview.thumbnail = Some(thumbnail),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "[Media] No picture for the preview");
            preview.problem = Some(e);
        }
    }
    preview
}

async fn picture(ffmpeg: &Path, path: &Path, stream: u32, at: Timestamp, fast: bool) -> Result<Thumbnail, String> {
    let size = picture_bytes();
    let output = run_as(
        ffmpeg,
        thumbnail_args(path, stream, at, fast),
        PICTURE_LIMIT,
        size,
        true,
    )
    .await
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(output.failure().to_string());
    }
    if u64::try_from(output.stdout.len()).ok() != Some(size) {
        return Err("ffmpeg wrote no picture".to_owned());
    }
    Ok(Thumbnail {
        width: THUMB_WIDTH,
        height: THUMB_HEIGHT,
        rgba: output.stdout,
    })
}

fn picture_bytes() -> u64 {
    u64::from(THUMB_WIDTH) * u64::from(THUMB_HEIGHT) * 4
}

/// Where the picture is taken: `percent` of the video's length in, its start when the duration is unknown.
pub fn picture_time(duration: Option<Timestamp>, percent: u8) -> Timestamp {
    duration.map_or(Timestamp::ZERO, |d| {
        Timestamp::from_micros(d.as_micros().saturating_mul(i64::from(percent.min(100))) / 100).max(Timestamp::ZERO)
    })
}

/// ffmpeg's arguments for one RGBA picture of [`THUMB_WIDTH`] x [`THUMB_HEIGHT`] of the stream with ffmpeg's index
/// `stream` at `at`: the source's pixel aspect and rotation applied (ffmpeg turns by itself), fitted inside and centered
/// on a transparent ground. `fast` decodes only the keyframe at or before `at`, on one thread. Timestamps pass through:
/// a keyframe before `at` would otherwise be dropped (no picture), and ffmpeg's demuxer would read on meanwhile (up to
/// the whole file; measured 2026-10-24: 0.2-2 MB read per picture with passthrough, 1-22 MB without).
pub fn thumbnail_args(file: &Path, stream: u32, at: Timestamp, fast: bool) -> Vec<OsString> {
    let (w, h) = (THUMB_WIDTH, THUMB_HEIGHT);
    let filter = format!(
        "scale=w='trunc(iw*sar/2)*2':h=ih,setsar=1,\
         scale=w={w}:h={h}:force_original_aspect_ratio=decrease:flags=bilinear,\
         format=rgba,pad=w={w}:h={h}:x=-1:y=-1:color=black@0"
    );
    let mut args: Vec<OsString> = ["-v", "error", "-hide_banner", "-nostats", "-nostdin"]
        .map(OsString::from)
        .to_vec();
    if fast {
        args.extend(["-noaccurate_seek", "-skip_frame", "nokey", "-threads", "1"].map(OsString::from));
    }
    args.push("-ss".into());
    args.push(at.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args.push("-map".into());
    args.push(format!("0:{stream}").into());
    args.extend(["-frames:v", "1", "-vf"].map(OsString::from));
    args.push(filter.into());
    args.extend(
        [
            "-an",
            "-sn",
            "-dn",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ]
        .map(OsString::from),
    );
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_taken_at_the_chosen_part() {
        assert_eq!(picture_time(None, 10), Timestamp::ZERO);
        assert_eq!(
            picture_time(Some(Timestamp::from_secs(20)), 10),
            Timestamp::from_secs(2)
        );
        assert_eq!(picture_time(Some(Timestamp::from_secs(20)), 0), Timestamp::ZERO);
        assert_eq!(
            picture_time(Some(Timestamp::from_secs(20)), 250),
            Timestamp::from_secs(20)
        );
        assert_eq!(picture_time(Some(Timestamp::from_micros(-5)), 10), Timestamp::ZERO);
    }

    #[test]
    fn arguments_take_one_rgba_frame_of_the_chosen_stream() {
        let args = thumbnail_args(Path::new("-odd: name.mkv"), 2, Timestamp::from_millis(1500), true);
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let at = |s: &str| text.iter().position(|a| a == s).unwrap();
        assert_eq!(
            text[at("-i") + 1],
            "-odd: name.mkv",
            "the path stays one argument after -i"
        );
        assert_eq!(text[at("-ss") + 1], Timestamp::from_millis(1500).to_ffmpeg_arg());
        assert!(at("-ss") < at("-i"), "input seeking is fast");
        assert_eq!(text[at("-map") + 1], "0:2");
        assert_eq!(text[at("-frames:v") + 1], "1");
        assert!(
            text[at("-vf") + 1].contains("pad=w=256:h=144"),
            "{}",
            text[at("-vf") + 1]
        );
        assert_eq!(text.last().map(String::as_str), Some("-"));
        assert_eq!(picture_bytes(), 256 * 144 * 4);
        assert!(
            at("-skip_frame") < at("-i") && at("-noaccurate_seek") < at("-i"),
            "decoder options of the input"
        );
        let exact = thumbnail_args(Path::new("a.mkv"), 0, Timestamp::ZERO, false);
        assert!(!exact.iter().any(|a| a == "-skip_frame" || a == "-noaccurate_seek"));
    }
}
