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
    n = 0
    for suffix, out, (w, h) in SCENARIOS:
        if only and out not in only:
            continue
        # fresh chrome per shot: scenarios share NOTHING (localStorage,
        # service worker, GPU tile cache) — a light scenario could and did
        # re-theme later dark scenarios through persisted origin state.
        prof = f"/tmp/ht_shot_profile_{n % 3}"
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
            join = "&" if "?" in suffix else "?"
            want = "light" if "mode=light" in suffix else ("dark" if "mode=dark" in suffix else None)
            wantT = "digital" if "theme=digital" in suffix else ("analogue" if "theme=analogue" in suffix else None)
            for attempt in range(3):
                ws.cmd("Page.navigate", url=f"{BASE}/test/ui_harness.html?{suffix}{join}w={w}&h={h}")
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
