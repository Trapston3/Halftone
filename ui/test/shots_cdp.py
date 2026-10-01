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
    proc = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
         f"--remote-debugging-port={PORT}", "--window-size=1280,900",
         "about:blank"],
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
        n = 0
        for suffix, out, (w, h) in SCENARIOS:
            if only and out not in only:
                continue
            ws.cmd("Emulation.setDeviceMetricsOverride", width=w, height=h,
                   deviceScaleFactor=1, mobile=False)
            ws.cmd("Page.navigate", url=f"{BASE}/test/ui_harness.html?{suffix}")
            time.sleep(1.2)   # boot
            ws.drain(3.0)     # settle (sheets open at ~2.6s)
            time.sleep(2.0)
            r = ws.cmd("Page.captureScreenshot", format="png")
            path = os.path.join(OUT, out)
            with open(path, "wb") as f:
                f.write(base64.b64decode(r["data"]))
            n += 1
            print("ok", out, flush=True)
        print(f"{n} shots -> {OUT}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    main()
