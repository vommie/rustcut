#!/usr/bin/env python3
"""The thumbnail lab (P12 block 6): repeatable measurements of how to fill a seek bar with thumbnails.

    lab.py media                 make the long test videos and the container variants (in --dir)
    lab.py build                 compile thumblab.c into --dir
    lab.py engines|tuning|order|edge|formats|priority|drain|load|render [--quick]   one group of measurements, appended to results.jsonl
    lab.py all                   every group
    lab.py report                print the tables of results.jsonl

Any video can join with --extra FILE (repeatable): real footage says more than the synthetic clips. Page cache: "cold" runs
drop the file from the cache with posix_fadvise first (no root needed); nothing here writes next to the media.
"""
import argparse
import json
import os
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_DIR = Path.home() / "claude/rustcut-testenv/scratch/thumbs-bench"
SOURCE = Path.home() / "claude/rustcut-testenv/source"
WIDTH = 320


def run_quiet(*cmd):
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def cold(path):
    """Drop the file from the page cache, so the next read comes from the disk."""
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
    finally:
        os.close(fd)


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
         "format=duration:stream=width,height,codec_name", "-of", "json", str(path)],
        capture_output=True, text=True, check=True).stdout
    data = json.loads(out)
    stream = data["streams"][0]
    return float(data["format"].get("duration", 0)), stream["width"], stream["height"], stream["codec_name"]


def even_height(width, w, h):
    return int(width * h / w) & ~1


class Lab:
    def __init__(self, args):
        self.dir = Path(args.dir)
        self.quick = args.quick
        self.extra = [Path(e) for e in args.extra]
        self.results = self.dir / "results.jsonl"
        self.tool = self.dir / "thumblab"

    def media(self, names):
        found = [self.dir / n for n in names if (self.dir / n).exists()]
        return found + self.extra

    def save(self, group, media, label, row):
        row = {"group": group, "media": Path(media).name, "label": label, **row}
        with self.results.open("a") as f:
            f.write(json.dumps(row) + "\n")
        keys = ("t_first", "t_5", "t_all", "per_ms", "unique", "err_mean_s", "cpu_s", "read_mb")
        print(f"{row['media'][:30]:30} {label:34} " + " ".join(f"{k}={row[k]}" for k in keys if k in row), flush=True)

    # ---- candidates
    def thumblab(self, path, *options, prefix=()):
        cmd = [*prefix, str(self.tool), str(path), *map(str, options)]
        done = subprocess.run(cmd, capture_output=True, text=True)
        if done.returncode != 0 or not done.stdout.strip():
            return {"failed": done.returncode}
        return json.loads(done.stdout.strip().splitlines()[-1])

    def spawn_each(self, path, count, jobs=1, nokey=True):
        """One ffmpeg process per thumbnail, what RustCut's media/preview.rs does today."""
        duration, w, h, _ = probe(path)
        height = even_height(WIDTH, w, h)
        times = [(i + 0.5) * duration / count for i in order_levels(count)]
        running, arrival, start, next_i = [], [], time.monotonic(), 0
        while next_i < len(times) or running:
            while next_i < len(times) and len(running) < jobs:
                cmd = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-noaccurate_seek", "-ss",
                       f"{times[next_i]:.3f}", *(["-skip_frame", "nokey"] if nokey else []), "-i", str(path), "-map", "0:v:0",
                       "-an", "-sn", "-frames:v", "1", "-vf", f"scale={WIDTH}:{height}:flags=fast_bilinear",
                       "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]
                running.append(subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL))
                next_i += 1
            child = running.pop(0)
            child.stdout.read()
            child.wait()
            arrival.append(time.monotonic() - start)
        return arrival_row(arrival, count)

    def ffmpeg_pass(self, path, count_limit, *input_options, vf_extra=""):
        """One ffmpeg process that reads the file once and pipes the frames."""
        _, w, h, _ = probe(path)
        height = even_height(WIDTH, w, h)
        frame = WIDTH * height * 4
        vf = f"{vf_extra}scale={WIDTH}:{height}:flags=fast_bilinear"
        cmd = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", *input_options, "-i", str(path), "-map", "0:v:0",
               "-an", "-sn", "-fps_mode", "passthrough", "-vf", vf, "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]
        start = time.monotonic()
        child = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        arrival = []
        while child.stdout.read(frame):
            arrival.append(time.monotonic() - start)
            if len(arrival) >= count_limit:
                child.kill()
                break
        child.wait()
        return arrival_row(arrival, count_limit)

    def mpv_persistent(self, path, count, extra=()):
        """A second mpv kept open and told to seek, the way thumbfast does it."""
        duration, w, h, _ = probe(path)
        height = even_height(WIDTH, w, h)
        with tempfile.TemporaryDirectory() as tmp:
            sock_path, out = f"{tmp}/s", f"{tmp}/o"
            start = time.monotonic()
            mpv = subprocess.Popen(
                ["mpv", "--no-config", "--msg-level=all=no", "--idle", "--pause", "--keep-open=always", "--really-quiet",
                 "--no-terminal", "--load-scripts=no", "--osc=no", "--ytdl=no", "--no-sub", "--no-audio", "--hr-seek=no",
                 "--demuxer-readahead-secs=0", "--demuxer-max-bytes=128KiB", "--vd-lavc-skiploopfilter=all",
                 "--vd-lavc-software-fallback=1", "--vd-lavc-fast", "--vd-lavc-threads=2", "--hwdec=no",
                 f"--vf=scale=w={WIDTH}:h={height},format=bgra", "--sws-scaler=fast-bilinear", "--sws-allow-zimg=no",
                 "--ovc=rawvideo", "--of=image2", "--ofopts=update=1", f"--o={out}",
                 f"--input-ipc-server={sock_path}", *extra, "--", str(path)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            while not os.path.exists(sock_path):
                time.sleep(0.002)
                if mpv.poll() is not None or time.monotonic() - start > 10:
                    return {"failed": 1}
            sock = socket.socket(socket.AF_UNIX)
            sock.connect(sock_path)
            arrival, seen = [], 0
            times = [(i + 0.5) * duration / count for i in order_levels(count)]
            for t in times:
                sock.sendall(json.dumps({"command": ["seek", t, "absolute+keyframes"]}).encode() + b"\n")
                limit = time.monotonic() + 3
                while time.monotonic() < limit:
                    try:
                        stamp = os.stat(out).st_mtime_ns
                    except FileNotFoundError:
                        stamp = 0
                    if stamp != seen:
                        seen = stamp
                        arrival.append(time.monotonic() - start)
                        break
                    time.sleep(0.0005)
            mpv.terminate()
            mpv.wait()
        return arrival_row(arrival, count)

    # ---- groups
    def engines(self):
        names = ["h264_1080p_1h_gop10.mp4", "h264_1080p_1h_gop2.mp4", "hevc_4k10_20m_gop5.mkv", "av1_1080p_20m_gop5.mkv",
                 "vp9_1080p_20m_gop5.webm"]
        count = 30 if self.quick else 200
        for path in self.media(names):
            cold(path)
            self.save("engines", path, "libav key, cold", self.thumblab(path, "--count", count))
            self.save("engines", path, "libav key, warm", self.thumblab(path, "--count", count))
            self.save("engines", path, "libav key, 4 workers", self.thumblab(path, "--count", count, "--workers", 4))
            self.save("engines", path, "ffmpeg per thumb (today)", self.spawn_each(path, min(count, 40)))
            self.save("engines", path, "ffmpeg per thumb, 4 parallel", self.spawn_each(path, min(count, 40), jobs=4))
            self.save("engines", path, "ffmpeg keyframes pass", self.ffmpeg_pass(path, count, "-skip_frame", "nokey"))
            self.save("engines", path, "ffmpeg keyframes pass + discard",
                      self.ffmpeg_pass(path, count, "-skip_frame", "nokey", "-discard", "nokey"))
            self.save("engines", path, "second mpv (thumbfast)", self.mpv_persistent(path, min(count, 60)))
            self.save("engines", path, "libav exact", self.thumblab(path, "--count", min(count, 40), "--mode", "exact"))
            self.save("engines", path, "libav exact, 2 threads",
                      self.thumblab(path, "--count", min(count, 40), "--mode", "exact", "--threads", 2))

    def tuning(self):
        names = ["h264_1080p_1h_gop10.mp4", "h264_1080p_1h_gop2.mp4", "hevc_4k10_20m_gop5.mkv", "av1_1080p_20m_gop5.mkv"]
        count = 50 if self.quick else 300
        for path in self.media(names):
            for threads in (1, 2, 4):
                self.save("tuning", path, f"threads {threads}", self.thumblab(path, "--count", count, "--threads", threads))
            for workers in (2, 3, 4, 6):
                self.save("tuning", path, f"workers {workers}", self.thumblab(path, "--count", count, "--workers", workers))
            self.save("tuning", path, "loop filter on", self.thumblab(path, "--count", count, "--loopfilter", 1))
            for scaler in ("bilinear", "bicubic"):
                self.save("tuning", path, f"scaler {scaler}", self.thumblab(path, "--count", count, "--scaler", scaler))
            for width in (160, 240, 480):
                self.save("tuning", path, f"width {width}", self.thumblab(path, "--count", count, "--width", width))
            self.save("tuning", path, "demuxer discard", self.thumblab(path, "--count", count, "--discard", 1))

    def order(self):
        names = ["h264_1080p_1h_gop10.mp4", "h264_1080p_1h_gop2.mp4", "h264_1080p_1h_gop10.mkv", "hevc_4k10_20m_gop5.mkv"]
        count = 60 if self.quick else 400
        for path in self.media(names):
            for order in ("sorted", "rev", "levels"):
                cold(path)
                self.save("order", path, f"cold {order}", self.thumblab(path, "--count", count, "--order", order))
            for skip in ("none", "bidir", "nonref"):
                self.save("order", path, f"sweep skip {skip}",
                          self.thumblab(path, "--count", count, "--mode", "sweep", "--skip", skip, "--threads", 4))

    def edge(self):
        derived = ["h264_1080p_1h.ts", "h264_1080p_1h_frag.mp4", "h264_1080p_1h_nocues.mkv", "h264_1080p_5m_onekey.mp4"]
        count = 30 if self.quick else 100
        for path in self.media(derived):
            cold(path)
            self.save("edge", path, "libav key, cold", self.thumblab(path, "--count", count))
            self.save("edge", path, "libav exact", self.thumblab(path, "--count", 20, "--mode", "exact", "--threads", 2))
            self.save("edge", path, "sweep", self.thumblab(path, "--count", count, "--mode", "sweep", "--threads", 4))
        for path in sorted(SOURCE.iterdir()):
            if path.is_file() and path.suffix.lower() in {".mp4", ".mkv", ".mpg", ".avi", ".mov", ".webm"}:
                self.save("edge", path, "short key", self.thumblab(path, "--count", 30))
                self.save("edge", path, "short sweep", self.thumblab(path, "--count", 30, "--mode", "sweep", "--threads", 2))

    def formats(self):
        from io import BytesIO
        from PIL import Image
        names = ["h264_1080p_1h_gop10.mp4", "h264_1080p_1h_gop2.mp4", "av1_1080p_20m_gop5.mkv"]
        for path in self.media(names):
            for width in (160, 320, 480):
                with tempfile.TemporaryDirectory() as tmp:
                    self.thumblab(path, "--count", 30, "--width", width, "--dump", tmp, "--order", "sorted")
                    images = []
                    for raw in sorted(Path(tmp).iterdir()):
                        w, h = map(int, raw.stem.split("_")[1].split("x"))
                        images.append(Image.frombytes("RGBA", (w, h), raw.read_bytes()).convert("RGB"))
                for name, options in (("jpeg q50", dict(format="JPEG", quality=50)), ("jpeg q75", dict(format="JPEG", quality=75)),
                                      ("jpeg q85", dict(format="JPEG", quality=85)),
                                      ("webp q50 m0", dict(format="WEBP", quality=50, method=0)),
                                      ("webp q75 m0", dict(format="WEBP", quality=75, method=0)),
                                      ("webp q75 m4", dict(format="WEBP", quality=75, method=4)),
                                      ("png", dict(format="PNG", compress_level=1))):
                    blobs, start = [], time.perf_counter()
                    for image in images:
                        out = BytesIO()
                        image.save(out, **options)
                        blobs.append(out.getvalue())
                    encoded = time.perf_counter() - start
                    start = time.perf_counter()
                    for blob in blobs:
                        Image.open(BytesIO(blob)).load()
                    decoded = time.perf_counter() - start
                    self.save("formats", path, f"{width} {name}", {
                        "kb_each": round(sum(map(len, blobs)) / len(blobs) / 1024, 1),
                        "enc_ms": round(encoded * 1000 / len(blobs), 2), "dec_ms": round(decoded * 1000 / len(blobs), 2)})

    def priority(self):
        """Does a busy generator disturb real-time playback? mpv plays, the generator loops at each priority."""
        names = ["hevc_4k10_20m_gop5.mkv", "h264_1080p_1h_gop10.mp4"]
        seconds = 8 if self.quick else 20
        loads = {
            "no generator": None,
            "4 workers, normal": (),
            "4 workers, nice 19": ("nice", "-n", "19"),
            "4 workers, SCHED_IDLE+ionice": ("ionice", "-c", "3", "chrt", "-i", "0"),
            "1 worker, nice 19": ("nice", "-n", "19"),
            "1 worker, SCHED_IDLE+ionice": ("ionice", "-c", "3", "chrt", "-i", "0"),
        }
        for path in self.media(names):
            for label, prefix in loads.items():
                workers = 1 if label.startswith("1 ") else 4
                stop = time.monotonic() + seconds + 2
                background = None
                if prefix is not None:
                    gen = next(iter(self.media(["hevc_4k10_20m_gop5.mkv"])), path)
                    cmd = ["bash", "-c", 'while [ "$(date +%s)" -lt "$1" ]; do "${@:2}"; done', "_", str(int(time.time() + seconds + 2)),
                           *prefix, str(self.tool), str(gen), "--count", "400", "--workers", str(workers), "--mode", "exact"]
                    background = subprocess.Popen(cmd, stdout=subprocess.DEVNULL)
                row = playback_row(path, seconds)
                if background:
                    background.terminate()
                    subprocess.run(["pkill", "-f", f"{self.tool} "], check=False)
                    background.wait()
                row["measured_s"] = round(stop - time.monotonic(), 1)
                self.save("priority", path, label, row)

    def drain(self):
        """The fix for a decoder's reorder delay, plus the mpv variants that could get the same effect."""
        names = ["h264_1080p_1h_gop10.mp4", "h264_1080p_1h_gop2.mp4", "h264_1080p_1h_gop10.mkv", "h264_1080p_1h.ts",
                 "hevc_4k10_20m_gop5.mkv", "av1_1080p_20m_gop5.mkv", "vp9_1080p_20m_gop5.webm"]
        count = 40 if self.quick else 240
        for path in self.media(names):
            cold(path)
            self.save("drain", path, "cold, 1 worker", self.thumblab(path, "--count", count, "--drain", 1))
            cold(path)
            self.save("drain", path, "cold, 1 worker (again)", self.thumblab(path, "--count", count, "--drain", 1))
            self.save("drain", path, "warm, 1 worker", self.thumblab(path, "--count", count, "--drain", 1))
            self.save("drain", path, "warm, 2 workers", self.thumblab(path, "--count", count, "--drain", 1, "--workers", 2))
            self.save("drain", path, "warm, 4 workers", self.thumblab(path, "--count", count, "--drain", 1, "--workers", 4))
            self.save("drain", path, "low delay flag", self.thumblab(path, "--count", count, "--lowdelay", 1))
            self.save("drain", path, "drain + bicubic", self.thumblab(path, "--count", count, "--drain", 1, "--scaler", "bicubic"))
            self.save("drain", path, "mpv skipframe=nonkey", self.mpv_persistent(path, 60, ("--vd-lavc-skipframe=nonkey",)))
            self.save("drain", path, "mpv low_delay", self.mpv_persistent(
                path, 60, ("--vd-lavc-skipframe=nonkey", "--vd-lavc-o=flags=+low_delay")))

    def load(self):
        """How much does the generator cost a busy foreground, and how much does it still get done? The foreground is a
        6-thread software decode at full speed (the worst case for a preview), measured in frames per second."""
        names = ["hevc_4k10_20m_gop5.mkv", "h264_1080p_1h_gop2.mp4"]
        priorities = {"normal": (), "nice 19": ("nice", "-n", "19"), "nice 19+idle io": ("ionice", "-c", "3", "nice", "-n", "19"),
                      "SCHED_IDLE+idle io": ("ionice", "-c", "3", "chrt", "-i", "0")}
        seconds = 6 if self.quick else 12
        for path in self.media(names):
            self.save("load", path, "foreground alone", foreground_fps(path, seconds))
            for workers in (1, 2, 4):
                for name, prefix in priorities.items():
                    cmd = [*prefix, str(self.tool), str(path), "--count", "2000", "--drain", "1", "--workers", str(workers),
                           "--seconds", str(seconds + 2)]
                    generator = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
                    row = foreground_fps(path, seconds)
                    out = generator.communicate()[0].strip().splitlines()
                    row["thumbs_per_s"] = round(json.loads(out[-1])["thumbs"] / (seconds + 2), 1) if out else None
                    self.save("load", path, f"{workers} workers, {name}", row)
            cmd = [str(self.tool), str(path), "--count", "2000", "--drain", "1", "--workers", "1", "--seconds", str(seconds)]
            alone = json.loads(subprocess.run(cmd, capture_output=True, text=True).stdout.strip().splitlines()[-1])
            self.save("load", path, "generator alone, 1 worker", {"thumbs_per_s": round(alone["thumbs"] / seconds, 1)})

    def render(self):
        """Thumbnails beside a render at the render's default niceness: which niceness keeps them ahead of it, and what is left
        for a foreground decode (the stand-in for mpv)?"""
        path = next(iter(self.media(["h264_1080p_1h_gop2.mp4"])), None)
        seconds = 6 if self.quick else 12
        encode = ["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-stream_loop", "-1", "-i", str(path),
                  "-an", "-c:v", "libx264", "-preset", "veryfast", "-f", "null", "-"]
        for nice in (19, 9, 0):
            render = subprocess.Popen(encode)
            time.sleep(1)
            cmd = ["nice", "-n", str(nice), str(self.tool), str(path), "--count", "2000", "--drain", "1", "--workers", "3", "--seconds", str(seconds + 2)]
            generator = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
            row = foreground_fps(path, seconds)
            out = generator.communicate()[0].strip().splitlines()
            row["thumbs_per_s"] = round(json.loads(out[-1])["thumbs"] / (seconds + 2), 1) if out else None
            render.kill()
            render.wait()
            self.save("render", path, f"beside a render, thumbs at nice {nice}", row)

    # ---- media
    def make_media(self):
        base = self.dir / "h264_1080p_1h_gop10.mp4"
        if not base.exists():
            sys.exit(f"make the base videos first: {base}")
        copy = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", str(base), "-map", "0", "-c", "copy"]
        for name, extra in (("h264_1080p_1h.ts", []),
                            ("h264_1080p_1h_frag.mp4", ["-movflags", "frag_keyframe+empty_moov"]),
                            ("h264_1080p_1h_nocues.mkv", ["-live", "1"])):
            if not (self.dir / name).exists():
                run_quiet(*copy, *extra, str(self.dir / name))
                print("made", name, flush=True)


def order_levels(n):
    seen, order, parts = set(), [], 5
    while len(order) < n and parts < 8 * n:
        level = sorted({int((i + 0.5) * n / parts) for i in range(parts)} - seen)
        seen.update(level)
        order.extend(level)
        parts *= 2
    return order + [i for i in range(n) if i not in seen]


def arrival_row(arrival, wanted):
    if not arrival:
        return {"failed": 1, "wanted": wanted}
    pick = lambda i: round(arrival[min(i, len(arrival) - 1)], 4)
    return {"wanted": wanted, "done": len(arrival), "unique": len(arrival), "t_first": pick(0), "t_5": pick(4), "t_all": pick(10**9),
            "per_ms": round(1000 * arrival[-1] / len(arrival), 2)}


def playback_row(path, seconds):
    """mpv in real time without a window: frames its decoder had to drop, plus how late a 60 Hz timer fires (a stand-in for the UI)."""
    with tempfile.TemporaryDirectory() as tmp:
        sock_path = f"{tmp}/s"
        mpv = subprocess.Popen(
            ["mpv", "--no-config", "--really-quiet", "--no-terminal", "--vo=null", "--ao=null", "--hwdec=no", "--framedrop=decoder",
             f"--length={seconds}", f"--start=30", "--keep-open=yes", f"--input-ipc-server={sock_path}", str(path)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        while not os.path.exists(sock_path) and mpv.poll() is None:
            time.sleep(0.01)
        sock = socket.socket(socket.AF_UNIX)
        sock.connect(sock_path)
        sock.settimeout(1)
        late, end = [], time.monotonic() + seconds - 0.5
        while time.monotonic() < end:
            before = time.monotonic()
            time.sleep(1 / 60)
            late.append((time.monotonic() - before - 1 / 60) * 1000)
        reader = sock.makefile()
        answers = {}
        for prop in ("decoder-frame-drop-count", "estimated-frame-count"):
            sock.sendall(json.dumps({"command": ["get_property", prop], "request_id": len(answers) + 1}).encode() + b"\n")
            while True:
                try:
                    line = json.loads(reader.readline())
                except (ValueError, OSError):
                    break
                if line.get("request_id") == len(answers) + 1:
                    answers[prop] = line.get("data")
                    break
        mpv.terminate()
        mpv.wait()
    late.sort()
    return {"dropped": answers.get("decoder-frame-drop-count"), "timer_p50_ms": round(statistics.median(late), 2),
            "timer_p99_ms": round(late[int(len(late) * 0.99)], 2), "timer_max_ms": round(late[-1], 2)}


def foreground_fps(path, seconds):
    """Frames per second of a full-speed software decode, stopped after the given time."""
    cmd = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-progress", "pipe:1", "-stats_period", "0.5", "-threads", "6",
           "-ss", "30", "-t", "900", "-i", str(path), "-map", "0:v:0", "-an", "-f", "null", "-"]
    child = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    start = time.monotonic()
    frames = 0
    for line in child.stdout:
        if line.startswith("frame="):
            frames = int(line.split("=")[1])
        if time.monotonic() - start > seconds:
            break
    elapsed = time.monotonic() - start
    child.kill()
    child.wait()
    return {"fg_fps": round(frames / elapsed, 1)}


def report(args):
    rows = [json.loads(l) for l in (Path(args.dir) / "results.jsonl").open()]
    group = None
    for row in rows:
        if row["group"] != group:
            group = row["group"]
            print(f"\n== {group}")
        rest = {k: v for k, v in row.items() if k not in ("group", "media", "label", "index_entries", "open_s", "duration", "wanted")}
        print(f"{row['media'][:28]:28} {row['label']:32} {rest}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command")
    parser.add_argument("--dir", default=str(DEFAULT_DIR))
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--extra", action="append", default=[])
    args = parser.parse_args()
    lab = Lab(args)
    if args.command == "build":
        flags = subprocess.run(["pkg-config", "--cflags", "--libs", "libavformat", "libavcodec", "libswscale", "libavutil"],
                               capture_output=True, text=True, check=True).stdout.split()
        subprocess.run(["gcc", "-O2", str(HERE / "thumblab.c"), "-o", str(lab.tool), "-lpthread", *flags], check=True)
    elif args.command == "media":
        lab.make_media()
    elif args.command == "report":
        report(args)
    elif args.command == "all":
        for name in ("engines", "tuning", "order", "edge", "formats", "priority", "drain", "load", "render"):
            getattr(lab, name)()
    else:
        getattr(lab, args.command)()


if __name__ == "__main__":
    shutil.which("ffmpeg") or sys.exit("ffmpeg is missing")
    main()
