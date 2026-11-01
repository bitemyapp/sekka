#!/usr/bin/env bash
# Make the engine's test media: small, so they can live in the repository.
# Run from this directory with FFmpeg on PATH:
#   nix shell --inputs-from ../../../../../.. nixpkgs#ffmpeg -c ./make.sh
set -euo pipefail
ff() { ffmpeg -nostdin -hide_banner -loglevel error -y "$@"; }

# Open-GOP H.264 recovery points: seeking can issue reference diagnostics
# despite succeeding. Audio makes rate changes take the same seek path.
ff -f lavfi -i "testsrc2=size=160x90:rate=24000/1001:duration=14" \
   -f lavfi -i "anullsrc=r=48000:cl=stereo" -shortest \
   -c:v libx264 -preset veryfast \
   -x264-params keyint=48:min-keyint=48:scenecut=0:open-gop=1:ref=3:bframes=3:b-pyramid=normal \
   -pix_fmt yuv420p -c:a flac open-gop.mkv

# 2 s of H.264 (160×90, 25 fps, a keyframe a second) and AAC stereo.
ff -f lavfi -i "testsrc2=size=160x90:rate=25:duration=2" \
   -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=2" \
   -ac 2 -c:v libx264 -preset veryfast -g 25 -pix_fmt yuv420p -c:a aac -b:a 64k \
   -metadata title="Test Card" -movflags +faststart clip.mp4

# A SubRip file beside it, which the player finds and shows.
cat > clip.srt <<'SRT'
1
00:00:00,200 --> 00:00:00,900
<i>First line</i>

2
00:00:01,000 --> 00:00:01,800
Second line
SRT

# 3 s of VP9 with Opus, an embedded SubRip track and two chapters.
cat > chapters.txt <<'META'
;FFMETADATA1
title=Chaptered
[CHAPTER]
TIMEBASE=1/1000
START=0
END=1500
title=Opening
[CHAPTER]
TIMEBASE=1/1000
START=1500
END=3000
title=Closing
META
ff -f lavfi -i "testsrc2=size=128x72:rate=24:duration=3" \
   -f lavfi -i "sine=frequency=330:sample_rate=48000:duration=3" \
   -i clip.srt -i chapters.txt \
   -map 0:v -map 1:a -map 2:s -map_metadata 3 -map_chapters 3 \
   -ac 2 -c:v libvpx-vp9 -b:v 100k -deadline realtime -g 24 -c:a libopus -b:a 48k -c:s srt \
   -metadata:s:a:0 language=eng -metadata:s:s:0 language=fra -disposition:s:0 default \
   chaptered.mkv
rm chapters.txt

# 2 s of FLAC with tags and a cover picture.
ff -f lavfi -i "color=c=0x1d3557:size=64x64:duration=1" -frames:v 1 cover.png
ff -f lavfi -i "sine=frequency=523:sample_rate=44100:duration=2" -i cover.png \
   -map 0:a -map 1:v -c:a flac -c:v png -disposition:v attached_pic \
   -metadata title="Indigo" -metadata artist="The Dyers" -metadata album="Aizome" -metadata date=2026 \
   song.flac
rm cover.png
ls -l
