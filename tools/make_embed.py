#!/usr/bin/env python3
"""Splice real-library assets into a gitignored local build of the Phosphor sketch.

Usage:  python tools/make_embed.py

Reads   sketches/002-phosphor/index.html        (committed; runs in synth mode)
        sketches/002-phosphor/assets/embed.json (gitignored; real covers/clips/LRC)
Writes  sketches/002-phosphor/index.local.html  (gitignored)

The committed file never contains third-party audio or artwork bytes — it runs
the procedural synth mode. The local build exercises the identical code path
(art dither, accent extraction, AnalyserNode spectrum, LRC sync) against your
real library, so neither mode can drift untested.

To regenerate assets/embed.json from a music folder, see the extract step in
git history / README (ffmpeg: cover frame, 45s AAC clip, .lrc parse).
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
