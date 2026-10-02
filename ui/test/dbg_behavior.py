#!/usr/bin/env python3
"""Behavior probe v2 — respects the rebuild churn:
every sync (seekTo emits one) rebuilds lyrics DOM from main's state.
Sequence: seek -> let rebuild settle -> patch to seconds (a10b2ae contract)
-> let follow scroll -> assert; then manual scroll -> local applySync
(the REAL rebuild path) -> assert scroll preserved."""
import json, os, subprocess, sys, time, urllib.request
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from widget_matrix import Ws, find_target, CHROME

W, H = 600, 380
prof = "/tmp/htw_beh2"
subprocess.run(["rm", "-rf", prof], check=False)
proc = subprocess.Popen(
    [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
     "--remote-debugging-port=9412", f"--window-size={W},{H+120}",
     f"--user-data-dir={prof}", "about:blank"],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

JS = r"""(async()=>{
const F=document.querySelector('#fShot');const W=F.contentWindow,D=F.contentDocument;
const out={};
const view=D.getElementById('wLyrView'),wrap=D.getElementById('lyrWrap');const inview=e=>{const v=view.getBoundingClientRect(),r=e.getBoundingClientRect();
  return r.bottom>v.top+2&&r.top<v.bottom-2};
const secs=()=>{const t=[0,5.2,10.4,15.6,20.8,26];
  [...wrap.children].forEach((l,i)=>l.dataset.t=t[i]??l.dataset.t)};
/* 0. initial: active line visible after settle */
{const a=D.querySelector('.lyric-line.active');
 const v=view.getBoundingClientRect(),r=a.getBoundingClientRect();
 out.geom0={st:view.scrollTop,ch:view.clientHeight,sh:view.scrollHeight,
   vt:Math.round(v.top),vb:Math.round(v.bottom),lt:Math.round(r.top),lb:Math.round(r.bottom),
   lidx:W.S.lidx};
 out.activeVisible0=inview(a);}
/* 1. seek (viewer path, emits sync) -> rebuild settles -> seconds -> follow */
W.seekTo(24.5);
await new Promise(r=>setTimeout(r,900));      /* rebuild churn settles */
secs();
for(let i=0;i<60;i++){W.htPagePaint&&W.htPagePaint();await new Promise(r=>setTimeout(r,50))}
{const a=D.querySelector('.lyric-line.active');
 const v=view.getBoundingClientRect(),r=a.getBoundingClientRect();
 out.followGeom={vt:Math.round(v.top),vb:Math.round(v.bottom),lt:Math.round(r.top),lb:Math.round(r.bottom),st:view.scrollTop};}
out.follow={lidx:W.S.lidx,st:Math.round(view.scrollTop),
  activeVisible:inview(D.querySelector('.lyric-line.active')),
  pos:+W.posSec().toFixed(2)};
/* 2. manual scroll up, then a LOCAL applySync (the real rebuild path) —
   identical content must PRESERVE scrollTop (MutationObserver settle) */
view.scrollTop=0;W.S._lyrManual=true;
const stBefore=view.scrollTop;
W.applySync(Object.assign({},W.syncState(),{pos:W.S._pos}));
await new Promise(r=>setTimeout(r,500));
out.preserve={stBefore,stAfter:Math.round(view.scrollTop),
  kept:Math.abs(view.scrollTop-stBefore)<4,
  manualStill:W.S._lyrManual===true};
/* 3. click-to-seek with CORRECT seconds data: click line 5 (t=26) —
   far from line 1, which stays pristine for the CDP hit-test */
const l3=[...wrap.children][5];
l3.click();
await new Promise(r=>setTimeout(r,300));
out.clickSeek={t:+l3.dataset.t,pos:+W.posSec().toFixed(2),
  ok:Math.abs(W.posSec()-26)<0.8};
/* 4. wrap: widest line never overflows the view horizontally */
let maxw=0;[...wrap.children].forEach(l=>{maxw=Math.max(maxw,l.scrollWidth)});
out.noHOverflow=maxw<=view.clientWidth+2;
/* 5. hit area: line 1's top edge +2px. Arm CAPTURE listeners on BOTH
   documents (per-node listeners die in rebuild churn); the click is
   dispatched from Python after this returns. No synthetic click near
   line 1 beforehand. */
view.scrollTop=0;
await new Promise(r=>setTimeout(r,300));
{
  const w2=D.getElementById('lyrWrap');
  const l1=[...w2.children][1];
  const r0=l1.getBoundingClientRect(),fr=F.getBoundingClientRect();
  out.__hit={cx:Math.round(fr.x+r0.x+r0.width/2),cy:Math.round(fr.y+r0.top+2)};
  W.__hitLog=[];W.__iframeArm=true;
  W.document.__hitLog=[];   /* unused; keep W only */
  D.addEventListener('click',e=>{
    const l=e.target.closest&&e.target.closest('.lyric-line');
    if(l)W.__hitLog.push({t:+l.dataset.t,x:e.clientX,y:e.clientY});
  },true);
  out.hitVisible=inview(l1);
}
return JSON.stringify(out);})()"""

try:
    for _ in range(50):
        try: find_target(); break
        except Exception: time.sleep(0.3)
    tgt = find_target(); ws = Ws(tgt["webSocketDebuggerUrl"])
    ws.cmd("Page.enable"); ws.cmd("Runtime.enable")
    ws.cmd("Emulation.setDeviceMetricsOverride", width=W, height=H, deviceScaleFactor=1, mobile=False)
    ws.cmd("Page.navigate", url=f"http://127.0.0.1:8812/test/ui_harness.html?page=widget&w={W}&h={H}&preset=lyrics&theme=analogue&mode=dark&track=0&harness=1&qa=1")
    time.sleep(4.5); ws.drain(1.0)
    r = ws.cmd("Runtime.evaluate", expression=JS, returnByValue=True, awaitPromise=True)
    res = r.get("result", {})
    if "exceptionDetails" in r:
        print("PAGE ERR:", json.dumps(r)[:600])
    o = res.get("value")
    if isinstance(o, str): o = json.loads(o)
    print(json.dumps(o, indent=0)[:900])
    # empirical hit-test: document-level capture listeners (survive the
    # harness's rebuild churn, unlike per-node once-listeners)
    xy = o.pop("__hit", None)
    if xy:
        ws.cmd("Input.dispatchMouseEvent", type="mousePressed", x=xy["cx"], y=xy["cy"],
               button="left", clickCount=1)
        ws.cmd("Input.dispatchMouseEvent", type="mouseReleased", x=xy["cx"], y=xy["cy"],
               button="left", clickCount=1)
        time.sleep(0.4); ws.drain(0.5)
        chk = ws.cmd("Runtime.evaluate", expression="""
(()=>{const F=document.querySelector('#fShot');const W=F.contentWindow;
return JSON.stringify({log:W.__hitLog||[]})})()""", returnByValue=True)
        o["empiricalHit"] = json.loads(chk.get("result", {}).get("value") or "{}")
    ok = (o.get("activeVisible0") and
          o["follow"]["lidx"] >= 5 and o["follow"]["activeVisible"] and
          o["preserve"]["kept"] and o["preserve"]["manualStill"] and
          o["clickSeek"]["ok"] and o["noHOverflow"] and
          o.get("empiricalHit", {}).get("log") and
          o["empiricalHit"]["log"][0].get("t") == 5.2)
    print("BEHAVIOR:", "PASS" if ok else "FAIL")
    print(" follow:", o.get("follow"))
    print(" preserve:", o.get("preserve"))
    print(" clickSeek:", o.get("clickSeek"))
    print(" empiricalHit:", o.get("empiricalHit"))
finally:
    proc.terminate()
    try: proc.wait(timeout=4)
    except Exception: proc.kill()
