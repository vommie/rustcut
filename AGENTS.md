# AGENTS.md

Rules for anyone, human or coding agent, who changes RustCut. Keep changes small and in line with these rules.

## Stack

- Rust and Slint only, with system libmpv as the player (render API into a GL texture). FFmpeg and ffprobe run as
  supervised child processes, never in-process. Plugins are WebAssembly components (wasmtime, WIT API). No web views,
  no Python or shell helpers at runtime, no extra runtimes.
- Target: Arch Linux / Manjaro with KDE Plasma; X11 is tested, Wayland must not be broken by design.
- `rustcut-core` has no UI dependency; `rustcut-player` is the only crate with `unsafe`, each block with a
  `// SAFETY:` comment.

## Licenses and third-party material

- Own code is MIT; the built program is GPL-3.0-only (libmpv, FFmpeg, Slint under GPL-3.0).
- Never copy code, icons, fonts or other assets from other projects. Use a dependency, or write it yourself. Check every
  new dependency's license first; `about.toml` (accepted list) and `deny.toml` define what is allowed.
- `ATTRIBUTION.md` is generated: run `scripts/attribution.py` after every dependency change (`check-all.sh` fails
  while it is stale). Notices for non-crate material (icons, fonts, bundled C code) go into `scripts/attribution-head.md`.

## Icons

- Only Tabler Icons (MIT), registered in `crates/rustcut/assets/icons/icons.toml` (`build.rs` generates `Icon` and
  `IconId`); keep Tabler's file names. Own symbols in `assets/icons/custom/` are drawn in Tabler's style. No emoji,
  Unicode or Nerd Font glyphs as icons. Remix Icon is not allowed (license).

## Code

- Prefer maintained crates over handwritten code; own code is domain glue. Name a new crate in the commit message and
  add it to `[workspace.dependencies]`.
- US English everywhere: code, comments, logs, UI strings, docs, commits.
- `unwrap`, `expect`, `panic`, indexing and lossy casts are denied outside tests. Files under ~600 lines; `src/` holds
  only `lib.rs`, `main.rs` and module folders; `tracing` messages carry a module prefix (`[Queue]`).
- The UI thread only presents: updates go through `UiDispatcher`, actors own their state, no lock across `.await` or a
  Slint call. The render thread never calls the synchronous mpv API.
- Times are `i64` microseconds. Preview and render share one `FilterChain`.
- Output goes to a temp file, is verified with ffprobe, then renamed atomically; a target is never touched before
  success and never overwritten without the user's choice.
- Data lives in the XDG folders (config, data, state; `--home <dir>` redirects all three). Released database migrations
  are never edited; a backup comes before every migration. Settings are written atomically.
- RustView is reached only through `rustview api <cmd> --json`; never delete a record RustCut did not create.
- Never remove features, tests or log statements to make something pass.

## UI

- RustView's look (`ui/styles.slint`). Buttons are icon-only with a tooltip that names the key; destructive actions
  are held, not confirmed by a dialog; text fields are `TextLine`.
- Every UI state the user changes persists in `ui-state.toml` (default, sanitizing, test).
- One place per piece of information: a warning shows in one place, never repeated; the status bar is one slim line.

## Git and checks

- `master` is always green. Work on short branches (`feat/`, `fix/`, `refactor/`, `docs/`, ...) with Conventional
  Commits; merge with `--no-ff` after `scripts/check-all.sh --fast` passes. Never force-push `master`.
- Before a commit: `cargo fmt --all`, `cargo cl` (clippy, `-D warnings`) and the relevant tests (`cargo t`).
  `scripts/check-deps.sh` lists missing tools. The golden tests need the PyCutRv test media (`RUSTCUT_GOLDEN_MEDIA`,
  default: `pycutrv-testenv/golden/source` next to the repository) and fail without it.
- Every user-visible change gets an entry under `[Unreleased]` in `CHANGELOG.md` (Keep a Changelog, SemVer) in the
  same commit.
