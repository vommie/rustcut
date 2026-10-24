# RustCut

A fast local video cutter for Arch Linux / KDE Plasma, written in Rust with a Slint UI and mpv as the player: mark sections,
apply filters (crop, deinterlace, resize, rotate, deshake, ...), render them through a queue with professional encoding profiles
(x264, x265, AV1), and tag and rate the clips directly in the [RustView](https://github.com/vommie/rustview) library.

RustCut is the successor of PyCutRv. Status: in development (no release yet); see `HANDOFF.md` for the plan and the
current state, `CHANGELOG.md` for what changed.

## Install (Arch Linux, Manjaro)

Build the package from a checkout and install it:

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
