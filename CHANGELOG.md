# Changelog

All notable changes to RustCut are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
- The window size and maximized state are restored at the next start (`~/.config/rustcut/ui-state.toml`).
- Settings (`~/.config/rustcut/config.toml`) and the UI state are versioned: files of older versions are backed up to
  `~/.config/rustcut/backups/` before they are migrated, files of newer versions are never overwritten, and damaged files
  are kept as a backup while defaults are used. The log level can be set with `advanced.log_level` (`RUST_LOG` wins).

[Unreleased]: https://github.com/vommie/rustcut/commits/master
