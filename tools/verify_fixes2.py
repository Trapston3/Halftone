# Round 2: accent decoupling (post-tween), real lyrics track, resize zoom, restore widget.
import json, time, base64, urllib.request, sys
from websocket import create_connection

pages = json.loads(urllib.request.urlopen("http://127.0.0.1:9333/json/list").read().decode())

class P:
    def __init__(self, ws_url):
        self.ws = create_connection(ws_url, timeout=30); self.id = 0
    def cmd(self, method, **params):
        self.id += 1
        self.ws.send(json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            m = json.loads(self.ws.recv())
            if m.get("id") == self.id: return m

def ev(p, expr, ap=False):
    r = p.cmd("Runtime.evaluate", expression=expr, awaitPromise=ap, returnByValue=True)
    v = r.get("result", {}).get("result", {})
    if v.get("subtype") == "error": return {"__error": v.get("description","")[:300]}
    return v.get("value")

def pick(url_end):
    c = [p for p in pages if p["url"].split("?")[0].endswith(url_end)]
    c.sort(key=lambda p: p["id"], reverse=True)
    return c[0] if c else None

main, widget = pick("tauri.localhost/main.html"), pick("tauri.localhost/")
mp, wp = P(main["webSocketDebuggerUrl"]), P(widget["webSocketDebuggerUrl"])
mp.cmd("Runtime.enable"); wp.cmd("Runtime.enable")
out = {}

# --- accent decoupling: tween long done by now ---
out["main.accent"] = ev(mp, "__qa.accent()")          # rose now?
out["widget.accent"] = ev(wp, "__qa.accent()")        # album red, unchanged
out["widget.accentMode"] = ev(wp, "__qa.accentMode()")

# --- find a track with synced lyrics (probe read_lyrics without loading) ---
idx = ev(mp, """(async()=>{for(let i=0;i<S.lib.length;i++){
  const L=await invoke("read_lyrics",{path:S.lib[i].path});
  if(L.length>3)return i}return -1})()""", ap=True)
out["lrcTrack"] = idx

if idx is not None and idx >= 0:
    out["load"] = ev(mp, f"__qa.load({idx})", ap=True)
    out["nav"] = ev(mp, "__qa.nav('nowplaying')")
    time.sleep(1.2)  # let a lyric frame paint
    out["lyrics"] = ev(mp, "__qa.lyr()")
    # follow: fake currentTime? Just check follow math ran: scroll near active line
    out["activeScroll"] = ev(mp, """(()=>{const a=el.lyrWrap.querySelector('.line.active');
      if(!a)return null;const v=el.npLyrView;
      const target=a.offsetTop-v.clientHeight/2+a.offsetHeight/2;
      return {scrollTop:Math.round(v.scrollTop),target:Math.round(target),
        font:getComputedStyle(a).fontSize,transform:getComputedStyle(el.lyrWrap).transform}})()""")
    # manual scroll pauses follow
    out["wheelPause"] = ev(mp, "__qa.lyrWheel()")
    # clicking a line resumes follow
    out["clickResume"] = ev(mp, """(()=>{el.lyrWrap.children[2]&&el.lyrWrap.children[2].click();
      return {manual:!!S._lyrManual,cur:el.aud.currentTime}})()""")
    out["followResumed"] = ev(mp, "S._lyrManual===false||S._lyrManual")

# --- widget resize scaling ---
out["zoom.before"] = ev(wp, "__qa.zoom()")
out["resize"] = ev(wp, """(async()=>{await curWin.setSize(new T.window.LogicalSize(600,380));
  await new Promise(r=>setTimeout(r,350));return __qa.zoom()})()""", ap=True)
out["zoom.after"] = ev(wp, """(async()=>{await curWin.setSize(new T.window.LogicalSize(400,254));
  await new Promise(r=>setTimeout(r,300));return __qa.zoom()})()""", ap=True)

# --- restore: show widget again via the main button (2nd half of toggle) ---
out["toggleBack"] = ev(mp, """(async()=>{el.btnWidget.click();
  await new Promise(r=>setTimeout(r,400));
  const w=await winByLabel("widget");
  return {visible:w?await w.isVisible():null,btnOn:el.btnWidget.classList.contains("on")}})()""", ap=True)

# --- final screenshots ---
for name, p in (("main", mp), ("widget", wp)):
    p.cmd("Page.enable")
    time.sleep(.4)
    s = p.cmd("Page.captureScreenshot", format="png")
    open(rf"C:/Users/traps/Projects/halftone/tools/shot_{name}_fix.png", "wb").write(base64.b64decode(s["result"]["data"]))
out["shots"] = "ok"
print(json.dumps(out, indent=1))
