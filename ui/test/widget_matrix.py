#!/usr/bin/env python3
"""Widget lyrics scaling matrix — CDP runner (agent W).
Serves nothing itself; expects http_serve.py on 8812 serving THIS worktree's ui/.
Unique CDP port 9412. Fresh chrome profile per scenario (mode hygiene).
Scenario list: (name, preset, w, h, theme, mode, scale_mode, scale_pct, lyrics, track)
Adds `lyrics=1` -> harness opens widget lyrics pane (via iw.setCfg widgetLyrics).
"""
import base64, json, os, socket, struct, subprocess, sys, time, urllib.request

CHROME = os.environ.get("CHROME", os.path.expanduser(
    "~/.cache/ms-playwright/chromium-1134/chrome-linux/chrome"))
BASE = os.environ.get("BASE", "http://127.0.0.1:8812")
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get("OUT", os.path.join(HERE, "shots"))
CDP = int(os.environ.get("CDP_PORT", "9412"))

# (name, preset, w, h, theme, mode, scalemode, scalepct, lyrics, track)
def S(name, preset, w, h, theme="analogue", mode="dark", sm="fit", sp=100, lyr=1, trk=0):
    return (name, preset, w, h, theme, mode, sm, sp, lyr, trk)

SC = []
# lyrics preset x sizes x 2 combos
for (w, h) in ((380, 300), (380, 132), (600, 380), (900, 560), (1200, 800)):
    SC.append(S(f"widget_lyrics_{w}x{h}_analogue_dark", "lyrics", w, h))
SC.append(S("widget_lyrics_600x380_digital_light", "lyrics", 600, 380, "digital", "light"))
SC.append(S("widget_lyrics_1200x800_digital_dark", "lyrics", 1200, 800, "digital", "dark"))
# card preset
for (w, h) in ((380, 260), (600, 380), (900, 560), (1200, 800)):
    SC.append(S(f"widget_card_{w}x{h}_analogue_dark", "card", w, h))
SC.append(S("widget_card_600x380_digital_light", "card", 600, 380, "digital", "light"))
# square
for (w, h) in ((300, 400), (600, 380), (900, 560)):
    SC.append(S(f"widget_square_{w}x{h}_analogue_dark", "square", w, h))
SC.append(S("widget_square_600x380_digital_dark", "square", 600, 380, "digital", "dark"))
# strip
SC.append(S("widget_strip_380x132_analogue_dark", "strip", 380, 132))
SC.append(S("widget_strip_600x380_analogue_dark", "strip", 600, 380))
# fixed scale extremes
SC.append(S("widget_lyrics_600x380_analogue_dark_s070", "lyrics", 600, 380, sm="fixed", sp=70))
SC.append(S("widget_lyrics_600x380_analogue_dark_s240", "lyrics", 600, 380, sm="fixed", sp=200))
SC.append(S("widget_lyrics_600x380_analogue_dark_s240b", "lyrics", 600, 380, sm="fixed", sp=240))
SC.append(S("widget_card_600x380_analogue_dark_s070", "card", 600, 380, sm="fixed", sp=70))
SC.append(S("widget_card_600x380_analogue_dark_s240", "card", 600, 380, sm="fixed", sp=200))
# lyrics pane OFF (clean resize when toggled)
SC.append(S("widget_card_600x380_analogue_dark_nolyr", "card", 600, 380, lyr=0))
SC.append(S("widget_lyrics_preset_nolyr", "lyrics", 600, 380, lyr=0))

# ---- ws plumbing (same wire format as shots_cdp.py) ----
def ws_connect(url):
    rest = url[5:]; hostport, path = rest.split("/", 1); path = "/" + path
    host, port = hostport.split(":")
    s = socket.create_connection((host, int(port)), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET {path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\n"
           f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
           "Sec-WebSocket-Version: 13\r\n\r\n")
    s.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf: buf += s.recv(4096)
    head, _, _ = buf.partition(b"\r\n\r\n")
    if b"101" not in head.split(b"\r\n")[0]: raise RuntimeError("ws upgrade failed")
    return s

def ws_send(s, data):
    payload = data.encode(); header = bytearray([0x81]); n = len(payload); mask = os.urandom(4)
    if n < 126: header.append(0x80 | n)
    elif n < 65536: header.append(0x80 | 126); header += struct.pack(">H", n)
    else: header.append(0x80 | 127); header += struct.pack(">Q", n)
    header += mask
    masked = bytearray(b ^ mask[i % 4] for i, b in enumerate(payload))
    s.sendall(bytes(header) + bytes(masked))

def ws_recv(s, timeout=30):
    s.settimeout(timeout)
    def rd(n):
        buf = b""
        while len(buf) < n:
            c = s.recv(n - len(buf))
            if not c: raise EOFError
            buf += c
        return buf
    b1, b2 = rd(2)
    ln = b2 & 0x7F
    if ln == 126: ln = struct.unpack(">H", rd(2))[0]
    elif ln == 127: ln = struct.unpack(">Q", rd(8))[0]
    return rd(ln) if ln else b""

class Ws:
    def __init__(self, url):
        self.s = ws_connect(url); self.id = 0
    def cmd(self, method, **params):
        self.id += 1
        ws_send(self.s, json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            msg = json.loads(ws_recv(self.s, timeout=120))
            if msg.get("id") == self.id:
                if "error" in msg: raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})
    def drain(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            try: json.loads(ws_recv(self.s, timeout=max(0.1, end - time.time())))
            except (socket.timeout, TimeoutError, ValueError): break

def find_target():
    with urllib.request.urlopen(f"http://127.0.0.1:{CDP}/json/list") as r:
        for t in json.load(r):
            if t.get("type") == "page": return t
    raise RuntimeError("no page target")

# Runtime.evaluate snippet: layout probe inside the fShot iframe.
PROBE = r"""
(()=>{
const F=document.querySelector('#fShot');if(!F)return 'NOFRAME';
const D=F.contentDocument,W=F.contentWindow;
const q=s=>D.querySelector(s);
const stage=q('#stage');if(!stage)return 'NOSTAGE';
const r=e=>{if(!e)return null;const b=e.getBoundingClientRect();
  return {x:Math.round(b.x),y:Math.round(b.y),w:Math.round(b.width),h:Math.round(b.height)}};
const host=q('#wLyrHost'),view=q('#wLyrView'),wrap=q('#lyrWrap'),st=q('#wLyrStatus');
const line=D.querySelector('.lyric-line.active')||D.querySelector('.lyric-line');
const cs=line?F.contentWindow.getComputedStyle(line):null;
const svc=view?F.contentWindow.getComputedStyle(view):null;
const hv=host?F.contentWindow.getComputedStyle(host):null;
return JSON.stringify({
  inner:[W.innerWidth,W.innerHeight],
  s:W.W?W.W.s:null, preset:W.W?W.W.preset:null,
  host:r(host),hostDisp:hv?hv.display:null,
  view:r(view),viewOv:svc?(svc.overflowY+'/'+svc.overflowX):null,
  viewAbs:svc?svc.position:null,
  status:r(st),
  lineH:r(line),lineFS:cs?cs.fontSize:null,
  lines:D.querySelectorAll('.lyric-line').length,
  active:D.querySelector('.lyric-line.active')?1:0,
  wrapScrollH:wrap?wrap.scrollHeight:null,viewClientH:view?view.clientHeight:null,
  art:r(q('.w-art')),title:r(q('#wTitle')),seek:r(q('#wSeek')),
  transport:r(q('.w-transport')),
  ovf:(view&&(view.scrollHeight>view.clientHeight+2))?1:0
});
})()
"""

def main():
    only = set(sys.argv[1:])
    os.makedirs(OUT, exist_ok=True)
    # health: server must serve THIS worktree's ui/
    with urllib.request.urlopen(f"{BASE}/main.html", timeout=5) as r:
        served = r.read()
    with open(os.path.join(HERE, "..", "main.html"), "rb") as f:
        disk = f.read()
    if served != disk:
        raise SystemExit("HEALTH FAIL: server on 8812 != this worktree")
    print(f"health: OK ({BASE} serves this worktree's ui/)", flush=True)
    n = 0; report = {}
    for (name, preset, w, h, th, mo, sm, sp, lyr, trk) in SC:
        if only and name not in only: continue
        q = (f"page=widget&w={w}&h={h}&preset={preset}&theme={th}&mode={mo}"
             f"&track={trk}&harness=1&qa=1")
        prof = f"/tmp/htw_prof_{n % 2}"
        subprocess.run(["rm", "-rf", prof], check=False)
        proc = subprocess.Popen(
            [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
             f"--remote-debugging-port={CDP}", f"--window-size={w},{h+120}",
             f"--user-data-dir={prof}", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(50):
                try: find_target(); break
                except Exception: time.sleep(0.3)
            else: raise RuntimeError("chrome did not come up")
            tgt = find_target(); ws = Ws(tgt["webSocketDebuggerUrl"])
            ws.cmd("Page.enable"); ws.cmd("Runtime.enable")
            ws.cmd("Emulation.setDeviceMetricsOverride", width=w, height=h,
                   deviceScaleFactor=1, mobile=False)
            url = f"{BASE}/test/ui_harness.html?{q}"
            ws.cmd("Page.navigate", url=url)
            time.sleep(1.2); ws.drain(2.0); time.sleep(1.6)
            # drive: lyrics toggle + fixed scale via the widget's own setters
            drive = ""
            if lyr: drive += "W.setCfg&&W.setCfg('widgetLyrics',true,{noSave:true,force:true});"
            else: drive += "W.setCfg&&W.setCfg('widgetLyrics',false,{noSave:true,force:true});"
            if sm == "fixed":
                drive += f"W.setCfg&&W.setCfg('widgetScaleMode','fixed',{{noSave:true,force:true}});W.setCfg&&W.setCfg('widgetScale',{sp},{{noSave:true,force:true}});"
            if drive:
                js = ("(()=>{const F=document.querySelector('#fShot');if(!F)return 'NOFRAME';"
                      "const W=F.contentWindow;try{" + drive +
                      "if(W.applyStage)W.applyStage();}catch(e){return 'ERR:'+e.message}return 'OK'})()")
                ws.cmd("Runtime.evaluate", expression=js, returnByValue=True)
                time.sleep(1.2); ws.drain(1.0)
            probe = ws.cmd("Runtime.evaluate", expression=PROBE, returnByValue=True)
            val = probe.get("result", {}).get("value")
            r = ws.cmd("Page.captureScreenshot", format="png")
            path = os.path.join(OUT, name + ".png")
            with open(path, "wb") as f: f.write(base64.b64decode(r["data"]))
            try: report[name] = json.loads(val)
            except Exception: report[name] = {"raw": str(val)[:200]}
            print(f"ok {name} :: {report[name]}", flush=True)
        finally:
            proc.terminate()
            try: proc.wait(timeout=4)
            except Exception: proc.kill()
        n += 1
    with open(os.path.join(OUT, "widget_probe_report.json"), "w") as f:
        json.dump(report, f, indent=1)
    print(f"{n} shots -> {OUT}", flush=True)

if __name__ == "__main__":
    main()
