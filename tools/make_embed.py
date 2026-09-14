#!/usr/bin/env python3
"""DEMO-ONLY TOOLING — NOT part of the production audio pipeline.

Splices real-library assets into a gitignored local build of the Phosphor
sketch so the design demo can run against real music without shipping
copyrighted bytes. This transcodes (45s AAC excerpts) and separates cover art
by hand — both things the real app MUST NOT do. The production contract is
docs/audio-pipeline.md: FLAC in, FLAC out, single read, METADATA_BLOCK_PICTURE
for covers. The reference implementation for that path is
tools/demo_playlist.html.

Usage:  python tools/make_embed.py

Reads   sketches/002-phosphor/index.html        (committed; runs in synth mode)
        sketches/002-phosphor/assets/embed.json (gitignored; real covers/clips/LRC)
Writes  sketches/002-phosphor/index.local.html  (gitignored)

The committed file never contains third-party audio or artwork bytes — it runs
the procedural synth mode. The local build exercises the identical widget code
path (art dither, accent extraction, AnalyserNode spectrum, LRC sync) against
your real library, so neither mode can drift untested.

Regenerating assets/embed.json from a music folder (demo only!):
  ffmpeg -i track.flac -an -frames:v 1 -vf scale=288:288 cover.jpg
  ffmpeg -i track.flac -t 45 -vn -b:a 48k clip.m4a
  parse matching .lrc sidecar
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SK = os.path.join(ROOT, "sketches", "002-phosphor")
SRC = os.path.join(SK, "index.html")
EMB = os.path.join(SK, "assets", "embed.json")
DST = os.path.join(SK, "index.local.html")

html = open(SRC, encoding="utf-8").read()
tracks = json.loads(open(EMB, encoding="utf-8").read())

MARKER = "/*__EMBED__*/[]"
if MARKER not in html:
    raise SystemExit("embed marker missing from index.html — splicer needs updating")

payload = json.dumps(tracks, ensure_ascii=False, separators=(",", ":"))
open(DST, "w", encoding="utf-8").write(html.replace(MARKER, payload, 1))
print("wrote %s: %d tracks, embed %d KB" % (DST, len(tracks), len(payload) // 1024))
