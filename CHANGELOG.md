# Changelog

All notable changes to RustCut are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Transport bar under the video: play/pause, previous and next frame, a seek bar that jumps to the clicked spot (drag it to
  scrub), the current and the total time (`H:MM:SS.mmm`), mute and volume. The mouse wheel over the video changes the
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

[Unreleased]: https://github.com/vommie/rustcut/commits/master
