#!/usr/bin/env bash
# Encode the test clips from the Blender open-movie sources (see
# docs/performance/media.md for the URLs, licenses and SHA-256 sums).
#
#   encode.sh SRC_DIR OUT_DIR
#
# SRC_DIR holds tos/tears_of_steel_1080p.mov and
# bbb/bbb_sunflower_2160p_60fps_normal.mp4 (unzipped). Needs ffmpeg with
# libx264, libx265, libvpx, libsvtav1, libmp3lame and libopus on PATH
# (nixpkgs#ffmpeg). Run it under lock-build: it uses every core.
set -euo pipefail
src=$1
out=$2
mkdir -p "$out"
tos="$src/tos/tears_of_steel_1080p.mov"
bbb="$src/bbb/bbb_sunflower_2160p_60fps_normal.mp4"
ff=(ffmpeg -hide_banner -nostdin -y -loglevel warning -stats)

# Tears of Steel (live action, 24 fps, 1920x800 = 2.40:1 scope), from 3:00
# for 90 s. Audio: its stereo mix (MP3 in the source) as AAC or Opus.
tos_in=(-ss 180 -t 90 -i "$tos")

[ -f "$out/tos-1080p24-h264.mp4" ] || "${ff[@]}" "${tos_in[@]}" -map 0:v:0 -map 0:a:0 \
  -c:v libx264 -profile:v high -preset medium -b:v 8M -maxrate 12M -bufsize 16M -g 48 -pix_fmt yuv420p \
  -c:a aac -b:a 160k -movflags +faststart "$out/tos-1080p24-h264.mp4"

[ -f "$out/tos-1080p24-hevc10.mp4" ] || "${ff[@]}" "${tos_in[@]}" -map 0:v:0 -map 0:a:0 \
  -c:v libx265 -preset medium -profile:v main10 -pix_fmt yuv420p10le -b:v 5M -tag:v hvc1 \
  -x265-params "keyint=48:vbv-maxrate=8000:vbv-bufsize=10000:log-level=warning" \
  -c:a aac -b:a 160k -movflags +faststart "$out/tos-1080p24-hevc10.mp4"

[ -f "$out/tos-1080p24-vp9.webm" ] || "${ff[@]}" "${tos_in[@]}" -map 0:v:0 -map 0:a:0 \
  -c:v libvpx-vp9 -b:v 5M -maxrate 8M -bufsize 10M -deadline good -cpu-used 2 -row-mt 1 -tile-columns 2 -g 48 -pix_fmt yuv420p \
  -c:a libopus -b:a 128k "$out/tos-1080p24-vp9.webm"

[ -f "$out/tos-1080p24-av1.webm" ] || "${ff[@]}" "${tos_in[@]}" -map 0:v:0 -map 0:a:0 \
  -c:v libsvtav1 -preset 6 -b:v 4M -g 48 -pix_fmt yuv420p \
  -c:a libopus -b:a 128k "$out/tos-1080p24-av1.webm"

# Big Buck Bunny "sunflower" (animation, 3840x2160 at 60 fps), from 2:00 for
# 90 s. Audio: its 5.1 AC-3 track downmixed to stereo.
bbb_in=(-ss 120 -t 90 -i "$bbb")

[ -f "$out/bbb-2160p30-hevc10.mp4" ] || "${ff[@]}" "${bbb_in[@]}" -map 0:v:0 -map 0:a:1 \
  -vf fps=30 -c:v libx265 -preset fast -profile:v main10 -pix_fmt yuv420p10le -b:v 16M -tag:v hvc1 \
  -x265-params "keyint=60:vbv-maxrate=24000:vbv-bufsize=32000:log-level=warning" \
  -c:a aac -b:a 160k -ac 2 -movflags +faststart "$out/bbb-2160p30-hevc10.mp4"

[ -f "$out/bbb-2160p60-av1.webm" ] || "${ff[@]}" "${bbb_in[@]}" -map 0:v:0 -map 0:a:1 \
  -c:v libsvtav1 -preset 8 -b:v 16M -g 120 -pix_fmt yuv420p \
  -c:a libopus -b:a 128k -ac 2 "$out/bbb-2160p60-av1.webm"

# Music: Big Buck Bunny's soundtrack (the 5.1 AC-3 downmixed), 90 s from
# 2:00, as MP3 192 kbit/s at 44.1 kHz and Opus 128 kbit/s (48 kHz).
[ -f "$out/bbb-music.mp3" ] || "${ff[@]}" "${bbb_in[@]}" -map 0:a:1 -vn \
  -c:a libmp3lame -b:a 192k -ar 44100 -ac 2 "$out/bbb-music.mp3"
[ -f "$out/bbb-music.opus" ] || "${ff[@]}" "${bbb_in[@]}" -map 0:a:1 -vn \
  -c:a libopus -b:a 128k -ac 2 "$out/bbb-music.opus"

# A test pattern for the time to first frame (ttff.py): 20 s of FFmpeg's
# testsrc2 (strongly coloured everywhere) with a 440 Hz tone.
[ -f "$out/pattern-1080p24-h264.mp4" ] || "${ff[@]}" -f lavfi -i testsrc2=size=1920x1080:rate=24 \
  -f lavfi -i sine=frequency=440:sample_rate=48000 -t 20 \
  -c:v libx264 -b:v 6M -pix_fmt yuv420p -c:a aac -shortest "$out/pattern-1080p24-h264.mp4"

cd "$out" && sha256sum ./*.mp4 ./*.webm ./*.mp3 ./*.opus > SHA256SUMS
