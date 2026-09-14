#!/usr/bin/env python3
"""Phase-2 live verification: redesigned main + widget submenu config + queue fix."""
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
        self.ws = websocket.create_connection(url, timeout=25, origin="http://127.0.0.1:9333")
        self.ws.send(json.dumps({"id": 1, "method": "Runtime.enable"}))
        self.ws.recv()
        self.n = 1

    def eval(self, expr, await_promise=True):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": "Runtime.evaluate",
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
                    return {"exc": d.get("exception", {}).get("description", str(d))[:300]}
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

def tgt():
    out = {}
    for t in targets():
        if t.get("type") != "page":
            continue
        u = t["url"]
        if u.endswith("/main.html") or "/main.html" in u:
            out["main"] = t["webSocketDebuggerUrl"]
        elif "tauri.localhost" in u:
            out["widget"] = t["webSocketDebuggerUrl"]
    return out

def P(name):
    print("\n=== " + name + " ===", flush=True)

def j(v):
    print(json.dumps(v, indent=1)[:2200], flush=True)

def main():
    urls = tgt()
    if "widget" not in urls:
        print("NO TARGETS", urls)
        return
    w = Conn(urls["widget"])

    P("widget: boot + cfg defaults")
    j(w.eval("window.__qa.cfg()"))
    j(w.eval("window.__qa.state()"))

    P("widget: scan + load + play")
    j(w.eval("window.__qa.scan('%s')" % LIB.replace("\\", "\\\\")))
    j(w.eval("window.__qa.pause&&window.__qa.pause()"))
    time.sleep(0.3)
    j(w.eval("window.__qa.state()"))
    j(w.eval("window.__qa.load(3)"))
    time.sleep(1.5)
    j(w.eval("window.__qa.state()"))          # click/queue load => PLAYING now
    j(w.eval("window.__qa.meter()"))

    P("widget: QUEUE FIX - click a row while paused")
    j(w.eval("window.__qa.pause()"))
    time.sleep(0.4)
    j(w.eval(
        "(async function(){const row=window.el.qrows.children[7];"
        "row.click();await new Promise(r=>setTimeout(r,1500));"
        "return {clickedI:7,state:window.__qa.state()}})()"))

    P("widget: ctx menu = submenu structure")
    j(w.eval("window.__qa.ctx(120,120)"))
    j(w.eval("window.__qa.setCfg('cover','large')"))
    j(w.eval("window.__qa.artSet('real')"))
    j(w.eval("window.__qa.artSet('dither')"))
    j(w.eval("window.__qa.setCfg('grid',64)"))
    j(w.eval("window.__qa.setCfg('grid',32)"))

    P("main: open on NOW PLAYING via btnMax, real art + ambient + lyrics")
    j(w.eval("window.__qa.openMain()"))
    time.sleep(2.2)
    urls = tgt()
    if "main" not in urls:
        print("MAIN MISSING", urls)
        return
    m = Conn(urls["main"])
    j(m.eval("window.__qa.state()"))
    j(m.eval("window.__qa.artSet('real')"))
    time.sleep(0.8)
    j(m.eval("window.__qa.state()"))
    j(m.eval("window.__qa.ambientLit()"))
    j(m.eval("window.__qa.seekBar()"))
    # lyrics visible?
    j(m.eval(
        "(function(){const lw=document.getElementById('lyrWrap');"
        "return {lines:lw.children.length,"
        "first:+(lw.children[0]&&lw.children[0].dataset.t),"
        "noLrcShown:getComputedStyle(document.getElementById('npNoLrc')).display!=='none'}})()"))
    # click a lyric line seeks
    j(m.eval(
        "(async function(){const ln=document.querySelector('#lyrWrap .line');if(!ln)return {lines:0};"
        "ln.click();await new Promise(r=>setTimeout(r,300));"
        "return {seekTo:+ln.dataset.t,now:+window.el.aud.currentTime.toFixed(2)}})()"))
    # ambient pixel-audit vs playing state
    j(m.eval("window.__qa.ambientLit()"))
    j(m.eval("window.__qa.pause()"))
    time.sleep(1.2)
    j(m.eval("window.__qa.ambientLit()"))
    j(m.eval("window.__qa.play()"))
    time.sleep(1.5)

    P("main: nav surfaces (np hidden on library views)")
    j(m.eval("window.__qa.nav('tracks')"))
    j(m.eval("window.__qa.seekBar()"))
    j(m.eval("window.__qa.nav('albums')"))
    j(m.eval("window.__qa.nav('nowplaying')"))
    j(m.eval("window.__qa.seekBar()"))

    P("main: ctx submenu + widget button")
    j(m.eval("window.__qa.ctx(400,240)"))
    j(m.eval(
        "(async function(){const w=await window.__TAURI__.window.getAllWindows();"
        "return {labels:w.map(x=>x.label)}})()"))

    P("shots")
    m.eval("window.__qa.artSet('real')")
    time.sleep(0.8)
    print(m.shot(SHOT + r"\009-main-nowplaying-realart.png"))
    j(m.eval("window.__qa.ambientSet(true)"))
    time.sleep(0.5)
    j(m.eval("window.__qa.ambientLit()"))
    m.eval("window.__qa.nav('albums')")
    time.sleep(0.6)
    print(m.shot(SHOT + r"\010-main-albums-realart.png"))
    print(w.shot(SHOT + r"\011-widget-submenu-cfg.png"))

    print("\nPHASE2 DONE", flush=True)

if __name__ == "__main__":
    main()
