# RustCut

A fast local video cutter for Arch Linux / KDE Plasma, written in Rust with a Slint UI and mpv as the player: mark sections,
apply filters (crop, deinterlace, resize, orientation, deshake, ...), render them through a queue with professional encoding profiles
(x264, x265, AV1), and tag and rate the clips directly in the [RustView](https://github.com/vommie/rustview) library.

RustCut is the successor of PyCutRv. Status: early releases (0.0.x) on the GitHub releases page; see `HANDOFF.md` for
the plan and the current state, `CHANGELOG.md` for what changed.

![The main window: sections on the timeline, the filters, a queue with one job rendering, tags and a rating for the
next job](docs/screenshots/main.png)

![The crop edited on the video: the edges in the Filters card, the output size and the ratio button on the
frame](docs/screenshots/crop.png)

The screenshots come from the test environment and are retaken with one command: `scripts/readme-shots.sh` (after
`cargo build -p rustcut -p rustcut-thumbs`).

## Install (Arch Linux, Manjaro)

Download the newest package from the GitHub releases and install it:

```bash
gh release download --repo vommie/rustcut --pattern 'rustcut-x86_64.pkg.tar.zst' --pattern SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
sudo pacman -U rustcut-x86_64.pkg.tar.zst
```

Or build the package from a checkout and install it:

```bash
./build-package.sh
sudo pacman -U dist/rustcut-<version>-1-x86_64.pkg.tar.zst
```

`scripts/check-deps.sh` lists every package the build and RustCut need, with the exact `sudo pacman -S` command for what
is missing. RustCut needs `mpv` and `ffmpeg`; Tag & Rate needs RustView (a package from its GitHub releases).

## Develop

`cargo run -p rustcut -- [FILE...]` starts RustCut (`--home DIR` keeps its settings, queue and logs in `DIR`).
`scripts/check-all.sh --fast` runs formatting, clippy, the tests and the dependency checks; without `--fast` it also builds
and lints the package. The rules for contributors are in `CLAUDE.md`.

## License

MIT, see `LICENSE`.
