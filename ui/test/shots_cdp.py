#!/usr/bin/env python3
"""CDP screenshot shooter for the UI harness (replaces --screenshot shots).

Why: chrome --headless --screenshot --virtual-time-budget under-rasterizes
the bottom tile row of tall pages (~90px unpainted band in main_* shots).
This shooter drives headless Chrome over CDP in REAL TIME (no virtual time),
so every tile is painted before the frame is captured. Stdlib only.

Usage: python3 shots_cdp.py [scenario ...]   (default: all)
Env:   CHROME (binary), BASE (default http://127.0.0.1:8791), OUT (shots dir)
       http server serving ui/ as root must be running (ui/test/http_serve.py).
"""
import base64
import json
import os
import re
import socket
import struct
import subprocess
import sys
import time
import urllib.request

CHROME = os.environ.get(
    "CHROME", os.path.expanduser(
        "~/.cache/ms-playwright/chromium-1134/chrome-linux/chrome"))
BASE = os.environ.get("BASE", "http://127.0.0.1:8791")
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get("OUT", os.path.join(HERE, "shots"))
PORT = int(os.environ.get("CDP_PORT", "9333"))

# (url-suffix, outfile, WxH)
MAIN = "page=main&nav=left&theme={th}&mode={mo}&view=tracks&track=0&play=1"
SCENARIOS = []
for th in ("analogue", "digital"):
    for mo in ("dark", "light"):
        SCENARIOS.append((MAIN.format(th=th, mo=mo), f"main_{th}_{mo}.png", (1200, 760)))
for pos in ("left", "right", "top", "bottom", "hidden"):
    SCENARIOS.append((f"page=main&nav={pos}&theme=analogue&mode=dark&view=tracks&track=0",
                      f"main_nav_{pos}.png", (1200, 760)))
for p in ("card", "strip", "square", "lyrics"):
    for wh in ((380, 260), (900, 560), (1200, 800)):
        SCENARIOS.append((f"page=widget&w={wh[0]}&h={wh[1]}&preset={p}&theme=analogue&mode=dark&track=0",
                          f"widget_{p}_{wh[0]}x{wh[1]}.png", wh))
for mo in ("light", "dark"):
    SCENARIOS.append((f"page=widget&w=600&h=380&preset=card&theme=digital&mode={mo}&track=0",
                      f"widget_digital_{mo}_600x380.png", (600, 380)))
SCENARIOS.append(("page=widget&w=600&h=380&preset=card&theme=analogue&mode=light&track=0",
                  "widget_analogue_light_600x380.png", (600, 380)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=queue&track=0&queue=3",
                  "queue.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=settings",
                  "settings.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=1",
                  "sheet.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=pl",
                  "sheet_playlist.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&ctx=1",
                  "main_cover_ctx.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=cover",
                  "main_cover_sheet.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=search",
                  "main_cover_search.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=0&play=1",
                  "lyrics_synced.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=1&play=1",
                  "lyrics_plain.png", (1200, 760)))
SCENARIOS.append(("page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=2&play=1",
                  "lyrics_none.png", (1200, 760)))

# ---- pass 2: full required set (visual.md "Verification") ----
# sizes: the harness reads &w=&h= to size the fShot iframe; main() below
# appends them per scenario, so every shot fills its viewport.
_NP = "page=main&nav=left&view=nowplaying&track=0&play=1"
_MAIN = "page=main&nav=left&view=tracks&track=0&play=1"
# drop legacy 1200x760 main theme/mode + nav-position entries (superseded below)
SCENARIOS = [s for s in SCENARIOS if not (s[1].startswith("main_") and s[2] == (1200, 760))]
# main 4 combos at 1280x800 AND 1920x1080
for th in ("analogue", "digital"):
    for mo in ("dark", "light"):
        SCENARIOS.append((f"{_MAIN}&theme={th}&mode={mo}", f"main_{th}_{mo}.png", (1280, 800)))
        SCENARIOS.append((f"{_MAIN}&theme={th}&mode={mo}", f"main_{th}_{mo}_1920.png", (1920, 1080)))
# nav 3-state (navstate=, not the legacy nav positions)
SCENARIOS += [
    (f"{_MAIN}&theme=analogue&mode=dark", "main_nav_expanded.png", (1280, 800)),
    (f"{_MAIN}&navstate=collapsed&theme=analogue&mode=dark", "main_nav_collapsed.png", (1280, 800)),
    (f"{_MAIN}&navstate=hidden&theme=analogue&mode=dark", "main_nav_hidden.png", (1280, 800)),
    (f"{_NP}&theme=digital&mode=dark", "main_digital_dark_nowplaying.png", (1280, 800)),
    (f"{_NP}&theme=analogue&mode=light", "main_analogue_light_nowplaying.png", (1280, 800)),
]
# widget card + strip x 4 combos
for p in ("card", "strip"):
    for th in ("analogue", "digital"):
        for mo in ("dark", "light"):
            SCENARIOS.append((f"page=widget&w=600&h=380&preset={p}&theme={th}&mode={mo}&track=0",
                              f"widget_{p}_{th}_{mo}.png", (600, 380)))
# seek visualiser close-ups (harness parks the playhead at 35%)
SCENARIOS += [
    ("page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=0&play=1&seekzoom=1",
     "seek_visualiser_closeup.png", (1280, 800)),
    ("page=main&nav=left&theme=digital&mode=dark&view=nowplaying&track=0&play=1&seekzoom=1",
     "seek_gel_closeup.png", (1280, 800)),
]
# context menu + sheet in liquid glass and risograph
SCENARIOS += [
    (f"{_MAIN}&theme=digital&mode=dark&ctx=1", "menu_digital_dark.png", (1280, 800)),
    (f"{_MAIN}&theme=analogue&mode=light&ctx=1", "menu_analogue_light.png", (1280, 800)),
    (f"{_MAIN}&theme=digital&mode=dark&sheet=1", "sheet_digital_dark.png", (1280, 800)),
    (f"{_MAIN}&theme=analogue&mode=light&sheet=1", "sheet_analogue_light.png", (1280, 800)),
]


def ws_connect(url):
    assert url.startswith("ws://")
    rest = url[5:]
    hostport, path = rest.split("/", 1)
    path = "/" + path
    host, port = hostport.split(":")
    s = socket.create_connection((host, int(port)), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET {path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\n"
           f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
           "Sec-WebSocket-Version: 13\r\n\r\n")
    s.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)
    head, _, rest_ = buf.partition(b"\r\n\r\n")
    if b"101" not in head.split(b"\r\n")[0]:
        raise RuntimeError("ws upgrade failed: " + head.decode(errors="replace"))
    return s, rest_


def ws_send(s, data):
    payload = data.encode()
    header = bytearray([0x81])
    n = len(payload)
    mask = os.urandom(4)
    if n < 126:
        header.append(0x80 | n)
    elif n < 65536:
        header.append(0x80 | 126)
        header += struct.pack(">H", n)
    else:
        header.append(0x80 | 127)
        header += struct.pack(">Q", n)
    header += mask
    masked = bytearray(b ^ mask[i % 4] for i, b in enumerate(payload))
    s.sendall(bytes(header) + bytes(masked))


def ws_recv(s, timeout=30):
    s.settimeout(timeout)

    def rd(n):
        buf = b""
        while len(buf) < n:
            c = s.recv(n - len(buf))
            if not c:
                raise EOFError("ws closed")
            buf += c
        return buf

    b1, b2 = rd(2)
    ln = b2 & 0x7F
    if ln == 126:
        ln = struct.unpack(">H", rd(2))[0]
    elif ln == 127:
        ln = struct.unpack(">Q", rd(8))[0]
    payload = rd(ln) if ln else b""
    if (b2 & 0x80) == 0:
        return payload  # server frames are unmasked
    return payload  # (not expected from chrome)


class Ws:
    def __init__(self, url):
        self.s, extra = ws_connect(url)
        self.buf = extra
        self.id = 0

    def cmd(self, method, **params):
        self.id += 1
        ws_send(self.s, json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            msg = json.loads(ws_recv(self.s, timeout=120))
            if msg.get("id") == self.id:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    def drain(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            try:
                msg = json.loads(ws_recv(self.s, timeout=max(0.1, end - time.time())))
            except (socket.timeout, TimeoutError):
                break
            # ignore events


def find_target():
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/list") as r:
        for t in json.load(r):
            if t.get("type") == "page":
                return t
    raise RuntimeError("no page target")


def main():
    only = set(sys.argv[1:])
    os.makedirs(OUT, exist_ok=True)
    # ---- server + tree health check (regression guard) ----------------
    # 2026-10-01: a stale http server from a DIFFERENT checkout (ht-ui/ui)
    # squatted :8791 and a dead one left Chrome error pages in the matrix —
    # ~40 shots were invalid. Verify what is actually served before shooting.
    import hashlib
    root = os.path.dirname(HERE)          # ui/ of THIS checkout
    try:
        with urllib.request.urlopen(f"{BASE}/main.html", timeout=5) as r:
            served = r.read()
    except Exception as e:
        raise SystemExit(f"HEALTH FAIL: no harness server on {BASE} ({e}) — "
                         "start ui/test/http_serve.py from THIS checkout")
    marker = b"gel-bars"                  # present only in ht-visual/ui tree
    if marker not in served:
        raise SystemExit("HEALTH FAIL: server on "
                         f"{BASE} is serving a different ui/ tree (no '{marker.decode()}' "
                         "in main.html) — kill the squatter, restart http_serve.py here")
    with open(os.path.join(root, "main.html"), "rb") as f:
        disk = f.read()
    if hashlib.md5(served).digest() != hashlib.md5(disk).digest():
        raise SystemExit("HEALTH FAIL: served main.html != disk main.html "
                         "(stale server or wrong cwd) — restart http_serve.py")
    print(f"health: OK ({BASE} serves this checkout's ui/)")
    n = 0
    for suffix, out, (w, h) in SCENARIOS:
        if only and out not in only:
            continue
        # fresh chrome per shot: scenarios share NOTHING (localStorage,
        # service worker, GPU tile cache) — a light scenario could and did
        # re-theme later dark scenarios through persisted origin state.
        prof = f"/tmp/ht_shot_profile_{n % 3}"
        # light↔dark scenarios reused the same 3 profile dirs; persisted
        # origin state (service worker / GPU cache) re-themed later shots
        # and poisoned the mode wait (RETRY data-mode=light storms).
        # Wipe the per-shot profile so every capture starts from nothing.
        subprocess.run(["rm", "-rf", prof], check=False)
        proc = subprocess.Popen(
            [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
             f"--remote-debugging-port={PORT}", f"--window-size={w},{h + 120}",
             f"--user-data-dir={prof}", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(50):
                try:
                    find_target()
                    break
                except Exception:
                    time.sleep(0.3)
            else:
                raise RuntimeError("chrome did not come up")
            tgt = find_target()
            ws = Ws(tgt["webSocketDebuggerUrl"])
            ws.cmd("Page.enable")
            ws.cmd("Runtime.enable")
            ws.cmd("Emulation.setDeviceMetricsOverride", width=w, height=h,
                   deviceScaleFactor=1, mobile=False)
            # All scenario suffixes are query strings WITHOUT an embedded '?'
            # (the table joins params with '&'), so w/h append after the
            # single '?'. The old code computed
            #   join = "&" if "?" in suffix else "?"
            # and formatted f"...html?{suffix}{join}w={w}&h={h}", producing
            # `...mode=dark?w=1280` (double '?'): w/h were swallowed into the
            # mode param, the harness fell back to its 1200x740 default
            # iframe → the 80px black band on the right of every main shot,
            # and the invalid mode string tripped the mode-wait RETRY loop.
            assert "?" not in suffix
            shot_url = f"{BASE}/test/ui_harness.html?{suffix}&w={w}&h={h}"
            want = "light" if "mode=light" in suffix else ("dark" if "mode=dark" in suffix else None)
            wantT = "digital" if "theme=digital" in suffix else ("analogue" if "theme=analogue" in suffix else None)
            # expected sheet title, if this scenario stages one (harness poller
            # re-opens until it matches; runner must WAIT for it, or the capture
            # races a late apply pass that swapped the sheet content)
            wantSheet = None
            m2 = re.search(r"[?&]sheet=([\w-]+)", suffix)
            if m2:
                wantSheet = {"cover":"CHANGE COVER ART","search":"SEARCH COVER ART",
                             "pl":"ADD TO PLAYLIST"}.get(m2.group(1), "TRACK")
            for attempt in range(3):
                ws.cmd("Page.navigate", url=shot_url)
                time.sleep(1.2)   # boot
                ws.drain(3.0)     # settle (sheets open at ~2.6s)
                time.sleep(2.0)
                if want:
                    got = ws.cmd("Runtime.evaluate", expression=(
                        "document.querySelector('#fShot')"
                        ".contentDocument.documentElement.getAttribute('data-mode')"),
                        returnByValue=True).get("result", {}).get("value")
                    if got != want:
                        print(f"RETRY {out}: data-mode={got}, want {want}", flush=True)
                        continue
                if wantSheet:
                    gotS = ws.cmd("Runtime.evaluate", expression=(
                        "(()=>{try{return document.querySelector('#fShot')"
                        ".contentDocument.getElementById('sheetTitle').textContent+' | '+document.title}"
                        "catch(e){return ''}})()"),
                        returnByValue=True).get("result", {}).get("value")
                    if wantSheet and not str(gotS or "").startswith(wantSheet):
                        # gotS is '<sheetTitle> | <page title>'; prefix-match
                        # on the sheet title only (exact match can never pass
                        # and burned 3 retries per sheet shot)
                        print(f"RETRY {out}: sheetTitle={gotS!r}, want {wantSheet!r}*", flush=True)
                        time.sleep(1.5)
                        continue
                # guard: Chrome error page? (dead server regressed the matrix
                # with ~40 identical error-page PNGs and nobody noticed)
                pageChk = ws.cmd("Runtime.evaluate", expression=(
                    "(()=>{const F=document.querySelector('#fShot');"
                    "if(!F)return 'NOFRAME';"
                    "const D=F.contentDocument;"
                    "return JSON.stringify({t:D.title,"
                    "err:/(ERR_|is not reachable|didn.t load|This site can.t)/i.test(D.body&&D.body.innerText||'')})})()"),
                    returnByValue=True).get("result", {}).get("value")
                try:
                    pc = json.loads(pageChk)
                except Exception:
                    pc = {"t": str(pageChk)[:60], "err": True}
                if pc.get("err") or pc.get("t") in ("", None):
                    print(f"RETRY {out}: harness page looks like an error/blank page ({pc})", flush=True)
                    time.sleep(1.5)
                    continue
                # guard: fShot must actually fill the viewport (the swallowed
                # w/h bug shrank every main shot to 1200x740 in a 1280x800
                # window — an 80px black band on the right)
                geom = ws.cmd("Runtime.evaluate", expression=(
                    "(()=>{const F=document.querySelector('#fShot');if(!F)return '0x0';"
                    "const B=F.getBoundingClientRect();"
                    "return Math.round(B.width)+'x'+Math.round(B.height)})()"),
                    returnByValue=True).get("result", {}).get("value")
                if geom != f"{w}x{h}":
                    print(f"RETRY {out}: fShot geometry {geom}, want {w}x{h}", flush=True)
                    time.sleep(1.5)
                    continue
                break
            # FINAL brute-force: write attrs RIGHT BEFORE capture (app can flip after settle)
            if want or wantT:
                attrs = {}
                if want: attrs["data-mode"] = want
                if wantT: attrs["data-theme"] = wantT
                js = "(()=>{const f=document.querySelector('#fShot');if(!f)return 'NOFRAME';const D=f.contentDocument.documentElement;"
                for k,v in attrs.items():
                    js += f"D.setAttribute('{k}','{v}');"
                js += "return 'OK'})()"
                ws.cmd("Runtime.evaluate", expression=js, returnByValue=True)
            r = ws.cmd("Page.captureScreenshot", format="png")
            path = os.path.join(OUT, out)
            with open(path, "wb") as f:
                f.write(base64.b64decode(r["data"]))
            n += 1
            # post-capture health check: right-size + not an error page +
            # no unpainted black band (right edge or bottom tile row).
            try:
                from pngrow import analyze as png_analyze
                st = png_analyze(path)
                ok_size = (st["w"], st["h"]) == (w, h)
                ok_band = (not st["artifact"]) and st["bottom"] > 2
                if not ok_size or not ok_band:
                    print(f"WARN {out}: PNG health check failed: {st}", flush=True)
            except Exception as e:
                print(f"WARN {out}: png analyze error: {e}", flush=True)
            print("ok", out, flush=True)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=4)
            except Exception:
                proc.kill()
    print(f"{n} shots -> {OUT}")


if __name__ == "__main__":
    main()
