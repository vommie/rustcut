#!/bin/bash
# Long test media for the thumbnail measurements: encodes a short noisy clip, then loops it by stream copy.
set -u
ROOT=${RUSTCUT_THUMB_LAB_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)/rustcut-testenv}
cd "${1:-$ROOT/scratch/thumbs-bench}"
SRC=$ROOT/source
F="ffmpeg -hide_banner -loglevel error -y"
$F -stream_loop 359 -i $SRC/hd_h264_1080p.mp4 -c copy -map 0:v -map 0:a? h264_1080p_1h_gop10.mp4
$F -i h264_1080p_1h_gop10.mp4 -c copy -map 0 h264_1080p_1h_gop10.mkv
nsrc="testsrc2=s=1920x1080:r=25:d=60,noise=alls=10:allf=t"
$F -f lavfi -i "$nsrc" -c:v libx264 -preset veryfast -crf 21 -g 50 -keyint_min 50 -pix_fmt yuv420p c1.mp4
$F -stream_loop 59 -i c1.mp4 -c copy h264_1080p_1h_gop2.mp4
$F -f lavfi -i "testsrc2=s=3840x2160:r=25:d=30,noise=alls=10:allf=t" -c:v libx265 -preset ultrafast -crf 26 -pix_fmt yuv420p10le -x265-params keyint=125:min-keyint=125:log-level=error c2.mkv
$F -stream_loop 39 -i c2.mkv -c copy hevc_4k10_20m_gop5.mkv
$F -f lavfi -i "$nsrc" -c:v libsvtav1 -preset 10 -crf 35 -g 125 -pix_fmt yuv420p c3.mkv
$F -stream_loop 19 -i c3.mkv -c copy av1_1080p_20m_gop5.mkv
$F -f lavfi -i "$nsrc" -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -row-mt 1 -crf 35 -b:v 0 -g 125 -keyint_min 125 c4.webm
$F -stream_loop 19 -i c4.webm -c copy vp9_1080p_20m_gop5.webm
$F -f lavfi -i "testsrc2=s=1920x1080:r=25:d=300" -c:v libx264 -preset ultrafast -crf 23 -x264-params keyint=infinite:scenecut=0 -pix_fmt yuv420p h264_1080p_5m_onekey.mp4
ls -la
