#!/usr/bin/env python3
"""Single-audio protocol verification on the installed release exe."""
import json, time, urllib.request, websocket

CDP = "http://127.0.0.1:9333"

def tgt():
    out = {}
    with urllib.request.urlopen(CDP + "/json", timeout=5) as r:
        for t in json.loads(r.read().decode()):
            if t.get("type") != "page":
                continue
            u = t["url"]
            if "main.html" in u:
                out["main"] = t["webSocketDebuggerUrl"]
            elif "tauri.localhost" in u:
                out["widget"] = t["webSocketDebuggerUrl"]
    return out

class C:
    def __init__(s, url):
        s.ws = websocket.create_connection(url, timeout=25, origin="http://127.0.0.1:9333")
        s.ws.send(json.dumps({"id": 1, "method": "Runtime.enable"})); s.ws.recv(); s.n = 1
    def ev(s, e, awp=True):
        s.n += 1
        s.ws.send(json.dumps({"id": s.n, "method": "Runtime.evaluate",
                              "params": {"expression": e, "returnByValue": True, "awaitPromise": awp}}))
        while True:
            m = json.loads(s.ws.recv())
            if m.get("id") == s.n:
                if "exceptionDetails" in m["result"]:
                    return {"exc": m["result"]["exceptionDetails"].get("exception", {}).get("description", "?")[:200]}
                return m["result"].get("result", {}).get("value")

urls = tgt()
w = C(urls["widget"])
print("widget scan:", w.ev("window.__qa.scan('C:\\\\Users\\\\traps\\\\Downloads\\\\music')"))
# widget plays
w.ev("window.__qa.play()")
time.sleep(1.2)
print("widget playing:", w.ev("window.__qa.state().playing"))
# open main, load SAME track, main plays -> widget must pause
w.ev("window.__qa.openMain()")
time.sleep(2.5)
urls = tgt()
m = C(urls["main"])
print("main state:", m.ev("window.__qa.state()"))
m.ev("window.__qa.play()")
time.sleep(1.6)
mw = w.ev("window.__qa.state().playing")
mm = m.ev("window.__qa.state().playing")
print("AFTER main play -> widget playing:", mw, "| main playing:", mm)
assert mm is True, "main should play"
assert mw is False, "SINGLE-AUDIO FAILED: widget still playing"
# reverse: widget takes back
w.ev("window.__qa.play()")
time.sleep(1.6)
mw = w.ev("window.__qa.state().playing")
mm = m.ev("window.__qa.state().playing")
print("AFTER widget play -> widget playing:", mw, "| main playing:", mm)
assert mw is True and mm is False, "reverse takeover failed"
print("SINGLE-AUDIO PROTOCOL VERIFIED (both directions)")
