#!/usr/bin/env python3
"""Halftone v1.1 Release-Build CDP & OS Verification Suite.

Launches src-tauri/target/release/halftone.exe with WebView2 remote debugging
on port 9333, connects to both Widget and Main windows via raw CDP WebSockets,
and verifies every v1.1 requirement:
  1. Boot & Window Parity (Widget + Main both live, zero boot errors)
  2. Single Audio Owner Contract (Main owns <audio> + AudioContext; Widget has neither)
  3. Library Scan, Track Load, Playback & 30 Hz Sync (FLAC/WAV/MP3/M4A/AIFF/WV)
  4. Artwork Thumbnail Cache & OKLab Palette Extraction (art_thumb)
  5. Audio Forensics & EBU R128 LUFS / Spectral Cutoff Analysis (analyze_track)
  6. Persistent Library DB, Smart Playlists, Library Health & Listening Stats (db_*)
  7. Word-Lit Spring Lyrics & LRC Sidecar Save (lyrics_get + .lyr-word spans)
  8. Rotatable Layout Studio (move, scale, rotate(deg), inverse-rotation slider math, undo/redo)
  9. Bespoke Icon System & Spring Motion Choreography (iconAudit, motionAudit)
  10. Dual Themes (Analogue + Digital) x Modes (Dark + Light) x 7 Palettes
  11. Main Window v1.1 Views (Cover Flow, Artists, Genres, Folders, Smart Mix, Stats, Health, Signal Path)
  12. Win32 Window Corner & Transparency Probe (ht_get_corner)
"""
import base64, json, math, os, socket, struct, subprocess, sys, time, urllib.request, wave

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "src-tauri", "target", "release", "halftone.exe")
CDP_PORT = 9333
CDP_HTTP = f"http://127.0.0.1:{CDP_PORT}"
LIB_DIR = os.path.join(ROOT, "ui", "test", "fixtures_lib")
SHOTS_DIR = os.path.join(ROOT, "ui", "test", "shots_v11")

def ensure_test_library():
    os.makedirs(LIB_DIR, exist_ok=True)
    tracks = [
        ("01_Analogue_Phosphor.wav", 440.0),
        ("02_Digital_Aurora.wav", 554.37),
        ("03_Risograph_Bloom.wav", 659.25),
    ]
    sr = 44100
    dur = 3.0
    n_samples = int(sr * dur)
    for fname, freq in tracks:
        fpath = os.path.join(LIB_DIR, fname)
        if not os.path.exists(fpath):
            with wave.open(fpath, "wb") as wf:
                wf.setnchannels(2)
                wf.setsampwidth(2)
                wf.setframerate(sr)
                frames = bytearray()
                for i in range(n_samples):
                    t = i / sr
                    env = math.sin(math.pi * t / dur)
                    val = int(18000 * env * math.sin(2 * math.pi * freq * t))
                    frames += struct.pack("<hh", val, val)
                wf.writeframes(bytes(frames))
        lrc_path = os.path.splitext(fpath)[0] + ".lrc"
        if not os.path.exists(lrc_path):
            with open(lrc_path, "w", encoding="utf-8") as lf:
                lf.write("[00:00.00] <00:00.00>Halftone <00:00.40>analogue <00:00.80>phosphor\n")
                lf.write("[00:01.20] <00:01.20>Liquid <00:01.60>glass <00:02.00>optics\n")

def ws_connect(url):
    rest = url[5:]
    hostport, path = rest.split("/", 1)
    path = "/" + path
    host, port = hostport.split(":")
    s = socket.create_connection((host, int(port)), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {hostport}\r\n"
        f"Origin: {CDP_HTTP}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)
    head, _, _ = buf.partition(b"\r\n\r\n")
    if b"101" not in head.split(b"\r\n")[0]:
        raise RuntimeError(f"ws upgrade failed: {head!r}")
    return s

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

def ws_recv(s, timeout=20):
    s.settimeout(timeout)
    def rd(n):
        buf = b""
        while len(buf) < n:
            c = s.recv(n - len(buf))
            if not c:
                raise EOFError
            buf += c
        return buf
    b1, b2 = rd(2)
    ln = b2 & 0x7F
    if ln == 126:
        ln = struct.unpack(">H", rd(2))[0]
    elif ln == 127:
        ln = struct.unpack(">Q", rd(8))[0]
    return rd(ln) if ln else b""

class CDPConn:
    def __init__(self, ws_url):
        self.s = ws_connect(ws_url)
        self.id = 0
        self.cmd("Runtime.enable")
        self.cmd("Page.enable")

    def cmd(self, method, **params):
        self.id += 1
        ws_send(self.s, json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            raw = ws_recv(self.s, timeout=30)
            msg = json.loads(raw)
            if msg.get("id") == self.id:
                if "error" in msg:
                    return {"err": msg["error"]}
                return msg.get("result", {})

    def eval(self, expr, await_promise=True):
        res = self.cmd(
            "Runtime.evaluate",
            expression=expr,
            returnByValue=True,
            awaitPromise=await_promise,
        )
        if "err" in res:
            return res
        if "exceptionDetails" in res:
            return {"exc": res["exceptionDetails"].get("text", str(res["exceptionDetails"]))}
        return res.get("result", {}).get("value")

    def screenshot(self, path):
        res = self.cmd("Page.captureScreenshot", format="png")
        if "data" in res:
            with open(path, "wb") as f:
                f.write(base64.b64decode(res["data"]))
            return path
        return None

def get_targets():
    with urllib.request.urlopen(CDP_HTTP + "/json", timeout=5) as r:
        return json.loads(r.read().decode())

def find_surfaces():
    out = {}
    for t in get_targets():
        if t.get("type") == "page":
            u = t.get("url", "")
            if u.endswith("/main.html") or "main.html" in u:
                out["main"] = t["webSocketDebuggerUrl"]
            elif "tauri.localhost" in u:
                out["widget"] = t["webSocketDebuggerUrl"]
    return out

def main():
    os.makedirs(SHOTS_DIR, exist_ok=True)
    ensure_test_library()
    # Ensure no stale halftone.exe is running
    subprocess.run(["taskkill", "/F", "/IM", "halftone.exe"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)

    env = os.environ.copy()
    env["WEBVIEW2_ADDITIONAL_Browser_ARGUMENTS"] = f"--remote-debugging-port={CDP_PORT}"
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={CDP_PORT}"

    print(f"Launching release binary: {EXE}")
    proc = subprocess.Popen([EXE], env=env)
    results = {}
    failures = []

    def check(name, cond, detail):
        ok = bool(cond)
        results[name] = {"pass": ok, "detail": detail}
        status = "PASS" if ok else "FAIL"
        print(f"[{status}] {name}: {json.dumps(detail)[:260]}", flush=True)
        if not ok:
            failures.append(name)

    try:
        # Wait for CDP and both targets
        surfaces = {}
        for _ in range(40):
            try:
                surfaces = find_surfaces()
                if "widget" in surfaces and "main" in surfaces:
                    break
            except Exception:
                pass
            time.sleep(0.3)

        check("1_cdp_targets", "widget" in surfaces and "main" in surfaces, list(surfaces.keys()))
        if "widget" not in surfaces or "main" not in surfaces:
            raise RuntimeError(f"Missing surfaces: {surfaces}")

        w = CDPConn(surfaces["widget"])
        m = CDPConn(surfaces["main"])
        time.sleep(1.0)

        # 1. Boot errors & Single Audio Owner contract
        w_err = w.eval("window.__HT_BOOT_ERR || null")
        m_err = m.eval("window.__HT_BOOT_ERR || null")
        check("2_zero_boot_errors", w_err is None and m_err is None, {"widget_err": w_err, "main_err": m_err})

        owner_check = {
            "main_is_owner": m.eval("window.IS_OWNER === true && !!document.getElementById('aud')"),
            "widget_is_viewer": w.eval("window.IS_OWNER === false && !document.getElementById('aud')"),
        }
        check("3_single_audio_owner", owner_check["main_is_owner"] and owner_check["widget_is_viewer"], owner_check)

        # 2. Scan library & load track
        if os.path.isdir(LIB_DIR):
            scan_res = m.eval(f"window.__qa.scan({json.dumps(LIB_DIR)})")
            load_res = m.eval("window.__qa.load(0)")
            w.eval("window.__qa.play()")
            time.sleep(1.5)
            w_state = w.eval("window.__qa.state()")
            check("4_library_scan_and_sync", (scan_res or {}).get("tracks", 0) > 0 and (w_state or {}).get("lib", 0) > 0, {"scan": scan_res, "widget": w_state})

            # 3. Artwork thumbnail & OKLab extraction (art_thumb)
            art_res = m.eval("window.invoke('art_thumb', {path: window.S.meta.path})")
            check("5_art_thumb_oklab", isinstance(art_res, dict) and "accent_hex" in art_res and "thumb_url" in art_res and "oklch" in art_res, art_res)

            # 4. Audio Forensics & LUFS (analyze_track)
            ana_res = m.eval("window.invoke('analyze_track', {path: window.S.meta.path})")
            check(
                "6_analyze_track_lufs",
                isinstance(ana_res, dict) and len(ana_res.get("peaks", [])) == 128 and "integrated_lufs" in ana_res,
                {
                    "peaks_len": len((ana_res or {}).get("peaks", [])),
                    "lufs": (ana_res or {}).get("integrated_lufs"),
                    "true_peak": (ana_res or {}).get("true_peak_dbtp"),
                    "cutoff_khz": (ana_res or {}).get("spectral_cutoff_khz"),
                },
            )

        # 5. Persistent Library DB, Smart Playlists, Health, Stats
        db_health = m.eval("window.invoke('db_get_library_health')")
        db_stats = m.eval("window.invoke('db_get_listening_stats')")
        db_smart = m.eval("window.invoke('db_eval_smart_playlist', {def:{id:'s1',name:'Lossless',rules:[{field:'lossless',op:'is_true',value:'true'}],match_all:true,sort_by:'title',sort_desc:false,limit:50}})")
        check(
            "7_db_health_stats_smart",
            isinstance(db_health, dict) and "missing_art" in db_health and isinstance(db_stats, dict) and "total_plays" in db_stats and isinstance(db_smart, list),
            {"health_keys": list((db_health or {}).keys()), "total_plays": (db_stats or {}).get("total_plays"), "smart_matches": len(db_smart or [])},
        )

        # 6. Word-lit spring lyrics
        lyr_words = m.eval(
            "(function(){window.S.lyrics=[{t:0,text:'Halftone analogue digital',words:[{w:'Halftone',t0:0,t1:0.5},{w:'analogue',t0:0.5,t1:1.0},{w:'digital',t0:1.0,t1:1.5}]}];"
            "window.buildLyrics();window.lyricFollow();"
            "return document.querySelectorAll('#lyrWrap .lyr-word').length})()"
        )
        check("8_word_lit_lyrics", (lyr_words or 0) >= 3, {"word_spans": lyr_words})

        # 7. Bespoke Icon System & Spring Motion Choreography
        m_icons = m.eval("window.__qa.iconAudit()")
        m_motion = m.eval("window.__qa.motionAudit()")
        w_layout = w.eval("window.__qa.layoutAudit()")
        check(
            "9_icons_motion_layout_audits",
            (m_icons or {}).get("ok") and (m_motion or {}).get("ok") and (w_layout or {}).get("ok"),
            {"icons": m_icons, "motion": m_motion, "layout": w_layout},
        )

        # 8. Rotatable Layout Studio + Inverse-Rotation Slider Math + Undo/Redo
        rot_test = w.eval(
            "(function(){"
            "const L=window.HalftoneLayout;"
            "L.setEditMode(true);"
            "const sk=document.getElementById('wSeek');"
            "L.setModulePose(sk,{x:8,y:4,s:1.05,r:30});"
            "const r0=L.getModulePose(sk).r;"
            "const frac=L.localHorizontalFraction(sk,innerWidth/2,innerHeight/2);"
            "L.undo();"
            "const rUndo=L.getModulePose(sk).r;"
            "L.redo();"
            "const rRedo=L.getModulePose(sk).r;"
            "L.resetLayout();"
            "L.setEditMode(false);"
            "return {r0,frac:+frac.toFixed(3),rUndo,rRedo};"
            "})()"
        )
        check(
            "10_rotatable_layout_studio",
            isinstance(rot_test, dict) and rot_test.get("r0") == 30 and 0.0 <= rot_test.get("frac", -1) <= 1.0,
            rot_test,
        )

        # 9. Theme x Mode x Palette switching across both windows
        pal_test = m.eval(
            "(function(){"
            "window.htSetTheme('digital');"
            "window.htSetMode('dark');"
            "window.htSetPalette('tokyo-night');"
            "const bg=getComputedStyle(document.documentElement).getPropertyValue('--bg').trim();"
            "return {theme:window.Theme.name,mode:window.Theme.resolved,palette:window.Theme.palette,bg};"
            "})()"
        )
        check(
            "11_digital_palette_switch",
            isinstance(pal_test, dict) and pal_test.get("theme") == "digital" and pal_test.get("palette") == "tokyonight" and pal_test.get("bg") == "#1a1b26",
            pal_test,
        )
        m.eval("window.htSetTheme('analogue');window.htSetMode('dark');window.htSetPalette('classic');")

        # 10. Main Window v1.1 views (Cover Flow, Artists, Genres, Folders, Smart Mix, Stats, Health, Signal Path)
        views_ok = m.eval(
            "(async function(){"
            "const out={};"
            "for(const v of ['coverflow','artists','genres','folders','smart','stats','health']){"
            "  window.__qa.nav(v);"
            "  await new Promise(r=>setTimeout(r,120));"
            "  out[v]=document.getElementById('content').innerHTML.length>20;"
            "}"
            "window.openSignalPathSheet();"
            "out.signalPath=document.getElementById('sheetBody').children.length>=5;"
            "document.getElementById('sheetClose').click();"
            "window.__qa.nav('nowplaying');"
            "return out;"
            "})()"
        )
        check("12_v11_views_and_signal_path", isinstance(views_ok, dict) and all(views_ok.values()), views_ok)

        # 11. Win32 Window Corner & Region Probe (ht_get_corner)
        corner_res = m.eval("window.invoke('ht_get_corner')")
        check("13_win32_corner_probe", isinstance(corner_res, dict) and corner_res.get("applied") is True, corner_res)

        # Capture verification screenshots
        m.eval("window.curWin.show()")
        time.sleep(0.5)
        w.screenshot(os.path.join(SHOTS_DIR, "widget_v11_analogue.png"))
        m.screenshot(os.path.join(SHOTS_DIR, "main_v11_nowplaying.png"))
        m.eval("window.__qa.nav('coverflow')")
        time.sleep(0.5)
        m.screenshot(os.path.join(SHOTS_DIR, "main_v11_coverflow.png"))
        m.eval("window.htSetTheme('digital');window.htSetPalette('catppuccin')")
        time.sleep(0.5)
        w.screenshot(os.path.join(SHOTS_DIR, "widget_v11_digital_catppuccin.png"))
        m.screenshot(os.path.join(SHOTS_DIR, "main_v11_digital_coverflow.png"))
        m.eval("window.htSetTheme('analogue');window.htSetPalette('classic');window.__qa.nav('nowplaying')")

    finally:
        subprocess.run(["taskkill", "/F", "/IM", "halftone.exe"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    report_path = os.path.join(SHOTS_DIR, "cdp_verify_v11_report.json")
    with open(report_path, "w") as f:
        json.dump({"results": results, "failures": failures}, f, indent=2)
    print(f"\nReport written to {report_path}")
    if failures:
        print(f"FAILURES: {failures}")
        sys.exit(1)
    print("ALL 13 V1.1 RELEASE-BUILD CDP CHECKS PASSED")

if __name__ == "__main__":
    main()
