# Changelog

All notable changes to RustCut are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

- The Sections card's buttons are icons with tooltips (set start/end to the playhead, add, move, delete).

[Unreleased]: https://github.com/vommie/rustcut/commits/master
