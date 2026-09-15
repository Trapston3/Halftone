# Verify UI-fix pass on the live release exe via CDP.
# Targets the NEWEST widget + main pages; runs __qa probes + screenshots.
import json, socket, sys, time, base64
try:
    from websocket import create_connection
except ImportError:
    sys.exit("need websocket-client: pip install websocket-client")

CDP = "http://127.0.0.1:9333"
import urllib.request
pages = json.loads(urllib.request.urlopen(CDP + "/json/list").read().decode())

class P:
    def __init__(self, ws_url):
        self.ws = create_connection(ws_url, timeout=30)
        self.id = 0
    def cmd(self, method, **params):
        self.id += 1
        self.ws.send(json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.id:
                return msg

def evaljs(p, expr, await_promise=False):
    r = p.cmd("Runtime.evaluate", expression=expr, awaitPromise=await_promise,
              returnByValue=True)
    v = r.get("result", {}).get("result", {})
    if v.get("subtype") == "error":
        return {"__error": v.get("description", "")[:300]}
    return v.get("value")

def pick(pages, url_end, newest=True):
    cands = [p for p in pages if p["url"].split("?")[0].endswith(url_end)]
    cands.sort(key=lambda p: p.get("id", ""), reverse=newest)
    return cands[0] if cands else None

widget = pick(pages, "tauri.localhost/")
main = pick(pages, "tauri.localhost/main.html")
out = {}

if main:
    p = P(main["webSocketDebuggerUrl"])
    p.cmd("Runtime.enable")
    # let boot scan finish
    time.sleep(2)
    out["main.state"] = evaljs(p, "window.__qa.state()")
    # 1. accent decoupling: main -> rose
    out["main.accent.before"] = evaljs(p, "__qa.accent()")
    out["main.accent.set"] = evaljs(p, "__qa.accentSet('rose')")
    # 3. lyrics: scrollable + size + manual-scroll pause + follow resume
    out["main.lyrics"] = evaljs(p, "__qa.lyr()")
    out["main.lyrWheel"] = evaljs(p, "__qa.lyrWheel()")
    # 5. widget toggle button
    out["main.widgetToggle"] = evaljs(p, "__qa.widgetToggle()", await_promise=True)
    p.cmd("Page.enable")
    shot = p.cmd("Page.captureScreenshot", format="png")
    open(r"C:/Users/traps/Projects/halftone/tools/shot_main_fix.png", "wb").write(base64.b64decode(shot["result"]["data"]))

if widget:
    q = P(widget["webSocketDebuggerUrl"])
    q.cmd("Runtime.enable")
    time.sleep(1)
    out["widget.zoom"] = evaljs(q, "__qa.zoom()")
    out["widget.accent"] = evaljs(q, "__qa.accent()")   # should still be album (mint-ish), NOT rose
    out["widget.accentMode"] = evaljs(q, "__qa.accentMode()")
    out["widget.wheelVol"] = evaljs(q, "__qa.wheelVol()", await_promise=True)
    # widget still has its own lyrics-follow (transform based)
    out["widget.lyricsOpen"] = evaljs(q, "__qa.lyrics(true)", await_promise=True)
    q.cmd("Page.enable")
    shot = q.cmd("Page.captureScreenshot", format="png")
    open(r"C:/Users/traps/Projects/halftone/tools/shot_widget_fix.png", "wb").write(base64.b64decode(shot["result"]["data"]))
    # cleanup: close lyrics again
    evaljs(q, "__qa.lyrics(false)", await_promise=True)

print(json.dumps(out, indent=1))
