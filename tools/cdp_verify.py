#!/usr/bin/env python3
"""Halftone live-verification driver: raw CDP into the WebView2 windows.

Usage: python cdp_verify.py [phases ...]   (default: all)
Connects to ws://127.0.0.1:9333, targets:
  widget = http://tauri.localhost/          main = http://tauri.localhost/main.html
Every phase prints a JSON verdict; failures never abort later phases.
"""
import base64, json, sys, time, urllib.request
import websocket

CDP = "http://127.0.0.1:9333"
LIB = r"C:\Users\traps\Downloads\music"
SHOT = r"C:\Users\traps\Projects\halftone\sketches\shots"

def targets():
    with urllib.request.urlopen(CDP + "/json", timeout=5) as r:
        return json.loads(r.read().decode())

class Conn:
    def __init__(self, url):
        self.ws = websocket.create_connection(
            url, timeout=20, origin="http://127.0.0.1:9333",
            suppress_origin=False)
        self.ws.send(json.dumps({"id": 1, "method": "Runtime.enable"}))
        self.ws.recv()
        self.n = 1

    def eval(self, expr, await_promise=True):
        self.n += 1
        self.ws.send(json.dumps({
            "id": self.n, "method": "Runtime.evaluate",
            "params": {"expression": expr, "returnByValue": True,
                        "awaitPromise": await_promise}}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.n:
                if "error" in msg:
                    return {"err": msg["error"].get("message")}
                res = msg["result"]
                if "exceptionDetails" in res:
                    d = res["exceptionDetails"]
                    return {"exc": d.get("exception", {}).get("description", str(d))}
                return res.get("result", {}).get("value")

    def shot(self, path):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": "Page.captureScreenshot",
                                 "params": {"format": "png"}}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.n:
                with open(path, "wb") as f:
                    f.write(base64.b64decode(msg["result"]["data"]))
                return path

def tgt_url():
    out = {}
    for t in targets():
        if t.get("type") == "page":
            u = t["url"]
            if u.endswith("/main.html") or u.endswith("/main.html/"):
                out["main"] = t["webSocketDebuggerUrl"]
            elif "tauri.localhost" in u:
                out["widget"] = t["webSocketDebuggerUrl"]
    return out

def phase(name):
    print("\n=== " + name + " ===", flush=True)

def jdump(v):
    print(json.dumps(v, indent=1)[:2400], flush=True)

def main():
    ph = sys.argv[1:] or ["boot", "lib", "cfg", "queue", "lyrics",
                          "resize", "main", "shots"]
    urls = tgt_url()
    if "widget" not in urls:
        print("NO WIDGET TARGET", urls)
        return
    w = Conn(urls["widget"])

    if "boot" in ph:
        phase("boot: window shape, drag API, edge with no art")
        jdump(w.eval("window.__qa.win()"))
        jdump(w.eval("window.__qa.dragAPI()"))
        jdump(w.eval("window.__qa.edgeLit()"))

    if "lib" in ph:
        phase("lib: scan 95 FLACs, load, play, art+edge dither")
        jdump(w.eval("window.__qa.scan('%s')" % LIB.replace("\\", "\\\\")))
        jdump(w.eval("window.__qa.load(0)"))
        jdump(w.eval("window.__qa.play()"))
        time.sleep(2.5)
        jdump(w.eval("window.__qa.state()"))
        jdump(w.eval("window.__qa.meter()"))
        jdump(w.eval("window.__qa.artLit()"))
        jdump(w.eval("window.__qa.edgeLit()"))
        jdump(w.eval("window.__qa.accent()"))

    if "cfg" in ph:
        phase("cfg: right-click configurator, cover size, dither scale")
        jdump(w.eval("window.__qa.ctx(120,120)"))
        jdump(w.eval("window.__qa.setCfg('cover','large')"))
        time.sleep(1.2)
        jdump(w.eval("window.__qa.win()"))
        jdump(w.eval("window.__qa.setCfg('grid',64)"))
        jdump(w.eval("window.__qa.artLit()"))
        jdump(w.eval("window.__qa.setCfg('grid',32)"))

    if "queue" in ph:
        phase("queue: side drawer + popover modes, rows")
        jdump(w.eval("window.__qa.drawer(true)"))
        jdump(w.eval("window.__qa.drawer(false)"))
        jdump(w.eval("window.__qa.setCfg('queueSide',false)"))
        jdump(w.eval("window.__qa.drawer(true)"))
        jdump(w.eval("window.__qa.drawer(false)"))
        jdump(w.eval("window.__qa.setCfg('queueSide',true)"))

    if "lyrics" in ph:
        phase("lyrics: toggle, window height, click-line seek")
        jdump(w.eval("window.__qa.lyrics(true)"))
        time.sleep(1.2)
        jdump(w.eval("window.__qa.win()"))
        jdump(w.eval(
            "(function(){const ln=document.querySelector('#lyrWrap .line');"
            "if(!ln)return {lines:document.querySelectorAll('#lyrWrap .line').length};"
            "ln.click();return {lines:document.querySelectorAll('#lyrWrap .line').length,"
            "seekTo:+ln.dataset.t,now:+window.el.aud.currentTime.toFixed(2)}})()"))
        jdump(w.eval("window.__qa.lyrics(false)"))
        time.sleep(1.0)

    if "resize" in ph:
        phase("resize: setSize -> edge + meter refit via ResizeObserver")
        jdump(w.eval(
            "(async function(){const W=window.__TAURI__.window;"
            "await W.getCurrentWindow().setSize(new W.LogicalSize(480,320));"
            "await new Promise(r=>setTimeout(r,700));"
            "const cv=window.el.edgeCv;"
            "return {w:cv.width,h:cv.height,"
            "edgeLit:window.__qa.edgeLit(),"
            "win:await window.__qa.win()}})()"))
        jdump(w.eval(
            "(async function(){const W=window.__TAURI__.window;"
            "await W.getCurrentWindow().setSize(new W.LogicalSize(400,254));"
            "await new Promise(r=>setTimeout(r,700));"
            "return window.__qa.win()})()"))

    if "main" in ph:
        phase("main: open via fixed maximize btn, parity checks")
        jdump(w.eval("window.__qa.openMain()"))
        urls = tgt_url()
        if "main" not in urls:
            print("MAIN TARGET MISSING", urls)
        else:
            m = Conn(urls["main"])
            time.sleep(0.8)
            jdump(m.eval("window.__qa.state()"))
            jdump(m.eval("window.__qa.dragAPI()"))
            jdump(m.eval("window.__qa.ctx(300,200)"))
            jdump(m.eval("window.__qa.nav('albums')"))
            jdump(m.eval("window.__qa.transportSet('shuffle')"))
            jdump(m.eval("window.__qa.transport()"))
            jdump(m.eval("window.__qa.gridSet(64)"))
            jdump(m.eval("window.__qa.gridSet(32)"))
            # close button on main HIDES (stays alive for re-show)
            jdump(m.eval(
                "(async function(){window.el.btnClose.click();"
                "await new Promise(r=>setTimeout(r,600));"
                "const W=window.__TAURI__.window;"
                "return {mainVisible:await W.getCurrentWindow().isVisible()}})()"))
            # re-show from widget to prove it stayed alive
            jdump(w.eval(
                "(async function(){const W=window.__TAURI__.window;"
                "const all=await W.getAllWindows();const m=all.find(x=>x.label==='main');"
                "if(!m)return {reopened:false};"
                "await m.show();"
                "return {mainVisible:await m.isVisible()}})()"))

    if "shots" in ph:
        phase("shots: widget edge + main player captures")
        # tidy final state: playing but collapsed widget, main on albums
        w.eval("window.__qa.setCfg('cover','small')")
        print(w.shot(SHOT + r"\007-widget-edge.png"))
        urls = tgt_url()
        if "main" in urls:
            m = Conn(urls["main"])
            m.eval("window.__qa.nav('tracks')")
            time.sleep(0.6)
            print(m.shot(SHOT + r"\008-main-parity.png"))
        jdump(w.eval("window.__qa.state()"))

    print("\nDRIVER DONE", flush=True)

if __name__ == "__main__":
    main()
