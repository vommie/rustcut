# Changelog

All notable changes to RustCut are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Jump marks (0.0.23): a message about an option has a small target button that leads straight to it. The panel or
  window opens, a folded card unfolds, the option scrolls into view and flashes three times. The Tag & Rate panel's
  header uses it while RustView cannot be reached (Settings > RustView, the program) or is switched off (the
  switch).

- New tooltips everywhere (0.0.21): one modern bubble with a small arrow that points at its button. The keys of a
  button sit on their own line under the text as key caps ("Key [Ctrl] [S]"; an action with two keys shows "Keys [,]
  or [PgUp]"; the arrow keys as arrows). Long explanations get a title and rows, e.g. the deinterlacer, the scaling
  algorithm and the deshake zoom list every choice with what it does. The bubble finds its place by itself: centered
  under the button, moved inward at the window's edges, above the button at the bottom of the window, beside the
  button when neither fits; a context menu's tooltip appears beside the menu so it never covers other entries. The
  first tooltip waits for the delay from Settings > Appearance, the next one along a row of buttons shows at once,
  and it fades in softly.
- The player's buttons (play/pause, frame steps, mute) have tooltips with their keys.

- The "file exists" dialog has tabs (0.0.20). "Decision" shows the existing file with a picture (a click plays it,
  the "i" in its corner shows date, size, length, resolution, codecs and bit rate) and every way out: next free number,
  overwrite, "(n)" and your own name, or the ways for queued jobs. "Similar files" lists every file of the name's
  numbered series ("Name - 01.mkv", "Name - 02.mp4", ...) in any video format and the other videos whose names start
  with the name, each with a picture, play and show-in-folder buttons, in two groups you can fold. "Queue" lists the
  jobs that write the same file when it also exists. Tab and Shift+Tab switch tabs.
- Pictures of the files are made in the background (the existing file first, then the list), a placeholder shows
  until each arrives, a file without one says why (audio only, not a video, not readable); "Reading 12 of 40 files"
  shows how far it is. Closing the dialog stops the work at once.
- Every dialog can be resized at its edges and corners; the size is remembered (`ui-state.toml`, `[dialogs]`), and
  switching tabs never changes it. The folded groups are remembered too.
- Settings > Dialogs: the way out chosen when the dialog opens, whether the series and the similar names are listed,
  which files get pictures (every file, the existing file only, none), where in a video the picture is taken, how many
  pictures are made at once and their size in the list (`[dialogs.conflict]` in `config.toml`).
- MD5 sidecars: when RustCut had to read a video for its MD5, it writes `.<name>.md5` next to it (md5sum's format), so
  neither RustCut nor other programs read it again; an existing sidecar is never changed. Settings > RustView > "Write a
  sidecar" (`[hashing] write_sidecar`, on).

### Changed

- The Job Finalization card checks the job again at once when the Codecs panel, the sections or the source's
  description change; an error of the render settings turns Save off with the reason in its tooltip instead of
  failing at the click (0.0.23).
- Buttons and menu entries that must be held show a red "Hold 2 s" badge in their tooltip (the time follows the
  setting) instead of a text starting with "Hold to" (0.0.21).
- The details behind the "i" of the "file exists" dialog use the new tooltip (0.0.21).
- The next free number passes over every number the name's series uses in another video format: with "Clip - 02.mp4"
  in the folder, an MKV job never becomes "Clip - 02.mkv". Gaps are filled first (0.0.20).
- The dialog's Save button names no file ("Save", "Overwrite", "Remove and save"); the chosen row shows the name. The
  header shows the whole path; a long folder is shortened, never the file name. "Don't ask again" left the dialog:
  whether it asks is on the Warnings page.
- Less drive work: pictures read only the keyframe they show (0.2-2 MB per file instead of up to the whole file), the
  pictures' ffprobe and ffmpeg and the MD5 reads run at a low priority so playback comes first, MD5 reads ask the
  kernel for larger read-ahead, and folders are read in one pass of names only.

- Arch package (0.0.19): `./build-package.sh` builds `dist/rustcut-<version>-1-x86_64.pkg.tar.zst` (install it with
  `sudo pacman -U dist/rustcut-<version>-1-x86_64.pkg.tar.zst`). It brings the desktop entry (RustCut opens videos
  from Dolphin and other file managers, several at once; the taskbar entry shows the queue's progress), the AppStream
  metainfo and the app icon in every size; it depends on `mpv` and `ffmpeg`, RustView and the desktop portal are
  optional. The package is built without the debugging server for automated UI checks.
- `scripts/release.sh` prepares a release (package, checksums, notes from this changelog) without tagging or publishing
  anything; `scripts/check-all.sh` validates the desktop file and the metainfo and, without `--fast`, builds the package,
  checks its content and lints it with namcap.
- App icon (0.0.18): a film frame with scissors in RustView's style; the windows and the tray use it.
- Tray icon (optional, `desktop.tray.enabled`, off by default): shows the queue in its tooltip ("1 rendering, 2 waiting ·
  42 %"); a click shows or hides the window (a minimized window comes back), its menu pauses or resumes the queue
  and quits. With `desktop.tray.minimize_to_tray`, minimizing the window while a job renders hides it in the tray; the
  open video plays on from the same place when the window comes back.
- Notifications for finished jobs (with "Open folder"; off by default), failed jobs (with the reason and "Show log")
  and the queue being done (`desktop.notifications.*`).
- When the queue is done (0.0.17): a power button in the queue panel chooses what happens after the last job:
  nothing, a notification, quit RustCut, suspend or power off. Before quit, suspend or power off a banner above the
  video counts down (`queue.when_done_countdown_secs`, default 5 s) with an Abort button (Escape also aborts), and a
  desktop notification with Abort appears. After quitting, suspending or powering off, the choice goes back to
  "nothing", so it never repeats by surprise; the choice is kept in `ui-state.toml`.
- Closing RustCut while a job renders asks first (`general.confirm_quit_while_rendering`): keep rendering (Escape),
  quit when the queue is done, or quit now (hold the button; the job starts again at the next start).
- While a job renders, the computer does not go to sleep (`desktop.inhibit_sleep`, through systemd-logind).
- The queue's progress and the number of open jobs show on RustCut's taskbar entry (`desktop.taskbar_progress`; KDE
  Plasma shows it once RustCut's desktop file is installed).
- Logs panel (0.0.16): besides the log of the job chosen in the queue, it shows RustCut's own log of this run (the
  lines the log file gets, with time and level) or only its RustView part (the link and the Tag & Rate panel). The
  log grows while it is shown; search, "errors only", following the newest line and Copy work on every log. The
  chosen log is kept in `ui-state.toml` (`logs.source`).
- Settings window (0.0.15; File > Settings..., Ctrl+, or Help > About RustCut), styled like RustView's: General, Player,
  Editor & Sections, Filters, Export & Naming, Render Profiles, Queue, RustView, Warnings & Confirmations, Hotkeys,
  Desktop Integration, Appearance, Advanced & Logs and About (with the Slint attribution). Every change is saved at
  once (no Save button); options that only work from the next start say so. A held button in the page header puts
  the page's options back to their defaults. The window's size, its page, the selected profile and the hotkey filter
  are kept in `ui-state.toml` (new `[settings]` table).
  - Export & Naming: the file-name template is checked while you type it and shows an example; the named target
    folders can be added, labeled, reordered and removed (held).
  - Render Profiles: every profile with the one new jobs start with (star); built-in profiles are shown read-only,
    "Duplicate" makes an own copy, and an own profile is saved with every change, renamed or deleted (held). The
    Codecs panel follows at once.
  - Hotkeys: every action with its keys, a filter, a key button that waits for the next key (Escape cancels), a
    cross per key, and a reset per action. A key another action has is named first; pressing it again moves it.
    `config.toml` keeps only the actions whose keys differ from the defaults.
- New settings (no migration; older files load with these defaults): `appearance.tooltips` (on) and
  `appearance.tooltip_delay_ms` (600), `rustview.tag_history_size` (20, up to 100), key `open_settings` (Ctrl+,).
- Help menu: open the settings, data and log folders.

- Queue panel (0.0.13): in a narrow sidebar the jobs show as two-line rows; as the panel gets wider (or its group is
  maximized) it becomes a table whose columns join as there is room: number, name, state with the render's
  progress, ETA, length (and the number of sections), size (estimated while rendering, measured when finished),
  profile and folder. Drag a column's edge to make it wider or narrower. Select several jobs with Ctrl+click and
  Shift+click (Ctrl+A selects every job shown); drag the selected jobs to another place, or move them with
  Ctrl+Up/Down and Ctrl+Home/End (configurable keys `queue_move_up`, `queue_move_down`, `queue_move_top`,
  `queue_move_bottom`, `queue_select_all`). State chips with counts (waiting, running, held, finished, failed,
  canceled) and a search over names, paths, profile, note and failure narrow the list. The header shows the overall
  progress of the current run with the time left, and a second switch pauses after the running jobs. A double click
  plays a finished job's output, shows a failed or canceled job's log, and shows any other job in the Job panel.
  The filter, the search, the column widths and the selection are kept in `ui-state.toml`.
- A context menu for the selected jobs (right click), with the number of jobs each entry reaches: edit in the editor
  or load a done job as a new one, show in the Job panel, show the log, edit tags and rating, play the output, show
  it in its folder, hold or release, render again, **render again with another profile** (any profile, or the Codecs
  panel's current settings), duplicate, move to the top or bottom, cancel, remove, and remove every finished job.
- Job panel: a new dock panel (next to Codecs by default, movable like every panel) shows the queue's current job:
  its state, times, attempt and RustView state, the source, the output, every section, the filters in their order,
  the codec settings, and the tags (by name) and rating, in sections you can fold. Its buttons edit, play, reveal,
  show the log, edit the tags, render again and duplicate the job.
- Logs panel: the log of the queue's current job (RustCut's notes and ffmpeg's lines; errors red, warnings yellow),
  with a search, "errors and warnings only", following the newest line and Copy to the clipboard. The log of a
  running render is stored when it ends.
- Editing queued jobs: "Edit in the editor" loads a waiting job's video, sections, filters, codec settings, target
  and tags into the editor; the job is held meanwhile and waits again afterwards. A banner over the Job Finalization
  card says which job is edited. Save updates the job while it writes the same file; another name, counter number,
  folder or container makes a new job instead, and "Save as new" always does. A finished, failed or canceled job can
  be loaded as the start of a new job. The new setting `editor.draft_on_load` decides what happens to the editor's
  draft: `replace` (default, like PyCutRv), `ask`, or `set_aside` (it comes back when the editing ends). Jobs now
  remember the name and counter they were saved with, so they show the same in the editor (jobs saved by older
  versions show their file name).
- Hold to confirm: buttons and menu entries that delete or discard something (remove or cancel jobs, remove the
  finished jobs, remove a target folder, delete or reset a render profile, delete a section, reset the filters,
  clear the tags) act only while held, instead of asking in a dialog: a red line runs around the button and its
  background fills up; let go early and nothing happens (a short click wiggles it and shows what holding does). The
  time is the new setting `general.hold_to_confirm_ms` (default 2000; 0 = a click is enough). Their keys still act
  at once.

- Tag & Rate panel (0.0.12): RustView's tag tree (in columns, with the same look as in RustView), its rating bar, the
  tag filter (plain text or, with the `.*` switch, a regular expression; an invalid expression is matched as text and
  says so) and the tag history (recently used tags, a click assigns or removes one). The tags and the rating you
  choose go with the next saved job into RustView. After a save the panel is cleared; "Apply the last saved job's
  tags" and the last rating button bring them back with one click. **Right-click a tag to lock it:** a locked tag
  (lock icon) stays assigned for every following job until you right-click it again. "Clear" removes the tags and
  the rating (locked tags stay); expand/collapse every branch; the eye button lets you hide branches you never use
  (click a branch to hide or show it, hidden ones are dimmed while choosing). The panel's header is the one place
  that says whether RustView is connected; when it cannot be reached it shows why, retries by itself and offers
  Retry. Expanded branches, filter, history, hidden branches, locked tags, the history's height and the last tags
  and rating are kept in `ui-state.toml` (new `[tags]` table; older files load unchanged, no migration).
- Saving a job without tags or without a rating asks once (`warnings.no_tags`, `warnings.no_rating`): the dialog
  names what is missing, a click on a rating (or the keys 0-5) saves with it, "Save without" (Enter) saves as it is,
  "Back to tagging" (Escape) shows the Tag & Rate panel, and "Don't ask again" switches the asking warning off. It
  asks about tags only when RustView offers tags, and nothing while the RustView link is switched off.
- Editing a job's RustView entry: the tag button in the queue panel switches the Tag & Rate panel to the selected
  job ("Editing: <name>"); Apply writes the new tags and rating (a waiting job's record is updated, a finished job's
  file record in RustView too; a failed or canceled job keeps them for a retry), Cancel or Escape returns to the next
  job's tags, which are kept meanwhile.
- RustView link (0.0.11): RustCut now talks to RustView through `rustview api` in the background (never blocking the
  window; one call at a time, each with the time limit `rustview.timeout_secs`, the program from `rustview.binary` or
  `rustview` in your PATH). Each queued job gets its record in the RustView library according to
  `rustview.write_timing`: "at_enqueue" (default) writes a pending record when the job is saved, which RustView
  completes once the file exists, and writes it once more after the render was verified; "after_render" writes it
  only after the render. Before the first write RustCut asks whether RustView already has a record for the file: such
  a record keeps its source app and is never deleted by RustCut; only the values RustCut changed are given back when
  the job does not produce the file. A job that fails, is canceled or removed before it finished loses the pending
  record RustCut created; a waiting job whose file name changes moves its record along. A record stays when the file
  exists or when RustView now says another app made it. While RustView cannot be reached, RustCut tries again after
  2 s, then 4 s, 8 s, ... up to every 2 minutes; nothing is lost when RustCut quits meanwhile (the next start
  continues). The tags and rating come from the Tag & Rate panel (0.0.12).
- Known-source warning: when a video opens, its MD5 comes from RustView's record of the file if the record matches
  the file's size and time (shown as "From the RustView library" in Media Info, no need to read the file), else from
  the sidecar `.<name>.md5` or by reading the file. RustView is then asked whether a job was made from this video
  before; the banner over the video shows when (UTC) and the files made from it (the video's own library record does
  not count). The source's MD5 is registered in RustView only when a job of it is saved, never just by opening it.
  Switch the warning off with `warnings.known_source = false`.
- Saving jobs (0.0.10): Save (Ctrl+S or F9, also File > Save Job) turns the open video into a job of the render
  queue: its sections (with an empty list the automatic section of `editor.auto_section`, else Save asks for a
  section), the filter chain in its order, a copy of the Render Settings panel's current settings (changing them later
  does not change the job) and what the probe found about the source.
- Job Finalization card (Export panel): the target folder ("Source folder" = next to the video, always first, then
  your named folders; the folder icons add a folder through the system's folder dialog or take the chosen one off the
  list, the folder itself stays; stored in `config.toml`, `[[export.target_folders]]`), the name (the video's file
  name when a video opens), the counter with its switch, auto-increment (the counter goes up after each save) and
  "clear the sections after each save" (both kept in `ui-state.toml`). Below them the file the job will write
  (naming template `export.name_template`) and a live check: free, a hint when other files in the folder start with
  the same name, a conflict that Save will ask about (the file exists, or a job in the queue writes it), or why the
  job cannot be saved (no video, the name is the source file itself, the folder cannot be read). Opening another
  video starts the counter at 1 again.
- Conflict dialog when saving: when the file exists or a queued job writes the same file, a dialog shows the
  existing file (size and date, play it or show it in the file manager), the queued jobs, similar names in the folder,
  and every way out with the file name it leads to: the next free number (the counter continues after it for the
  next job; "Add a number" when the counter is off), "Name (n)", another name typed in the dialog (checked while you
  type), overwrite (the old file is replaced only after the new render is verified), remove the queued jobs, or save
  anyway. Keys 1-6 pick, Up/Down move, Enter saves, Escape cancels. "Don't ask again" switches the warning off in
  `config.toml` (`[warnings]` `target_exists`: then a free name is taken without asking, an existing file is never
  replaced unasked; `target_queued`: then the job is saved anyway).
- Render queue: saved jobs render one after the other (`queue.workers` renders at the same time, default 1) at low
  priority; two jobs that write the same file never render at the same time, and a job that cannot render fails on
  its own without holding up the jobs behind it. The queue is stored in the profile's database
  (`~/.local/share/rustcut/queue.sqlite`) with each render's log, and comes back after a restart; a render stopped by
  quitting, a crash or a kill waits again at the next start (with a note), and what it left (the hidden temp file, the
  deshake and two-pass work files) is deleted. Removing a job never touches its source or the source's `.md5` file.
- Queue panel (a first version; the full panel follows): two-line rows with the file, the state, a progress bar,
  and the folder, profile and length, or while rendering the pass, ETA, the estimated file size, speed and fps, or
  why a job failed; the job id, attempt, target, source and notes in the tooltip. Buttons: pause/resume processing
  (paused renders stop where they are; kept across restarts), and for the selected job move up/down, hold/release,
  render again, cancel (an existing target stays) and remove.
- Render Settings panel (left sidebar, next to Export), in five cards:
  - Profile: quick selection of the built-in profiles and your own. Changes go to a working copy marked "(modified)";
    icons drop the changes, save them into your own profile, save them as a new profile, rename or delete your profile.
    Your profiles are stored in `config.toml` (`[[render.profiles]]`); the working copy and the profile it came from
    are remembered across restarts (`ui-state.toml`, `[export]`: `profile`, `render`). Problems of the settings are
    listed here, errors first (the render would fail or the file would not be standard) and then warnings (plays on few
    devices, slow combinations); the option concerned is marked in its card.
  - Encoding (the simple view): the codec (only encoders that work on this computer; the software encoders while the
    check at start still runs), a quality slider from "Small" over "Good", "High" and "Very high" to "Archive" that means
    about the same picture with every codec (the codec's own CRF, QP or CQ is shown under it), five speeds from
    "Fastest (largest files)" to "Slowest (smallest files)", and the container. Switching the codec keeps the slider
    position, the speed, a bit rate, the keyframe interval and the color tags; options only the old codec had are
    dropped, and H.264 starts at 8 bits.
  - Audio: codec, bit rate, channels and sample rate.
  - Expert (folded at first), per codec: rate control (constant quality, constant QP, average bit rate, bit rate in two
    passes, target size in two passes) with its value, the codec's full preset list, tune, profile, level, bit depth,
    keyframe interval, film grain and variance boost (SVT-AV1), AQ mode, keyframes at scene cuts (x264, x265), color
    tags and range (labels only, nothing is converted), and extra codec parameters (`key=value:key=value`).
  - Probe Render (see below).
  Every option has a tooltip that says what it does and what it costs.
- The real encoder checks the settings in the background half a second after a change (a few frames of a test picture
  with silence, into the chosen container): settings it refuses and codec parameters it ignores (x264, x265, SVT-AV1 and
  rav1e only warn about keys they do not know) are listed as errors, its own advice as warnings.
- Probe render (T, the flask button of the Probe Render card, or Filters > Probe Render): renders a few seconds around
  the playhead (10 by default, 2 to 60, a quarter of it before the playhead) with the current filters, deshake included,
  and the render settings at the encoder's fastest speed, then plays it in the player in a loop with a banner above the
  video. Escape or the banner's button returns to the source at the same place, paused or playing as before. While the
  probe plays, the timeline is dimmed and marks and sections wait. "Exact settings" renders with the profile's own
  speed and estimates the size of the whole job (the sections, or the whole video without sections). The length and
  the switch are remembered (`ui-state.toml`, `[export.probe]`). Probes are written to the cache folder (`probe/`) and
  removed at the next start.
- Size estimate of a render after its first seconds, from the bytes written so far (used by the probe render now and by
  the queue later).
- No migration: `config.toml` and `ui-state.toml` of 0.0.8 load unchanged (the new options have defaults), and a user
  profile in `config.toml` that cannot be read is skipped instead of resetting all settings.

- Render engine (not yet reachable from the window; the queue that drives it comes next): a job's sections are cut with
  frame-accurate seeking and joined, every audio track of the source is kept with its language and title, the filters
  run exactly as previewed (also on rotated phone videos and odd sizes, which are rounded to even pixels), and global
  metadata is copied; subtitles and chapters are left out. Variable frame rates (screen and phone recordings) keep
  their timing in every container. Deshake renders in two passes (motion analysis, then the transform); two-pass
  bit-rate profiles get their first pass. ffmpeg runs at a lower CPU and idle I/O priority (`render.nice`,
  `render.idle_io`), can be paused without using any CPU, and is stopped when it makes no progress for
  `render.stall_timeout_secs`. The output is written to a hidden `.<name>.rustcut-part.<ext>` file next to the target,
  checked with ffprobe (video, every audio track, size, duration) and only then renamed onto the target. A failed or
  canceled render removes only its temp file: an existing file at the target is never touched, and without
  "overwrite" a file that appeared meanwhile is never replaced. Each render keeps a log with the exact ffmpeg command
  lines and ffmpeg's messages; a failure names ffmpeg's actual reason.
- Encoder check at start, in the background (about half a second): every encoder RustCut knows is tried with a
  one-frame test encode, so hardware encoders (VAAPI, NVENC, QSV) are offered later only when they really work here; the
  result, with the reason for each encoder that does not work, is written to the log.

- Filters card in the Export panel: deinterlace, crop, deshake, rotate, mirror and resize, one row each with a switch,
  shown in the order they run (move a filter up or down to change it), their settings while switched on, the output size
  ("Output 1916×1076", or the even size the render makes of an odd one, or a warning when the crop removes everything),
  and buttons to show the filters in the player or not, to keep them for the next video and the next start (all filters
  with their order; `ui-state.toml`, `[export]`), and to reset them. Buttons are icons; their tooltips explain them and
  show their keys.
- Live preview: the player shows the filters as they will be rendered (deshake only in the render; it needs two passes).
  Rotated videos (phone recordings) are cropped in the orientation they are shown, like the render does.
- Crop: top, right, bottom and left as seen in the output (after a rotation or mirror that follows the crop); type a
  value or use the mouse wheel over a field (2 pixels per notch, Shift: 20).
- Cropping on the video: press C or the crop button to see the whole picture with the crop on it; drag a corner, an edge
  or the area itself, with the size shown, the rest dimmed and thirds lines while dragging; Enter keeps the crop, Escape
  restores the one from before. Quicker: hold Ctrl over the video and drag; the edge or corner nearest to the pointer
  follows, no aiming needed, and releasing Ctrl keeps the result. An aspect ratio lock (Free, Source, 16:9, 4:3, 3:2,
  1:1, 9:16) shapes the crop while it is dragged; the choice is kept. Edges snap to even pixels.
- Auto-crop (Shift+C or the wand button) looks at up to eight points of the video instead of only the current frame,
  ignores black scenes, and crops only what is black everywhere, rounded to even pixels.
- Deinterlace with bwdif, yadif, w3fdif or kerndeint, optionally one frame per field (2×); a background check of the
  video says next to it whether it is interlaced (and which field comes first). nnedi is not offered: it needs a weights
  file that ffmpeg does not ship.
- Resize to a width and/or height (an empty side keeps the aspect ratio, shown as "auto (720)"), with 16:9 and 4:3
  helpers that compute the height from the width and a choice of scaling algorithm; the output has square pixels.
- Rotate in quarter turns (R right, Shift+R left) and mirror left/right and top/bottom.
- Deshake settings: shakiness, accuracy, step size, smoothing, zoom, automatic zoom, border fill, interpolation and the
  virtual tripod.
- Filters menu: show or hide the filters in the player (P), keep filters, edit the crop on the video (C), detect black
  borders (Shift+C), rotate left/right, mirror, reset. All are keymap actions (`crop_edit`, `auto_crop`,
  `toggle_filter_preview`, `toggle_keep_filters`, `rotate_left`, `rotate_right`, `mirror_horizontal`,
  `mirror_vertical`, `reset_filters`).
- Tooltips on the buttons of the Filters and Sections cards, with their keys.

- Timeline under the video: a time ruler, every section of the list as a numbered block (the selected one highlighted,
  overlapping ones outlined in red), the current section's start and end marks with the range between them, the
  playhead, and the time under the mouse pointer. Click to jump there and drag to scrub; click a section to select it;
  drag a section edge or a mark to move it (it snaps to the playhead, the marks and other edges, else to the video's
  frames; hold Shift to snap to frames only), and the video shows the frame at the edge. The mouse wheel zooms around the
  pointer (Shift + wheel or a horizontal wheel scrolls), while zoomed a strip below shows and moves the visible part, and
  the view pages along with the playhead. The zoom is kept for the next start (`ui-state.toml`, `[timeline]`).
- Sections card in the Export panel: the current section's start and end (from the playhead with "Set", the keys I/Home
  and O/End, or typed), its length, and "Add section". Setting a start after the end (or an end before the start) clears
  the other mark, so a new section begins. Added sections are listed with start, end and duration, the number of
  sections and the total length; move the selected one up or down or delete it. A click on a time jumps there, a double
  click edits it (Enter keeps, Escape cancels; reversed times are swapped, times past the end are limited to it). A
  section that is already in the list is not added twice, and overlapping sections are pointed out.
- Sections menu: Set Start, Set End, Add Section, Go to Start, Go to End, Previous and Next Cut Point. View menu: Zoom
  Timeline In, Zoom Timeline Out, Fit Timeline.
- Keys (PyCutRv's plus NLE style): I or Home sets the start, O or End the end, + or Scroll Lock adds the section, Delete
  deletes the selected one, Shift+I / Shift+O go to the start / end, Up / Down jump to the previous / next cut point,
  Left / Right seek 2 s and Shift+Left / Shift+Right 10 s (`editor.seek_step_secs`, `editor.seek_step_long_secs`),
  Page Up / Page Down step a frame like `,` / `.`, J / K / L shuttle (L plays forward and each press doubles the speed
  up to 8x, J plays backward the same way, K stops; the speed shows next to the time), Ctrl++ / Ctrl+- / Ctrl+0 zoom the
  timeline. Ctrl+S and F9 are reserved for saving the job, which arrives with the queue. Held keys repeat only steps,
  seeks and zoom.
- Configurable keymap: `config.toml` takes a `[keymap]` table (`set_start = ["I", "Home"]`, Slint key names such as
  `Control+S`, `Shift+LeftArrow`, `PageUp`, `Plus`, `ScrollLock`); an action that is missing keeps its default keys and
  an empty list unbinds it. Unknown actions and keys are ignored and a key bound to two actions stays with the one that
  was configured (all logged at startup). The menus show the configured keys. The editor for it comes with the Settings
  window. No migration: older `config.toml` files load unchanged.
- Media Info panel: when a video opens, it shows the container (format, duration, start time, size, overall bit rate,
  title) and every stream: codec and profile, resolution, rotation (and the size it is shown at), aspect ratio, frame rate
  (a variable frame rate is detected and shown with its range), frame count, scan type (progressive or interlaced),
  pixel format and bit depth, color, sample rate, channels, sample format, bit rate, language, title, and stream
  durations or start times that differ from the container's (for example audio that ends early). A file that cannot be
  read says why. The information comes from `ffprobe` (setting `advanced.ffprobe`, otherwise from `PATH`); a stalled
  ffprobe is stopped after 20 seconds.
- The MD5 of the open video is shown in the Media Info panel. A sidecar file `.<name>.md5` next to the video is used
  when it exists (it is never changed or deleted); otherwise the MD5 is computed in the background with a progress bar
  and a Cancel button, without slowing down the window. It is computed automatically while the known-source warning
  (`warnings.known_source`) is on, else on demand with "Compute". The warning itself (a banner above the video when
  RustView knows the video already) arrives with the RustView connection.
- Transport bar under the timeline: play/pause, previous and next frame, the current and the total time (`H:MM:SS.mmm`),
  mute and volume. The mouse wheel over the video changes the
  volume (`player.wheel_volume_step`, 5 % per notch). Volume and mute are kept for the next start.
- Playback menu with keys: Space plays or pauses (at the end of a video it starts over), `,` and `.` step one frame back
  and forward, M mutes, F5 reloads the video.
- Opening videos: File > Open (Ctrl+O) shows the desktop's file dialog, starting in the folder of the last video; drag a
  video onto the window (a hint shows while dragging; of several files the first one is opened); `rustcut VIDEO` while
  RustCut runs hands the video to the open window instead of starting a second one (setting `general.single_instance`;
  profiles started with different `--home` folders stay apart).
- A video that cannot be played says why over the video, with a Reload button (click or F5); the rest of the window stays
  usable.
- The window title shows the video and its folder: `<file> (<folder>) - RustCut`. The status line shows messages (for
  example when a dropped item is not a file) and how the video is decoded (hardware or software).
- The player settings are applied: auto play, hardware decoding (`player.hwdec`), the background color around the video,
  muting the last second of a video, and loudness normalization of the preview audio.
- Log files: one file per day in `~/.local/state/rustcut/logs/`; the newest `advanced.log_keep_days` (14) are kept.
- File > Quit (Ctrl+Q).

- Dock layout in RustView's style: the left sidebar holds Export (with collapsible cards) and Render Settings above the
  Queue, the right sidebar holds Tag & Rate, Media Info and Logs. Drag a tab into another group or sidebar, onto the top
  or bottom of a group to split off a new group, or to the window edge to bring back an emptied sidebar; the drop area
  lights up while dragging. Click the active tab (or the chevron) to collapse a group, double-click a tab bar (or use the
  maximize button) to let a group fill its sidebar, collapse a whole sidebar to a rail of panel icons, and resize the
  sidebars and the groups with the bars between them. The layout is kept in `ui-state.toml`; View > Reset Layout
  restores the default arrangement. The panels themselves arrive with their milestones.
- Video playback: a file given on the command line (`rustcut VIDEO`) plays in the main window. mpv draws into RustCut's own
  OpenGL context, so later overlays (crop handles, section markers) can be drawn directly over the video. On a software
  OpenGL renderer (for example llvmpipe in a virtual machine) the preview uses mpv's fast scaling automatically, so playback
  keeps the video's frame rate; the rendered output is not affected.
- First runnable build: RustCut opens its main window in RustView's color palette, with a one-line status bar.
- Command line: `rustcut [FILE]... [--home DIR]` (`--home` keeps settings, data and logs in an isolated folder).
- The window size, position and maximized state are restored at the next start (`~/.config/rustcut/ui-state.toml`).
  Closing the window, File > Quit and the signals SIGTERM, SIGINT and SIGHUP all save the window, the layout and the
  volume before RustCut ends; a second signal ends it at once. A stored position far off every screen is dropped and
  `advanced.log_keep_days = 0` is read as 1 (no migration: the files keep their version).
- On X11 the window class is `rustcut` / `io.github.vommie.rustcut`, so the task manager groups the window correctly.
- For development: debug builds include Slint's MCP server; started with `SLINT_MCP_PORT=<port>`, a running RustCut can be
  inspected and driven from an MCP client (element tree, screenshots, clicks). Without the variable nothing listens.
- Settings (`~/.config/rustcut/config.toml`) and the UI state are versioned: files of older versions are backed up to
  `~/.config/rustcut/backups/` before they are migrated, files of newer versions are never overwritten, and damaged files
  are kept as a backup while defaults are used. The log level can be set with `advanced.log_level` (`RUST_LOG` wins).

### Changed

- The close question while a job renders follows the queue: while it is open no new job starts (the running ones go
  on). Once no job renders anymore (the running render or the whole queue is done), it says so and RustCut quits by
  itself after 3 s; "Stay" (Escape) keeps RustCut open and lets the queue go on. With "when done" set to quit,
  suspend or power off, that action runs instead (and RustCut quits after a suspend).
- Settings changed in the Settings window apply while RustCut runs where they can: hold time, tooltips, keys, seek
  steps, auto play, wheel step, preview audio filters, the export card's folders, template and warnings, the
  automatic section, filter defaults, the draft rule when a job is edited, render profiles, ffmpeg's priority and
  stall limit, the number of jobs at the same time, the RustView link and the tag history size, the log level and
  how long log files are kept, hardware decoding and the background color of the player (also while a video plays),
  the ffmpeg and ffprobe programs (the encoders are checked again), the known-source warning, and "One window" (a
  second start opens its own window as soon as it is off). Only the tray icon still needs a new start.
- Closing the main window quits RustCut even while the Settings window is open.
- The Render Settings panel's tab is called "Codecs" (shorter; your layout keeps its place).
- The queue panel's actions apply to every selected job; its buttons sit below the list.
- The Sections card's buttons are icons with tooltips (set start/end to the playhead, add, move, delete).

### Fixed

- Audio "Copy (no re-encode)" works (0.0.23): a job with one section takes the source's audio tracks unchanged.
  Before, every job with audio failed when it rendered. With several sections, or when the container cannot take the
  source's audio (e.g. AAC in WebM), Save is off and its tooltip names the reason before the job is saved.
- QuickTime (.mov) with Opus or FLAC audio is marked as an error in the Codecs panel (ffmpeg refuses it; it was only a
  warning, and the render failed) (0.0.23).
- A `rustview` launcher that cannot find its program (exit code 126 or 127, e.g. a wrapper script) shows RustView as
  unavailable with the reason, instead of as connected without tags (0.0.23).
- The Tag & Rate panel lays out the tag tree like RustView again (0.0.22): the tags run down a column and a new
  column starts only when the area is full. Before, every tag got a column of its own, so the tags stood side by side
  in one row.
- The last progress of a render (often its end) is no longer lost when ffmpeg writes it just before it exits.
- Starting ffmpeg, ffprobe or rustview tries again for a moment while the system reports the program as busy (for
  example while a package update replaces it) instead of failing the job at once.
- Quitting RustCut while a video without audio is open no longer logs "The file cannot be played: The video output
  could not be started." (0.0.14). Closing the window takes the video output away from the player, which is a normal
  end of playback, not a failure; a real failure of the video output still shows the error over the video.
- FLAC audio honors the channel count and the sample rate of a profile (both were ignored).

[Unreleased]: https://github.com/vommie/rustcut/commits/master
