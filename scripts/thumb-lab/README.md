# The thumbnail lab

Repeatable measurements for the seek bar thumbnails (P12 block 6). `lab.py` runs the candidates over long test videos and
appends one JSON line per run to `results.jsonl`; `thumblab.c` is the in-process candidate (libav, one long-lived decoder per
worker). Nothing is written next to the media, and nothing of this ships with RustCut.

```bash
python3 scripts/thumb-lab/lab.py build
python3 scripts/thumb-lab/lab.py all --extra /path/to/a/real/video.mkv
python3 scripts/thumb-lab/lab.py report
```

`--dir` sets the working folder (default `~/claude/rustcut-testenv/scratch/thumbs-bench`, which also holds the test videos;
`lab.py media` derives the container variants from `h264_1080p_1h_gop10.mp4`). `--quick` shortens a run, `--extra` adds any
video to the groups. Groups: `engines` (the approaches side by side), `tuning` (threads, workers, loop filter, scaler, width),
`order` (visiting order on a cold cache, a full pass), `edge` (containers, few keyframes, the short test media), `formats`
(JPEG, WebP, PNG), `priority` (real-time playback beside a generator), `drain` (the decoder's reorder delay, mpv variants),
`load` (what the generator costs a full-speed foreground at each priority).

A real number needs a quiet machine. "Cold" runs drop the file from the page cache with `posix_fadvise`, which needs no root.
