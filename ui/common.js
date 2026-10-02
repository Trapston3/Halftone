/* ============================================================
   HALFTONE shared engine — v0.2 (foundation)
   Main window = sole audio owner (one <audio>, AudioContext,
   EQ chain, analyser). Widget = pure view + controller: sends
   `halftone:cmd`, renders `halftone:sync|tick|bars`.

   LOAD-BEARING:
   - Main's close button HIDES the window; only the widget's
     close or the tray Quit ends the app.
   - Viewers never construct an AudioContext or decode.
   - Canvas drawing is THEME-AWARE: all colors come from the
     tokens theme.js caches on <html> (Theme.canvas), never
     hardcoded — light modes need it.
   - Settings persist through settings.js (backend JSON shared
     by both windows); localStorage is only a boot-time mirror.
   ============================================================ */
"use strict";
/* Test-harness adoption: the harness installs __HT_TAURI_MOCK__ on the
   iframe's INITIAL about:blank window; a real navigation replaces that
   window object, so the mock is lost unless the page pulls it from the
   parent. Same-origin in the harness (http), no-op in production. */
if(!window.__TAURI__&&!window.__HT_TAURI_MOCK__){
  try{
    const pm=window.parent&&window.parent.__HT_TAURI_MOCK__;
    if(pm){Object.defineProperty(window,"__HT_TAURI_MOCK__",{value:pm,configurable:true})}
  }catch(_){/* cross-origin parent — production */}
}
const el={};
window.el=el;
const T=window.__TAURI__||window.__HT_TAURI_MOCK__||null;
window.T=T;
const IS_OWNER=!T||!T.window||!window.IS_VIEWER;   /* widget page sets window.IS_VIEWER=true */
window.IS_OWNER=IS_OWNER;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
function invoke(cmd,args){
  args=args||{};
  if(window.__invoke&&window.__invoke._ht)return window.__invoke(cmd,args);   /* test harness mock */
  if(T&&T.core&&T.core.invoke)return T.core.invoke(cmd,args);
  return Promise.resolve(null);
}
window.invoke=invoke;
const NBARS=32;   /* spectrum band count (owner analyser + viewer bars) */
window.NBARS=NBARS;

/* ============================================================
   GLOBAL STATE
   ============================================================ */
const S={
  lib:[], root:"", i:-1, meta:null, playing:false,
  shuffle:false, repeat:"off", vol:.8,
  liked:new Set(), playlists:[],
  queue:[],               /* play queue: {path,title,artist,album,duration_s,streaminfo,format,lossless,bitrate_kbps,coverUrl} */
  queueHistory:[],
  cfg:{},                 /* merged settings mirror (populated at boot) */
  lyrics:[], lidx:-1,
  lyricsStatus:"none",    /* none|searching|sidecar|embedded|cache|lrclib|notfound|error */
  lyricsPlain:null,
  view:"nowplaying",
  drag:null, pinned:false, lyricsOpen:false,
  _img:null, _artUrl:null, _barsRcv:new Float32Array(NBARS), _pos:0,
  _lyrManual:false,       /* lyrics auto-follow paused (user is scrolling) */
  _lyrManualT:0,          /* timer id for the 3s auto-follow resume */
};
window.S=S;
const ACC={hex:[102,224,194],cur:[102,224,194],anim:false,from:null,to:null,t0:0,mode:"album",custom:null};
const BG_LUM=.07;               /* fallback; recomputed from --bg on theme change */
let themeCanvas={bg:[18,23,26],off:[35,42,46],ink:[242,232,207]};
let themeSeekStyle="led", themeVolStyle="leds", themeArtDefault="dither", themeAmbientDefault="dither";
const SLEEP={until:0,stopAfterTrack:false,timer:null};
const SWEEP={t:0,dir:0,v:0};    /* seek sweep state (drag scrub) */
window.SWEEP=SWEEP;
let AC=null,EQ_NODES=[null,null,null,null,null,null,null,null,null,null],gainIn=null,gainOut=null,analyser=null,SRC=null,MEDIA=null;
const EMA=new Float32Array(NBARS);
window.EMA=EMA;

/* ============================================================
   TINY HELPERS
   ============================================================ */
function $(id){return document.getElementById(id)}
function clamp(v,a,b){return Math.max(a,Math.min(b,v))}
function fmt(sec){sec=Math.max(0,sec|0);const m=(sec/60)|0,s=sec%60;return String(m).padStart(2,"0")+":"+String(s).padStart(2,"0")}
function esc(s){return String(s==null?"":s).replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]))}
function lum(r,g,b){return (0.2126*(r/255)+0.7152*(g/255)+0.0722*(b/255))}
function contrast(a,b){const l1=lum(...a),l2=lum(...b);const [hi,lo]=l1>l2?[l1,l2]:[l2,l1];return (hi+.05)/(lo+.05)}
function css(c,a){return a==null?`rgb(${c[0]},${c[1]},${c[2]})`:`rgba(${c[0]},${c[1]},${c[2]},${a})`}
function toast(msg,kind){
  let box=document.getElementById("htToasts");
  if(!box){box=document.createElement("div");box.id="htToasts";document.body.appendChild(box)}
  const t=document.createElement("div");
  t.className="toast"+(kind?" "+kind:"");
  t.textContent=msg;
  t.onclick=()=>dismissToast(t);
  box.appendChild(t);
  setTimeout(()=>dismissToast(t),3600);
}
function dismissToast(t){
  if(!t||t._bye)return;t._bye=true;
  t.classList.add("bye");
  setTimeout(()=>t.remove(),320);
}
window.toast=toast;
function openExternal(url){
  try{
    if(T&&T.opener&&T.opener.openUrl)return T.opener.openUrl(url);
    if(T&&T.shell&&T.shell.open)return T.shell.open(url);
  }catch(e){console.warn("openExternal",e)}
  try{window.open(url,"_blank")}catch(_){}
}
window.openExternal=openExternal;

/* ============================================================
   THEME ADAPTERS
   common.js reads every color through these. theme.js caches
   tokens on <html>; we mirror them here and re-render.
   ============================================================ */
function parseTok(name){
  const v=getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  if(!v)return null;
  let m=v.match(/rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i);
  if(m)return [+m[1],+m[2],+m[3]];
  if(/^#[0-9a-f]{6}$/i.test(v))return [parseInt(v.slice(1,3),16),parseInt(v.slice(3,5),16),parseInt(v.slice(5,7),16)];
  if(/^#[0-9a-f]{3}$/i.test(v))return [parseInt(v[1]+v[1],16),parseInt(v[2]+v[2],16),parseInt(v[3]+v[3],16)];
  return null;
}
function rgbStr(c){return c?`rgb(${c[0]|0},${c[1]|0},${c[2]|0})`:"#000"}
function windowBgColor(){
  const cs=getComputedStyle(document.body);
  return cs&&cs.backgroundColor?cs.backgroundColor:"#12171A";
}
document.addEventListener("halftone:theme",e=>{
  const d=e.detail||{};
  const t=window.Theme||{};
  if(t.canvas)themeCanvas={bg:[...t.canvas.bg],off:[...t.canvas.off],ink:[...t.canvas.ink]};
  themeSeekStyle=t.seekStyle||"led";
  themeVolStyle=t.volStyle||"leds";
  themeArtDefault=t.artDefault||"dither";
  themeAmbientDefault=t.ambientDefault||"dither";
  S.cfg.theme=d.theme||S.cfg.theme;
  S.cfg.mode=d.mode||S.cfg.mode;
  recalibrateBgLum();
  updateSwitchStyles();
  requestRedraw("theme");
});

/* ---------- accent: extraction + clamp + tween ---------- */
let bgLum=BG_LUM;
function recalibrateBgLum(){
  const bg=parseTok("--bg")||[22,24,26];
  bgLum=lum(...bg);
  if(bgLum<0.02)bgLum=0.02;
}
function clampAccent(rgb){
  let [h,s,l]=rgbToHsl(rgb);
  const C=window.Theme?themeCanvas.bg:[18,23,26];
  let ratio=contrast([rgb[0]|0,rgb[1]|0,rgb[2]|0],C);
  while(ratio<3&&l<92){l++;[rgb[0],rgb[1],rgb[2]]=hslToRgb(h,s,l);ratio=contrast([rgb[0]|0,rgb[1]|0,rgb[2]|0],C)}
  while(ratio<3&&l>8){l--;[rgb[0],rgb[1],rgb[2]]=hslToRgb(h,s,l);ratio=contrast([rgb[0]|0,rgb[1]|0,rgb[2]|0],C)}
  return {rgb:[rgb[0]|0,rgb[1]|0,rgb[2]|0]};
}
function rgbToHsl([r,g,b]){
  r/=255;g/=255;b/=255;
  const mx=Math.max(r,g,b),mn=Math.min(r,g,b);let h,s,l=(mx+mn)/2;
  if(mx===mn){h=s=0}else{
    const d=mx-mn;
    s=l>.5?d/(2-mx-mn):d/(mx+mn);
    switch(mx){
      case r:h=(g-b)/d+(g<b?6:0);break;
      case g:h=(b-r)/d+2;break;
      default:h=(r-g)/d+4;
    }
    h/=6;
  }
  return [h*360,s*100,l*100];
}
function hslToRgb(h,s,l){
  h/=360;s/=100;l/=100;
  if(s===0){const v=Math.round(l*255);return [v,v,v]}
  const hue2rgb=(p,q,t)=>{t=(t+1)%1;if(t<1/6)return p+(q-p)*6*t;if(t<1/2)return q;if(t<2/3)return p+(q-p)*(2/3-t)*6;return p};
  const q=l<.5?l*(1+s):l+s-l*s,p=2*l-q;
  return [Math.round(hue2rgb(p,q,h+1/3)*255),Math.round(hue2rgb(p,q,h)*255),Math.round(hue2rgb(p,q,h-1/3)*255)];
}
function extractAccent(img){
  try{
    const cv=document.createElement("canvas");cv.width=24;cv.height=24;
    const g=cv.getContext("2d",{willReadFrequently:true});
    g.drawImage(img,0,0,24,24);
    const d=g.getImageData(0,0,24,24).data;
    const buckets=new Map();
    for(let i=0;i<d.length;i+=4){
      const [h,s,l]=rgbToHsl([d[i],d[i+1],d[i+2]]);
      if(s<18||l<12||l>90)continue;
      const key=((h/24)|0)+":"+((s/25)|0);
      const b=buckets.get(key)||{n:0,h:0,s:0,l:0};
      b.n++;b.h+=h;b.s+=s;b.l+=l;
      buckets.set(key,b);
    }
    let best=null;
    buckets.forEach(b=>{if(!best||(b.n>best.n||(b.n===best.n&&b.s>best.s)))best=b});
    if(!best)return null;
    let [h,s,l]=[best.h/best.n,Math.min(80,best.s/best.n),clamp(best.l/best.n,45,70)];
    return hslToRgb(h,s,l);
  }catch(e){return null}
}
function setAccentRGB(rgb,inst){
  ACC.from=ACC.cur.slice();
  ACC.to=rgb;
  ACC.t0=performance.now();
  if(inst){ACC.cur=[...rgb];ACC.anim=false;applyAccentVars()}
  else ACC.anim=true;
}
function applyAccentVars(){
  const c=ACC.cur.map(v=>v|0);
  document.documentElement.style.setProperty("--accent",`rgb(${c[0]},${c[1]},${c[2]})`);
  document.documentElement.style.setProperty("--accent-22",`rgba(${c[0]},${c[1]},${c[2]},.22)`);
  document.documentElement.style.setProperty("--accent-12",`rgba(${c[0]},${c[1]},${c[2]},.12)`);
  document.dispatchEvent(new CustomEvent("halftone:accent"));
}
function tickAccent(now){
  if(!ACC.anim)return false;
  const p=clamp((now-ACC.t0)/500,0,1);
  for(let k=0;k<3;k++)ACC.cur[k]=ACC.from[k]+(ACC.to[k]-ACC.from[k])*p;
  applyAccentVars();
  if(p>=1)ACC.anim=false;
  return true;
}
function accMode(){return ACC.mode}
function setAccentMode(m){
  ACC.mode=["album","mint","sky","violet","rose","amber","red","custom"].includes(m)?m:"album";
  const preset={mint:[102,224,194],sky:[110,197,232],violet:[167,139,250],rose:[242,125,160],amber:[224,185,102],red:[224,102,102]};
  if(ACC.mode==="custom"&&ACC.custom)setAccentRGB(clampAccent(ACC.custom).rgb);
  else if(preset[ACC.mode])setAccentRGB(clampAccent(preset[ACC.mode]).rgb);
  else if(S._img)setAccentRGB(clampAccent(extractAccent(S._img)||preset.mint).rgb);
  else setAccentRGB(preset.mint);
  window.__ACC_SILENT=true;
  try{window.htSet&&window.htSet("accent",ACC.mode,{noSave:false,force:true})}finally{window.__ACC_SILENT=false}
}
window.setAccentMode=setAccentMode;
function setCustomAccent(c){
  ACC.custom=hexToRgb(c);
  ACC.mode="custom";
  setAccentRGB(clampAccent(ACC.custom).rgb);
}
window.setCustomAccent=setCustomAccent;
function hexToRgb(h){if(/^#[0-9a-f]{6}$/i.test(h))return [parseInt(h.slice(1,3),16),parseInt(h.slice(3,5),16),parseInt(h.slice(5,7),16)];return [102,224,194]}

/* ============================================================
   DITHER / DRAW HELPERS  (theme-token driven)
   fitCanvas: backing store from ResizeObserver
   devicePixelContentBoxSize (fallback rect*dpr) — correct under
   the widget stage transform, never clientWidth.
   drawDither: integer device-pixel cells, backing = N*cell,
   one putImageData per frame, cached per (image,N,accent,size,
   theme) — repaints only when a key changes.
   ============================================================ */
const _fitCache=new WeakMap();
function fitCanvas(cv){
  if(!cv)return 0;
  const cached=_fitCache.get(cv);
  if(cached&&cached.w&&cv.width!==undefined){
    if(cv.width!==cached.w||cv.height!==cached.h){cv.width=cached.w;cv.height=cached.h}
    return cached.w;
  }
  const dpr=window.devicePixelRatio||1;
  const r=cv.getBoundingClientRect();
  const w=Math.max(2,Math.round(r.width*dpr));
  const h=Math.max(2,Math.round(r.height*dpr));
  if(cv.width!==w||cv.height!==h){cv.width=w;cv.height=h}
  return w;
}
function observeCanvas(cv,onsize){
  if(!cv||_fitCache.get(cv))return;
  _fitCache.set(cv,{w:0,h:0});
  if(typeof ResizeObserver!=="function")return;
  const ro=new ResizeObserver(es=>{
    for(const e of es){
      const box=e.devicePixelContentBoxSize&&e.devicePixelContentBoxSize[0];
      let w,h;
      if(box){w=box.inlineSize;h=box.blockSize}
      else{const r=cv.getBoundingClientRect();const dpr=window.devicePixelRatio||1;
        w=Math.max(2,Math.round(r.width*dpr));h=Math.max(2,Math.round(r.height*dpr))}
      _fitCache.set(cv,{w,h});
      onsize&&onsize();
    }
  });
  ro.observe(cv);
}
window.observeCanvas=observeCanvas;
const BAYER4=[[0,8,2,10],[12,4,14,6],[3,11,1,9],[15,7,13,5]].map(r=>r.map(v=>(v+.5)/16));
/* per-canvas dither cache: the old implementation kept a SINGLE shared
   {_k,_cv} slot, so calling drawDither on a second canvas (the widget art,
   an album card, np) evicted the only slot and made every other canvas
   redraw on its next paint even when nothing about it had changed — a
   latent "last canvas wins" bug. Keyed per-canvas (WeakMap) so N canvases
   cache independently and correctly. */
const ditherCache=new WeakMap();
/* canvases we've already attached a resize watcher to (brief: a canvas
   whose box is resized — window resize, widget stage rescale (T4), album
   grid reflow — must redraw at the NEW backing size. Previously nothing
   called drawDither again after a resize, so the old bitmap just got
   stretched/squashed by the browser -> blurry, warped "messed up" dither
   until some unrelated event (track/theme change) happened to repaint
   it. observeCanvas() existed but was never wired to anything -- wire it
   here so every dithered canvas self-heals on resize. */
const _ditherObserved=new WeakSet();
function ditherKey(img,N,cell,W,H){
  /* ink state included: riso light prints art in the spot ink, others in
     the accent — a theme/mode switch must invalidate the cache */
  let inkSw="0";
  try{inkSw=getComputedStyle(document.documentElement).getPropertyValue("--dither-ink").trim()==="1"?"1":"0"}catch(_){}
  return [img?img.src.slice(-48):"none",N,cell,W,H,ACC.cur.map(v=>v|0).join(","),themeName(),themeMode(),(S.cfg&&S.cfg.grid)||32,inkSw].join("|");
}
function themeName(){return (window.Theme&&window.Theme.name)||"analogue"}
function themeMode(){return (window.Theme&&window.Theme.resolved)||"dark"}
function drawDither(cv,img,opts){
  opts=opts||{};
  if(!cv)return;
  /* self-heal on resize: attach once per canvas, redraw at the new size
     using whatever image was last drawn into it (a resize never changes
     which track's art is showing, only how big the backing store must
     be) */
  if(!_ditherObserved.has(cv)){
    _ditherObserved.add(cv);
    observeCanvas(cv,()=>{if(cv._img)drawDither(cv,cv._img,{force:true})});
  }
  fitCanvas(cv);
  const g=cv.getContext("2d");
  if(!g)return;
  const W=cv.width,H=cv.height;
  if(!img||(!img.naturalWidth&&!img.naturalHeight)){g.clearRect(0,0,W,H);cv._img=null;return}
  cv._img=img;
  const density=(S.cfg&&S.cfg.grid)||32;
  const cell=Math.max(2,Math.round(W/density));
  const N=Math.max(2,Math.floor(W/cell));
  const M=Math.max(1,Math.round(H/cell));
  /* BACKING = N*cell exactly (brief T5); re-derive W from N*cell */
  const BW=N*cell,BH=M*cell;
  if(cv.width!==BW||cv.height!==BH){cv.width=BW;cv.height=BH}
  /* fitCanvas enforces css*dpr (e.g. 137) while the dither needs N*cell
     (136): on the next loop tick fitCanvas would resize 136->137, WIPING
     the canvas, and the ditherCache key (unchanged) would skip the
     repaint — blank art forever. Align the fit cache with the dither
     backing so the two stops fighting. */
  _fitCache.set(cv,{w:BW,h:BH});
  const key=ditherKey(img,N,cell,BW,BH);
  if(!opts.force&&ditherCache.get(cv)===key)return;
  ditherCache.set(cv,key);
  console.log("[dither] backing="+BW+"x"+BH+" N="+N+" cell="+cell+" N*cell="+(N*cell)+" (exact="+((BW===N*cell&&BH===M*cell)?"YES":"NO")+")");
  g.imageSmoothingEnabled=false;
  /* ink color: risograph light prints dither art in the spot ink,
     not the mint accent (theme sets --dither-ink: 1) */
  let fg=ACC.cur;
  try{
    if(getComputedStyle(document.documentElement).getPropertyValue("--dither-ink").trim()==="1")
      fg=parseColor(getComputedStyle(document.documentElement).getPropertyValue("--canvas-ink"))||fg;
  }catch(_){}
  const bg=themeCanvas.bg;
  /* downsample to N×M once, threshold with a 4×4 Bayer matrix */
  const tmp=drawDither._tmp||(drawDither._tmp=document.createElement("canvas"));
  if(tmp.width!==N||tmp.height!==M){tmp.width=N;tmp.height=M}
  const tg=tmp.getContext("2d",{willReadFrequently:true});
  tg.imageSmoothingEnabled=true;
  tg.clearRect(0,0,N,M);
  tg.drawImage(img,0,0,N,M);
  const src=tg.getImageData(0,0,N,M).data;
  const out=g.createImageData(BW,BH);
  const od=out.data;
  for(let y=0;y<BH;y++){
    const ci=(y/cell)|0;
    const rowOff=ci*N;
    for(let x=0;x<BW;x++){
      const pi=(y*BW+x)*4;
      const cj=(x/cell)|0;
      const si=((rowOff+cj))*4;
      /* lum() already returns 0..1 (it /255s internally) */
      const l=lum(src[si],src[si+1],src[si+2]);
      const thr=BAYER4[y&3][x&3];
      const on=l>thr;
      od[pi]=on?fg[0]:bg[0];od[pi+1]=on?fg[1]:bg[1];od[pi+2]=on?fg[2]:bg[2];od[pi+3]=255;
    }
  }
  g.putImageData(out,0,0);
}
function drawEdge(cv,img){
  if(!cv||!cv.parentElement)return;
  fitCanvas(cv);
  const g=cv.getContext("2d");
  if(!g)return;
  const W=cv.width,H=cv.height;
  g.clearRect(0,0,W,H);
  const cell=4;
  g.fillStyle=css(ACC.cur,.85);
  for(let y=0;y<H;y+=cell*2)for(let x=0;x<W;x+=cell*2){
    const edge=Math.min(x,W-x,y,H-y);
    if(edge<26&&((x/cell+y/cell)%3===0))g.fillRect(x,y,cell,cell);
  }
}
function drawMeter(cv,h){
  if(!cv)return;
  fitCanvas(cv);
  const g=cv.getContext("2d");if(!g)return;
  const W=cv.width,H=cv.height;
  g.clearRect(0,0,W,H);
  const B=S._barsRcv||new Float32Array(NBARS);
  const segW=Math.max(3,Math.round(W/60));
  const gap=2;
  const n=Math.max(1,Math.floor(W/(segW+gap)));
  for(let i=0;i<n;i++){
    const v=B[Math.floor(i/n*NBARS)]||0;
    const litH=Math.round(v*H);
    const x=i*(segW+gap);
    /* unlit rail (theme canvas-off), lit portion accent */
    g.fillStyle=css(themeCanvas.off,.9);
    g.fillRect(x,0,segW,H);
    if(litH>0){g.fillStyle=css(ACC.cur,.95);g.fillRect(x,H-litH,segW,litH)}
  }
}
/* ============================================================
   SEEK VISUALISER — the v0.1.2 signature LED dot-matrix strip.
   NBARS frequency columns ride the analyser; each column is a
   vertical ladder of dots (4px pitch). Played columns are lit at
   full accent, unplayed dim; the sweep flash follows the drag;
   the playhead is a tall ink marker with a notch.
   ============================================================ */
const SEEK_ROW_PITCH=4;
function drawSeekLedShared(cv,p){
  if(!cv)return;
  const box=cv.parentElement,W=box?box.clientWidth:cv.clientWidth;
  const H=cv.clientHeight;
  if(!W||!H)return;
  const dpr=Math.min(2,devicePixelRatio||1);
  if(cv.width!==Math.round(W*dpr)||cv.height!==Math.round(H*dpr)){cv.width=Math.round(W*dpr);cv.height=Math.round(H*dpr)}
  const g=cv.getContext("2d");if(!g)return;
  g.setTransform(dpr,0,0,dpr,0,0);
  g.clearRect(0,0,W,H);
  const spec=S._barsRcv||new Float32Array(NBARS);
  const gap=2,cw=Math.max(2,(W-gap*(NBARS-1))/NBARS);
  const rows=Math.max(2,Math.floor((H-2)/SEEK_ROW_PITCH));
  const dur=durSec();
  const prog=dur>0?posSec()/dur:0;
  const dragP=S.drag!=null?S.drag.p:prog;
  const [ar,ag,ab]=ACC.cur;
  const durS=dur||1;
  for(let i=0;i<NBARS;i++){
    const segT=(i+.5)/NBARS*durS;
    const played=(i+.5)/NBARS<dragP;
    const lit=Math.round(Math.max(.08,(spec[i]||0))*rows);
    let alpha=played?1:.22;
    if(S.drag!=null){
      const dist=(segT-(S.drag.t!=null?S.drag.t:0))/durS*NBARS;
      if(dist>0){const decay=Math.exp(-dist*.14);alpha=.22+.6*decay}
      else alpha=1;
    }
    for(let r=0;r<rows;r++){
      const y=H-2-(r+1)*SEEK_ROW_PITCH;
      if(r<lit){
        g.fillStyle=`rgba(${ar},${ag},${ab},${alpha})`;
      }else{
        g.fillStyle=played?`rgba(${ar},${ag},${ab},.08)`:css(themeCanvas.off,.9);
      }
      g.fillRect(i*(cw+gap),y,cw,3);
    }
    /* sweep flash column while scrubbing */
    if(S.drag!=null&&Math.abs(segT-(S.drag.t!=null?S.drag.t:0))<durS/NBARS){
      g.fillStyle="rgba(255,255,255,.85)";
      g.fillRect(i*(cw+gap),H-2-rows*SEEK_ROW_PITCH,cw,rows*SEEK_ROW_PITCH-1);
    }
  }
  /* playhead: ink marker + notch */
  const px=dragP*W;
  g.fillStyle=css(themeCanvas.ink,.9);
  g.fillRect(px-.75,0,1.5,H);
  g.beginPath();g.moveTo(px-3,0);g.lineTo(px+3,0);g.lineTo(px,4);g.closePath();g.fill();
}
function drawAmbient(cv,now){
  if(!cv)return;
  fitCanvas(cv);
  const g=cv.getContext("2d");if(!g)return;
  const W=cv.width,H=cv.height;
  g.clearRect(0,0,W,H);
  const B=S._barsRcv||new Float32Array(NBARS);
  const cx=W/2,cy=H*.42;
  const rMin=Math.min(W,H)*.18;
  for(let i=0;i<NBARS;i++){
    const a=(i/NBARS)*Math.PI*2+now*.05;
    const v=B[i]||0;
    const r=rMin*(1.25+v*1.6);
    const x=cx+Math.cos(a)*r,y=cy+Math.sin(a)*r*.8;
    const s=Math.max(2,rMin*.06*(0.5+v*2));
    g.fillStyle=css(ACC.cur,.10+v*.5);
    g.fillRect(x-s/2,y-s/2,s,s);
  }
}
function drawHalo(cv,now){
  if(!cv)return;
  fitCanvas(cv);
  const g=cv.getContext("2d");if(!g)return;
  const W=cv.width,H=cv.height;
  g.clearRect(0,0,W,H);
  const grd=g.createRadialGradient(W/2,H*.36,10,W/2,H*.36,Math.max(W,H)*.55);
  const [r,gg,b]=ACC.cur;
  const pulse=.16+.04*Math.sin(now*.8);
  grd.addColorStop(0,`rgba(${r},${gg},${b},${pulse})`);
  grd.addColorStop(1,"rgba(0,0,0,0)");
  g.fillStyle=grd;g.fillRect(0,0,W,H);
}
function sweepTick(){
  if(SWEEP.dir!==0){
    SWEEP.v=clamp(SWEEP.v+SWEEP.dir*.016,0,1);
    if(SWEEP.v<=0||SWEEP.v>=1)SWEEP.dir=0;
  }
}

/* ============================================================
   UPDATE SWITCH ATTRIBUTES FROM THEME TOKENS
   .seek[data-seek], .vol[data-vol], art default, ambient default
   ============================================================ */
function updateSwitchStyles(){
  document.querySelectorAll(".seek").forEach(sk=>sk.setAttribute("data-seek",themeSeekStyle));
  document.querySelectorAll(".vol").forEach(vb=>vb.setAttribute("data-vol",themeVolStyle));
  if(window.htSettings){
    /* art style "theme" follows --art-default; explicit value wins */
    if(S.cfg.art==="theme"||S.cfg.art==null)S.cfg.art=themeArtDefault;
    if(S.cfg.ambient==="theme"||S.cfg.ambient==null)S.cfg.ambient=themeAmbientDefault;
  }
}

/* ============================================================
   AUDIO OWNER (main only)
   ============================================================ */
function ensureAudio(){
  if(!IS_OWNER||AC)return;
  try{
    AC=new (window.AudioContext||window.webkitAudioContext)();
    const media=AC.createMediaElementSource(document.getElementById("aud"));
    gainIn=AC.createGain();
    EQ_NODES=EQ_NODES.map(()=>{const f=AC.createBiquadFilter();f.type="peaking";f.Q.value=1.1;return f});
    gainOut=AC.createGain();
    analyser=AC.createAnalyser();analyser.fftSize=256;analyser.smoothingTimeConstant=.82;
    let prev=media;
    prev.connect(gainIn);prev=gainIn;
    EQ_NODES.forEach(f=>{prev.connect(f);prev=f});
    prev.connect(gainOut);prev=gainOut;
    prev.connect(analyser);analyser.connect(AC.destination);
    MEDIA=media;
    applyEqGains();
  }catch(e){console.warn("audio graph",e)}
}
function reconnectEq(){
  if(!AC)return;
  try{
    try{gainIn.disconnect()}catch(_){}
    EQ_NODES.forEach(f=>{try{f.disconnect()}catch(_){}});
    try{gainOut.disconnect()}catch(_){}
    let prev=MEDIA;prev.connect(gainIn);prev=gainIn;
    EQ_NODES.forEach(f=>{prev.connect(f);prev=f});
    prev.connect(gainOut);prev=gainOut;
    prev.connect(analyser);analyser.connect(AC.destination);
  }catch(e){console.warn("reconnectEq",e)}
}
function applyEqGains(){
  if(!AC)return;
  const eq=S.cfg.eq||{on:false,pre:0,bands:new Array(10).fill(0)};
  const on=eq.on!==false;
  EQ_NODES.forEach((f,k)=>{f.gain.value=on?(eq.bands[k]||0):0});
  if(gainIn)gainIn.gain.value=1;
  if(gainOut)gainOut.gain.value=on?Math.pow(10,((eq.pre||0))/20):1;
  document.dispatchEvent(new CustomEvent("halftone:eq"));
}
window.applyEqGains=applyEqGains;
window.reconnectEq=reconnectEq;
function listSinks(){
  return invoke("list_sinks").then(r=>(r&&r.devices)||[]).catch(()=>[]);
}
window.listSinks=listSinks;
async function setSink(id){
  S.cfg.sink=id||"";
  try{await invoke("set_sink",{id:id||null})}catch(e){console.warn(e)}
  window.htSet&&window.htSet("sink",S.cfg.sink,{noSave:true});
  window.htSaveSettingsNow&&window.htSaveSettingsNow();
  emitSync();
}
window.setSink=setSink;
function sleepSet(mins){
  SLEEP.until=Date.now()+mins*60000;SLEEP.stopAfterTrack=false;
  clearTimeout(SLEEP.timer);
  SLEEP.timer=setTimeout(()=>{setPlaying(false);SLEEP.until=0;toast("Sleep timer: playback stopped","ok")},mins*60000);
  toast("Sleep timer: "+mins+" min","ok");
  emitSync();
}
function sleepEndOfTrack(){
  SLEEP.until=0;SLEEP.stopAfterTrack=true;clearTimeout(SLEEP.timer);
  toast("Sleep: after this track","ok");emitSync();
}
function sleepCancel(){SLEEP.until=0;SLEEP.stopAfterTrack=false;clearTimeout(SLEEP.timer);emitSync()}
window.sleepSet=sleepSet;window.sleepEndOfTrack=sleepEndOfTrack;window.sleepCancel=sleepCancel;

/* ============================================================
   BARS (owner analyses; viewers receive halftone:bars)
   ============================================================ */
const BAR_TMP=new Uint8Array(256);
function sampleBars(){
  if(!analyser)return;
  analyser.getByteFrequencyData(BAR_TMP);
  const NB=NBARS,per=Math.floor(BAR_TMP.length/NB);
  for(let i=0;i<NB;i++){
    let s=0;for(let k=0;k<per;k++)s+=BAR_TMP[i*per+k];
    const v=s/(per*255);
    EMA[i]=EMA[i]*.82+v*.18;
    S._barsRcv[i]=EMA[i];
  }
}

/* ============================================================
   PERSISTENCE (settings.js is the store; this is a mirror)
   ============================================================ */
function saveStore(){
  /* settings.js owns persistence; this mirror is a boot-time fallback.
     NEVER stringify S wholesale (DOM refs like S._img are circular). */
  try{
    const pick={
      liked:[...S.liked],
      playlists:S.playlists.map(p=>({name:p.name,paths:[...p.paths]})),
      root:S.root,i:S.i,vol:S.vol,shuffle:S.shuffle,repeat:S.repeat,
      lastTrack:S.i,sink:S.cfg.sink||""
    };
    localStorage.setItem("halftone.store",JSON.stringify({v:pick}));
  }catch(e){}
  window.htSaveSettingsNow&&window.htSaveSettingsNow();
}
window.saveStore=saveStore;
function applyPersisted(){
  /* settings.js owns persistence now; S.cfg is hydrated from the
     settings store at boot (htSettings.ready) and via setCfg. */
}
function pushEq(rebuild){
  if(IS_OWNER){if(rebuild)reconnectEq();else applyEqGains();emitSync()}
  else emitCmd({cmd:"eq",eq:{on:S.cfg.eq.on!==false,pre:S.cfg.eq.pre||0,bands:[...(S.cfg.eq.bands||[])],rebuild:!!rebuild}});
}
window.pushEq=pushEq;

/* ============================================================
   IPC: OWNER <-> VIEWER
   ============================================================ */
function curWin(){try{return T&&T.window?T.window.getCurrentWindow():null}catch(e){return null}}
window.curWin=curWin();
function wireDrag(elm,allow){
  if(!elm)return;
  elm.addEventListener("pointerdown",e=>{
    if(e.target.closest("button,input,select,a,.seek,.slider,.menu,.sheet"))return;
    if(allow&&!allow(e))return;
    if(e.button!==0)return;
    const w=window.curWin;
    if(w&&w.startDragging){try{w.startDragging()}catch(err){console.warn(err)}}
  });
}
window.wireDrag=wireDrag;
/* shared small helpers (widget page reuses these) */
window.clamp=clamp;window.esc=esc;window.fmt=fmt;window.css=css;
function emitCmd(o){
  if(!(T&&T.event&&T.event.emit))return;
  try{T.event.emit("halftone:cmd",o).catch(()=>{})}catch(_){}
}
window.emitCmd=emitCmd;
function emitSync(extra){
  if(!(T&&T.event&&T.event.emit))return;
  const p=Object.assign(syncState(),extra||{});
  try{T.event.emit("halftone:sync",p).catch(()=>{})}catch(_){}
}
window.emitSync=emitSync;
function syncState(){
  return {
    i:S.i,playing:S.playing,shuffle:S.shuffle,repeat:S.repeat,vol:S.vol,
    pos:S._pos,libN:S.lib.length,
    meta:S.meta?{...S.meta,cover:null,coverUrl:S.meta.coverUrl||null}:null,
    lyrics:{lines:S.lyrics,plain:S.lyricsPlain,status:S.lyricsStatus,synced:S.lyrics.length>0},
    pinned:S.pinned,lyricsOpen:S.lyricsOpen,
    root:S.root,liked:[...S.liked],
    playlists:S.playlists.map(p=>({name:p.name,paths:[...(p.paths||[])]})),
    sleepLeft:SLEEP.until?SLEEP.until-Date.now():0,
    sleepTrack:SLEEP.stopAfterTrack,
    sink:S.cfg.sink||"",
  };
}
window.syncState=syncState;
function emitTick(t){
  if(!(T&&T.event&&T.event.emit))return;
  try{T.event.emit("halftone:tick",{t,pos:t,playing:S.playing,dur:durSec()}).catch(()=>{})}catch(_){}
}
let lastBarsEmit=0;
function emitBars(force){
  if(!(T&&T.event&&T.event.emit))return;
  const now=performance.now();
  if(!force&&now-lastBarsEmit<1000/30)return;
  lastBarsEmit=now;
  try{T.event.emit("halftone:bars",{bars:Array.from(S._barsRcv)}).catch(()=>{})}catch(_){}
}
function durSec(){const a=document.getElementById("aud");return (a&&a.duration&&!isNaN(a.duration))?a.duration:(S.meta?S.meta.duration_s:0)}
function posSec(){
  if(IS_OWNER){const a=document.getElementById("aud");return (a&&!isNaN(a.currentTime))?a.currentTime:S._pos||0}
  return window.viewerSmoothTick?viewerSmoothTick():(S._pos||0);
}
window.posSec=posSec;window.durSec=durSec;

/* ---------- commands from the viewer (or main) ---------- */
const CMD={
  async play(o){await setPlaying(!S.playing)},
  async load(o){if(o&&o.i!=null)await loadTrack(o.i)},
  async next(){await nextTrack()},
  async prev(){posSec()>3?seekTo(0):await loadTrack(S.i-1)},
  async seek(o){seekTo(o&&o.t||0)},
  async nudge(o){seekTo(clamp(posSec()+((o&&o.d)||5),0,durSec()))},
  async vol(o){setVol(o&&o.v!=null?o.v:S.vol)},
  async shuffle(){S.shuffle=!S.shuffle;updTransport();emitSync()},
  async repeat(){cycleRepeat();updTransport()},
  async eq(o){
    if(!o||!o.eq)return;
    S.cfg.eq={on:o.eq.on!==false,pre:o.eq.pre||0,bands:[...(o.eq.bands||new Array(10).fill(0))],profile:null};
    if(IS_OWNER){if(o.eq.rebuild)reconnectEq();else applyEqGains()}
    emitSync();
  },
  async sink(o){if(IS_OWNER)await setSink(o&&o.id);else S.cfg.sink=(o&&o.id)||""},
  async sleep(o){
    if(!IS_OWNER)return;
    const m=o&&o.mode;
    if(m==="min")sleepSet((o&&o.mins)||15);
    else if(m==="track")sleepEndOfTrack();
    else sleepCancel();
  },
  async lrcfetch(o){if(IS_OWNER&&o&&o.path){try{await invoke("lrc_fetch",{path:o.path,artist:o.artist,title:o.title});await loadLyrics(S.meta,true)}catch(e){console.warn(e)}}},
  async scan(o){if(o&&o.dir)await scanLibrary(o.dir)},
  async pin(o){S.pinned=!!(o&&o.v);if(window.applyWidgetPin)applyWidgetPin(S.pinned);emitSync()},
  async hello(){
    emitSync();
    if(!IS_OWNER)return;
  },
  async opennp(o){
    if(IS_OWNER&&window.takeNP)window.takeNP(o||{});
  },
  /* widget art right-click -> "Change cover art" (task 10): the full
     import/search/reset sheet lives in the owner window, like opennp */
  async cover(o){
    if(IS_OWNER&&o&&o.path&&window.openCoverSheet){
      const t=S.lib.find(x=>x.path===o.path)||S.meta;
      if(t)window.openCoverSheet(t);
    }
  },
};
async function handleCmd(o){
  try{const fn=CMD[o&&o.cmd];if(fn)await fn(o)}catch(e){console.warn("cmd",o&&o.cmd,e)}
}
window.handleCmd=handleCmd;

/* ---------- Tauri event wiring ---------- */
function listen(name,fn){
  if(T&&T.event&&T.event.listen){try{T.event.listen(name,fn).catch(()=>{})}catch(e){console.warn(e)}}
  else document.addEventListener(name.replace("halftone:","halftone-ev-"),fn);
}
window.listen=listen;
function wireEvents(){
  listen("halftone:cmd",e=>handleCmd(e.payload));
  if(IS_OWNER){
    /* owner: viewers ask for state; owner pushes sync on changes */
    listen("halftone:hello",()=>emitSync());
    listen("halftone:lib-changed",()=>{
      if(S.cfg.watch===false)return;
      if(!S.root)return;
      scanLibrary(S.root).then(()=>render&&render());
    });
  }else{
    listen("halftone:sync",e=>applySync(e.payload));
    listen("halftone:tick",e=>{const p=e.payload||{};if(window.viewerReconcile)viewerReconcile(p.pos!=null?p.pos:p.t);updClockText()});
    listen("halftone:bars",e=>{const b=e.payload&&e.payload.bars;if(b)for(let i=0;i<NBARS;i++)S._barsRcv[i]=b[i]||0});
    listen("halftone:lib",e=>{refreshLibSnapshot()});
  }
}
window.applySync=applySync;
window.wireEvents=wireEvents;
function applySync(p){
  if(!p)return;
  const had=S.meta?S.meta.path:null;
  S.i=p.i!=null?p.i:S.i;
  S.playing=!!p.playing;
  S.shuffle=!!p.shuffle;
  S.repeat=p.repeat||"off";
  S.vol=p.vol!=null?p.vol:S.vol;
  S.root=p.root||S.root;
  S.pinned=!!p.pinned;
  S.lyricsOpen=!!p.lyricsOpen;
  S.meta=p.meta||null;
  if(p.liked)S.liked=new Set(p.liked);
  if(p.playlists)S.playlists=p.playlists.map(x=>({name:x.name,paths:new Set(x.paths)}));
  if(p.lyrics){
    S.lyrics=p.lyrics.lines||[];
    S.lyricsPlain=p.lyrics.plain||null;
    S.lyricsStatus=p.lyrics.status||"none";
    buildLyrics();
  }
  if(p.sleepLeft!=null)S._sleepLeft=p.sleepLeft;
  if(p.sleepTrack!=null)S._sleepTrack=p.sleepTrack;
  if(p.sink!=null)S.cfg.sink=p.sink;
  if(S.meta&&S.meta.path!==had)loadArtFromMeta(S.meta);
  document.dispatchEvent(new CustomEvent("halftone:track"));
  document.dispatchEvent(new CustomEvent("halftone:state"));
  document.dispatchEvent(new CustomEvent("halftone:vol"));
  if(p.pos!=null&&window.viewerReconcile)viewerReconcile(p.pos);
  updTransport&&updTransport();
}

/* ============================================================
   LIBRARY (scan / snapshot / rescan / watcher)
   ============================================================ */
function trackCoverUrl(t){return t&&t.path?invoke("cover_url",{path:t.path}).then(u=>u||""):Promise.resolve("")}
/* page render hook: main.html overrides with its router; widget = no-op */
function render(){}
window.render=render;window.trackCoverUrl=trackCoverUrl;
let scanAbort=false;
async function scanLibrary(dir){
  if(!dir){toast("Set a music folder first","warn");return null}
  S.root=dir;
  scanAbort=false;
  document.dispatchEvent(new CustomEvent("halftone:scan-start"));
  try{
    const res=await invoke("scan_library",{dir});
    if(res&&res.tracks){
      S.lib=res.tracks.map(t=>Object.assign({},t,{coverUrl:null}));
      toast(`Library: ${S.lib.length} tracks`+
        (res.skipped&&res.skipped.length?` \u00b7 ${res.skipped.length} skipped`:"")+
        (res.unsupported?` \u00b7 ${res.unsupported} unsupported`:""),"ok");
    }
    if(res&&res.skipped&&res.skipped.length)console.info("skipped:",res.skipped.slice(0,10));
    render&&render();
    emitSync();
    if(T&&T.event&&T.event.emit){try{T.event.emit("halftone:lib",{n:S.lib.length}).catch(()=>{})}catch(_){}}
    return res;
  }catch(e){
    console.warn("scan",e);
    toast("Scan failed: "+e,"error");
    return null;
  }
}
window.scanLibrary=scanLibrary;
async function refreshLibSnapshot(){
  try{
    const r=await invoke("library_snapshot");
    /* BACKEND_API.md: library_snapshot returns a PLAIN TrackMeta[]
       (older builds wrapped it as {tracks:[...]} — accept both) */
    const arr=Array.isArray(r)?r:(r&&r.tracks);
    if(Array.isArray(arr)){S.lib=arr.map(t=>Object.assign({},t,{coverUrl:null}));render&&render()}
  }catch(e){console.warn("snapshot",e)}
}
window.refreshLibSnapshot=refreshLibSnapshot;
function rescanLibrary(){if(S.root)return scanLibrary(S.root);toast("No folder set","warn")}
window.rescanLibrary=rescanLibrary;

/* ============================================================
   PLAYBACK
   ============================================================ */
async function setPlaying(v){
  S.playing=v;
  const a=document.getElementById("aud");
  if(IS_OWNER&&a){
    ensureAudio();
    if(AC&&AC.state==="suspended"){try{await AC.resume()}catch(e){}}
    if(v){try{await a.play()}catch(e){console.warn("play",e);S.playing=false}}
    else a.pause();
  }
  updTransport&&updTransport();
  emitSync();
  document.dispatchEvent(new CustomEvent("halftone:state"));
}
window.setPlaying=setPlaying;
function setVol(v){
  S.vol=clamp(v,0,1);
  const a=document.getElementById("aud");
  if(a)a.volume=S.vol;
  document.dispatchEvent(new CustomEvent("halftone:vol"));
  emitSync();
}
window.setVol=setVol;
function nudgeVol(d){setVol(S.vol+d)}
window.nudgeVol=nudgeVol;
function cycleRepeat(){S.repeat=S.repeat==="off"?"all":S.repeat==="all"?"one":"off"}
window.cycleRepeat=cycleRepeat;
function updTransport(){}
window.updTransport=updTransport;
function updClockText(){}

/* ---------- queue (real play-queue support) ---------- */
function findIdxByPath(p){return S.lib.findIndex(t=>t.path===p)}
function queueAdd(paths,mode){
  for(const p of paths){
    const t=S.lib[findIdxByPath(p)];
    if(!t)continue;
    if(mode==="next")S.queue.unshift({path:t.path});
    else S.queue.push({path:t.path});
  }
  emitSync();
  document.dispatchEvent(new CustomEvent("halftone:queue"));
}
window.queueAdd=queueAdd;
function queueRemove(i){S.queue.splice(i,1);emitSync();document.dispatchEvent(new CustomEvent("halftone:queue"))}
window.queueRemove=queueRemove;
function queueClear(){S.queue=[];S.queueHistory=[];emitSync();document.dispatchEvent(new CustomEvent("halftone:queue"))}
window.queueClear=queueClear;
function nextIndex(){
  if(S.queue.length){
    const q0=S.queue.shift();
    const idx=findIdxByPath(q0.path);
    if(idx>=0)return idx;
    return nextIndex();
  }
  if(S.repeat==="one")return S.i;
  if(S.shuffle){
    if(S.lib.length<=1)return S.i;
    let n;do{n=Math.floor(Math.random()*S.lib.length)}while(n===S.i);
    return n;
  }
  if(S.i+1<S.lib.length)return S.i+1;
  return S.repeat==="all"?0:S.i;   /* stop at end unless repeat-all */
}
window.nextIndex=nextIndex;
function prevIndex(){return S.i-1>=0?S.i-1:(S.repeat==="all"?S.lib.length-1:S.i)}

/* ============================================================
   TRACK LOADING (owner only)
   ============================================================ */
let loadSeq=0;
async function loadTrack(i,autoplay=true){
  if(!S.lib.length)return;
  i=clamp(i,0,S.lib.length-1);
  const seq=++loadSeq;
  S.i=i;
  const t=S.lib[i];
  if(!t)return;
  if(autoplay||S.playing){S.playing=true}
  /* 1. open_track: full meta + cover + embedded lyrics (owner only) */
  let full=null;
  try{full=await invoke("open_track",{path:t.path})}catch(e){console.warn("open_track",e)}
  if(seq!==loadSeq)return;   /* track changed mid-flight */
  S.meta=full||Object.assign({},t);
  if(S.meta&&!S.meta.coverUrl)S.meta.coverUrl=null;
  S._pos=0;S.lidx=-1;
  /* 2. playback URL + audio element */
  if(IS_OWNER){
    const a=document.getElementById("aud");
    try{
      const url=await invoke("media_url",{path:t.path});
      if(seq!==loadSeq)return;
      if(url&&a){a.src=url;S.playing?a.play().catch(()=>{}):0}
    }catch(e){console.warn("media_url",e)}
  }
  /* 3. cover art (lazy URL, never base64 over IPC) */
  loadArtFromMeta(S.meta);
  /* 3b. no embedded art? auto-fetch (task 10) */
  coverAutoMaybe(S.meta);
  /* 4. lyrics: auto-fetch (T6) */
  loadLyrics(S.meta,false);
  /* 5. broadcast */
  document.dispatchEvent(new CustomEvent("halftone:track"));
  emitSync();
}
window.loadTrack=loadTrack;
async function loadArtFromMeta(m){
  if(!m)return;
  const a=document.getElementById("aud");
  if(m&&m.path){
    try{
      const u=await invoke("cover_url",{path:m.path});
      if(!u)return;
      S._artUrl=u;
      const im=new Image();
      im.crossOrigin="anonymous";
      im.onload=()=>{
        if(S.meta&&S.meta.path===m.path){
          S._img=im;
          document.dispatchEvent(new CustomEvent("halftone:art"));
          if(ACC.mode==="album"){
            const acc=extractAccent(im);
            if(acc)setAccentRGB(clampAccent(acc).rgb);
          }
        }
      };
      im.src=u;
    }catch(e){console.warn("cover_url",e)}
  }
}
window.loadArtFromMeta=loadArtFromMeta;

/* ============================================================
   COVER ART (task 10)
   - auto: tracks without art get cover_auto() (throttled queue,
     max 2 in flight; allowNet from the privacy setting)
   - manual: "Change cover art" context menu -> import / search /
     reset (main.html wires the menu; shared helpers live here)
   ============================================================ */
const COVER_AUTO_MAX=2;
const coverAutoQ=[];          /* paths pending cover_auto */
let coverAutoInFlight=0;
const coverAutoTried=new Set();   /* album keys already attempted this run */
const albumKeyOf=t=>((t&&t.album)||"").toLowerCase()+"|"+((t&&t.artist)||"");

function coverAutoPump(){
  while(coverAutoInFlight<COVER_AUTO_MAX&&coverAutoQ.length){
    const t=coverAutoQ.shift();
    coverAutoInFlight++;
    const allowNet=window.htSettings?window.htSettings.all().coverAutoNet!==false:true;
    invoke("cover_auto",{path:t.path,artist:t.artist||"",album:t.album||"",allowNet})
      .then(r=>{
        if(r&&r.url){
          /* album art changed: refresh every view (cover_url / this url
             carries the ?v= cache buster per BACKEND_API.md) */
          S.lib.forEach(x=>{if(albumKeyOf(x)===albumKeyOf(t))x.coverUrl=r.url});
          if(S.meta&&albumKeyOf(S.meta)===albumKeyOf(t)){S.meta.coverUrl=r.url;loadArtFromMeta(S.meta)}
          document.dispatchEvent(new CustomEvent("halftone:lib"));
        }
      })
      .catch(()=>{})
      .finally(()=>{
        coverAutoInFlight--;
        coverAutoPump();
      });
  }
}
/* Enqueue auto-fetch for tracks shown/played without art (dedup per album) */
function coverAutoMaybe(t){
  if(!t||t.has_cover||coverAutoTried.has(albumKeyOf(t)))return;
  coverAutoTried.add(albumKeyOf(t));
  coverAutoQ.push(t);
  coverAutoPump();
}
window.coverAutoMaybe=coverAutoMaybe;

/* "Change cover art" action set (shared by context menu + row sheet) */
async function coverImportFor(t){
  try{
    const u=await invoke("cover_import",{path:t.path});   /* null = cancelled */
    if(u!==null&&u!==undefined)await coverRefreshTrack(t);
    return u;
  }catch(e){toast("Import failed: "+e,"error");return undefined}
}
async function coverApplyFor(t,url){
  try{
    const u=await invoke("cover_apply_url",{path:t.path,url:url||""});
    await coverRefreshTrack(t);
    return u;
  }catch(e){toast("Apply failed: "+e,"error");return undefined}
}
async function coverResetFor(t){
  try{
    await invoke("cover_reset",{path:t.path});
    await coverRefreshTrack(t);
  }catch(e){toast("Reset failed: "+e,"error")}
}
/* after any override change: re-pull cover_url (new ?v=) + refresh views */
async function coverRefreshTrack(t){
  const u=await invoke("cover_url",{path:t.path}).catch(()=>null);
  S.lib.forEach(x=>{if(albumKeyOf(x)===albumKeyOf(t))x.coverUrl=u||null});
  if(S.meta&&albumKeyOf(S.meta)===albumKeyOf(t)){S.meta.coverUrl=u||null;S._artUrl=u||null;loadArtFromMeta(S.meta)}
  coverAutoTried.delete(albumKeyOf(t));   /* reset allows auto again later */
  document.dispatchEvent(new CustomEvent("halftone:lib"));
  render&&render();
}
window.coverImportFor=coverImportFor;
window.coverApplyFor=coverApplyFor;
window.coverResetFor=coverResetFor;
window.coverInfoFor=t=>invoke("cover_info",{path:t.path}).catch(()=>({source:"none",url:null}));

/* ============================================================
   LYRICS (T6: auto-fetch, sources, status, no auto-browser)
   ============================================================ */
let lyricsSeq=0;
async function loadLyrics(m,force){
  if(!m)return;
  const seq=++lyricsSeq;
  S.lyricsStatus="searching";
  document.dispatchEvent(new CustomEvent("halftone:lyrics-status"));
  let res=null;
  try{
    res=await invoke("lyrics_get",{path:m.path,artist:m.artist||"",title:m.title||"",
      album:m.album||"",duration:Math.round(m.duration_s||0),
      allowNet:S.cfg.lyricsAuto!==false,force:!!force});
  }catch(e){console.warn("lyrics_get",e)}
  if(seq!==lyricsSeq)return;   /* stale: track changed meanwhile */
  if(res&&(res.lines&&res.lines.length||res.plain)){
    S.lyrics=res.lines||[];
    S.lyricsPlain=res.plain||null;
    S.lyricsStatus=res.source||"cache";
    S._lyricsSynced=!!res.synced;
  }else{
    S.lyrics=[];
    S.lyricsPlain=null;
    S.lyricsStatus=res&&res.source==="none"?"notfound":"error";
    S._lyricsSynced=false;
  }
  buildLyrics();
  document.dispatchEvent(new CustomEvent("halftone:lyrics"));
  emitSync();
}
window.loadLyrics=loadLyrics;
function retryLyrics(){
  const m=S.meta;
  if(m)loadLyrics(m,true);
}
window.retryLyrics=retryLyrics;
function openLrcSearch(){
  const t=S.meta||{};
  openExternal("https://lrclib.net/search?q="+encodeURIComponent((t.artist||"")+" "+(t.title||"")));
}
window.openLrcSearch=openLrcSearch;

/* ---------- lyric line DOM builders (both surfaces) ---------- */
function buildLyrics(){
  const wrap=document.getElementById("lyrWrap")||document.getElementById("lyr-wrap");
  if(!wrap)return;
  wrap.innerHTML="";
  S.lidx=-1;
  const view=wrap.closest(".lyr-view");
  if(S.lyrics.length){
    const host=wrap.closest(".lyrics")||wrap.closest(".widget-lyrics");
    if(host)host.classList.remove("plain");
    S.lyrics.forEach(l=>{
      const d=document.createElement("div");
      d.className="lyric-line";
      d.dataset.t=+l.t||0;   /* backend LyricLine.t is SECONDS (lib.rs parse_lrc) */
      d.textContent=l.text||"";
      /* lyricFollow lights a line when pos-offset >= t, so seek to t+offset */
      d.onclick=()=>{
        const t=Math.max(0,(+l.t||0)+(S.cfg.lyricsOffset||0)/1000);
        if(IS_OWNER)seekTo(t);                       /* owner: applies to <audio> */
        else{S._pos=t;emitCmd({cmd:"seek",t})}       /* viewer: owner applies it (a10b2ae path) */
        lyrResume();
      };
      wrap.appendChild(d);
    });
  }else if(S.lyricsPlain){
    const host=wrap.closest(".lyrics")||wrap.closest(".widget-lyrics");
    if(host)host.classList.add("plain");
    S.lyricsPlain.split(/\r?\n/).forEach(txt=>{
      const d=document.createElement("div");
      d.className="lyric-line";
      d.textContent=txt;
      wrap.appendChild(d);
    });
  }
  /* fresh lyrics -> auto-follow resumes from a clean slate */
  if(view)view.classList.remove("manual");
  if(wrap._lrcPill){wrap._lrcPill.remove();wrap._lrcPill=null}
}
window.buildLyrics=buildLyrics;

/* ---------- manual scroll vs auto-follow ----------
   Wheel / touchpad / touch / drag-scrollbar over the lyrics pauses
   auto-follow (`S._lyrManual`); it resumes 3s after the last manual
   scroll, when a line is clicked (lyrResume), or via the "LIVE" pill. */
function lyrPause(){
  if(!S._lyrManual){
    S._lyrManual=true;
    const wrap=document.getElementById("lyrWrap")||document.getElementById("lyr-wrap");
    const view=wrap&&wrap.closest(".lyr-view");
    if(view)view.classList.add("manual");
    lyrPill();
  }
  clearTimeout(S._lyrManualT);
  S._lyrManualT=setTimeout(lyrResume,3000);
}
function lyrResume(){
  clearTimeout(S._lyrManualT);S._lyrManualT=0;
  if(!S._lyrManual)return;
  S._lyrManual=false;
  const wrap=document.getElementById("lyrWrap")||document.getElementById("lyr-wrap");
  if(!wrap){
    const view=document.getElementById("npLyrView")||document.getElementById("wLyrView");
    if(view){view.classList.remove("manual")}
    return;
  }
  const view=wrap.closest(".lyr-view");
  if(view)view.classList.remove("manual");
  if(wrap._lrcPill){wrap._lrcPill.remove();wrap._lrcPill=null}
  S.lidx=-1;   /* re-center on the current line right away */
  lyricFollow();
}
window.lyrPause=lyrPause;window.lyrResume=lyrResume;window.lyrPill=lyrPill;
/* "● LIVE" pill: click = jump back to the playing line now.
   Lives in the lyrics HOST (not the scrolling view) so it is pinned to
   the visible bottom-right corner no matter the scroll position. */
function lyrPill(){
  const wrap=document.getElementById("lyrWrap")||document.getElementById("lyr-wrap");
  if(!wrap)return;
  const host=wrap.closest(".lyrics")||wrap.closest(".lyr-view");
  if(!host)return;
  if(!wrap._lrcPill){
    const p=document.createElement("button");
    p.className="lyr-live";
    p.type="button";
    p.textContent="\u25cf LIVE";
    p.title="Back to the playing line";
    p.onclick=e=>{e.stopPropagation();lyrResume()};
    wrap._lrcPill=p;
    host.appendChild(p);
  }
  wrap._lrcPill.classList.add("show");
}
/* free scrolling: wheel over lyrics never seeks (only .seek uses the
   wheel), and any manual scroll pauses auto-follow */
function wireLyrScroll(doc){
  doc=doc||document;
  const bind=el=>{
    if(!el)return;
    el.addEventListener("wheel",e=>{if(!e.ctrlKey&&!e.shiftKey)lyrPause()},{passive:true});
    el.addEventListener("touchmove",lyrPause,{passive:true});
    el.addEventListener("pointerdown",e=>{
      /* drag on the scrollbar (~15px gutter) = manual scroll */
      if(e.target.closest(".lyric-line"))return;
      const r=el.getBoundingClientRect();
      if(e.clientX>=r.right-16)lyrPause();
    });
  };
  bind(doc.getElementById("npLyrView"));
  bind(doc.getElementById("wLyrView"));
}
window.wireLyrScroll=wireLyrScroll;

function lyricFollow(){
  const wrap=document.getElementById("lyrWrap")||document.getElementById("lyr-wrap");
  if(!wrap||!S.lyrics.length)return;
  const view=wrap.closest(".lyr-view");
  if(!view)return;
  const lines=[...wrap.children];
  if(!lines.length)return;
  const t=posSec()-((S.cfg.lyricsOffset||0)/1000);
  let idx=-1;
  for(let k=0;k<lines.length;k++){if(t>=+lines[k].dataset.t)idx=k}
  if(idx!==S.lidx){
    S.lidx=idx;
    lines.forEach((l,k)=>{l.classList.toggle("active",k===idx);l.classList.toggle("past",idx>=0&&k<idx);l.classList.toggle("future",idx>=0&&k>idx)});
    if(idx>=0&&!S._lyrManual){
      const ln=lines[idx];
      const top=ln.offsetTop-view.clientHeight/2+ln.offsetHeight/2;
      view.scrollTo({top:Math.max(0,top),behavior:"smooth"});
    }
    /* LIVE pill stays visible for the whole manual pause (resume =
       pill click, a line click, a seek, or the 3s scroll timer) */
    if(S._lyrManual){
      const p=wrap._lrcPill;
      if(p)p.classList.add("show");
    }
  }
}
window.lyricFollow=lyricFollow;

/* ---------- seek helper ---------- */
function seekTo(t){
  const a=document.getElementById("aud");
  if(IS_OWNER&&a&&a.duration){a.currentTime=clamp(t,0,a.duration);S._pos=a.currentTime}
  else S._pos=t;
  document.dispatchEvent(new CustomEvent("halftone:seeked"));
  emitSync();
}
window.seekTo=seekTo;

/* ============================================================
   SEEKBAR WIRING (per-element; canvas + gel both supported)
   ============================================================ */
function wireSeek(seekEl){
  if(!seekEl)return;
  const apply=e=>{
    const r=seekEl.getBoundingClientRect();   /* transform-aware */
    return clamp((e.clientX-r.left)/Math.max(1,r.width),0,1);
  };
  seekEl.addEventListener("pointerdown",e=>{
    try{seekEl.setPointerCapture(e.pointerId)}catch(_){}
    S.drag={p:apply(e),el:seekEl,t:apply(e)*durSec()};
    seekEl.classList.add("dragging");
  });
  seekEl.addEventListener("pointermove",e=>{if(S.drag&&S.drag.el===seekEl){S.drag.p=apply(e);S.drag.t=S.drag.p*durSec()}});
  const end=()=>{
    if(!S.drag||S.drag.el!==seekEl)return;
    seekTo(S.drag.p*durSec());
    S.drag=null;S.lidx=-1;
    seekEl.classList.remove("dragging");
  };
  seekEl.addEventListener("pointerup",end);
  seekEl.addEventListener("pointercancel",end);
  seekEl.addEventListener("wheel",e=>{
    /* only the seekbar itself seeks on wheel — events bubbling from
       children/grandchildren (e.g. tooltips over the lyrics pane) must
       scroll normally instead of yanking playback (owner bug 2) */
    if(!(e.target===seekEl||seekEl.contains(e.target)))return;
    e.preventDefault();
    seekTo(posSec()+(e.deltaY<0?5:-5));
  },{passive:false});
}
window.wireSeek=wireSeek;

/* ============================================================
   GEL FILL SYNC (DOM variant: --val + knob, per contract)
   ============================================================ */
function paintGel(root,p){
  if(!root)return;
  root.style.setProperty("--val",(p*100).toFixed(2)+"%");
  /* liquid-glass spectrum: NBARS micro bars inside the glass tube, full
     width — accent-lit left of the playhead, glassy dim right of it.
     Renders in digital (gel themes); LED themes never see it (.seek-led
     covers the tube and the gel layer is display:none). */
  const cv=root.querySelector(".gel-bars");
  if(cv&&getComputedStyle(cv).display!=="none"){
    const track=cv.parentElement;
    const W=track?track.clientWidth:0,H=track?track.clientHeight:0;
    if(W&&H){
      const dpr=Math.min(2,devicePixelRatio||1);
      if(cv.width!==Math.round(W*dpr)||cv.height!==Math.round(H*dpr)){cv.width=Math.round(W*dpr);cv.height=Math.round(H*dpr)}
      const g=cv.getContext("2d");
      if(g){
        g.setTransform(dpr,0,0,dpr,0,0);
        g.clearRect(0,0,W,H);
        const spec=S._barsRcv||new Float32Array(NBARS);
        const [ar,ag,ab]=ACC.cur;
        const gap=2,bw=Math.max(2,(W-gap*(NBARS-1))/NBARS);
        const bh=Math.max(3,Math.round(H*.42));
        for(let i=0;i<NBARS;i++){
          const played=(i+.5)/NBARS<p;
          const v=spec[i]||0;
          const h=played?Math.max(bh*(.55+.45*v),3):Math.max(bh*.22*v,2);
          g.fillStyle=played?`rgba(${ar},${ag},${ab},.95)`:`rgba(${ar},${ag},${ab},.20)`;
          g.fillRect(i*(bw+gap),(H-h)/2,bw,h);
        }
      }
    }
  }
}
function paintSeek(){
  const p=durSec()?posSec()/durSec():0;
  document.querySelectorAll(".seek").forEach(sk=>{
    const cv=sk.querySelector(".seek-led");
    if(cv&&getComputedStyle(cv).display!=="none"){
      /* LED dot-matrix spectrum strip (v0.1.2 signature) */
      drawSeekLedShared(cv,p);
    }
    const gel=sk.querySelector(".seek-gel");
    if(gel&&getComputedStyle(gel).display!=="none")paintGel(sk,p);
  });
}

/* ============================================================
   VOLUME CONTROL (LED ladder + gel slider, per contract)
   ============================================================ */
function buildVol(box){
  if(!box)return;
  box.innerHTML="";
  if(themeVolStyle==="gel"){
    const wrap=document.createElement("div");wrap.className="vol-gel";
    wrap.innerHTML='<div class="track"><div class="fill"></div><div class="knob"></div></div>';
    box.appendChild(wrap);
    const set=e=>{const r=wrap.getBoundingClientRect();setVol((e.clientX-r.left)/Math.max(1,r.width))};
    let down=false;
    wrap.addEventListener("pointerdown",e=>{down=true;try{wrap.setPointerCapture(e.pointerId)}catch(_){};set(e)});
    wrap.addEventListener("pointermove",e=>down&&set(e));
    wrap.addEventListener("pointerup",()=>down=false);
    wrap.addEventListener("pointercancel",()=>down=false);
  }else{
    const wrap=document.createElement("canvas");wrap.className="vol-leds";wrap.width=64;wrap.height=14;
    box.appendChild(wrap);
    const paint=()=>{
      fitCanvas(wrap);
      const g=wrap.getContext("2d");if(!g)return;
      const W=wrap.width,H=wrap.height;
      g.clearRect(0,0,W,H);
      const n=Math.max(4,Math.floor(W/9));
      const segW=Math.max(3,Math.floor(W/n)-3);
      for(let i=0;i<n;i++){
        const on=S.vol>=(i+1)/n-.01;
        g.fillStyle=on?css(ACC.cur,.95):css(themeCanvas.off,.9);
        g.fillRect(i*(segW+3),1,segW,H-2);
      }
    };
    paint();
    box._paintLeds=paint;
    const set=e=>{const r=wrap.getBoundingClientRect();setVol((e.clientX-r.left)/Math.max(1,r.width))};
    let down=false;
    wrap.addEventListener("pointerdown",e=>{down=true;try{wrap.setPointerCapture(e.pointerId)}catch(_){};set(e)});
    wrap.addEventListener("pointermove",e=>{if(down){set(e);paint()}});
    wrap.addEventListener("pointerup",()=>down=false);
    wrap.addEventListener("pointercancel",()=>down=false);
  }
}
window.buildVol=buildVol;

/* ============================================================
   RENDER LOOP (one per window; skips hidden work)
   ============================================================ */
let rafPending=false,artDirty=true,themeDirty=true;
function requestRedraw(why){artDirty=true}
window.requestRedraw=requestRedraw;
let lastTickEmit=0;
function loop(now){
  rafPending=false;
  const hidden=document.hidden;
  tickAccent(now);
  if(!hidden){
    if(IS_OWNER)sampleBars();
    /* paint any visible canvas meters */
    paintSeek();
    document.querySelectorAll(".vol-leds").forEach(c=>{if(c.parentElement&&c.parentElement._paintLeds)c.parentElement._paintLeds()});
    /* page-level paint hook (widget page paints its ids through this) */
    if(window.htPagePaint)window.htPagePaint(now);
    /* ambient (main np view only) */
    const amb=document.querySelector(".fx-ambient");
    if(amb&&amb.offsetParent!==null){
      const mode=S.cfg.ambient==="theme"?themeAmbientDefault:(S.cfg.ambient||themeAmbientDefault);
      if(mode==="dither")drawAmbient(amb,now/1000);
      else if(mode==="halo")drawHalo(amb,now/1000);
      else if(mode==="aurora"){/* pure CSS (theme-owned) */}
      else g_clear(amb);
    }
    /* dither repaint when accent tweens */
    if(artDirty||ACC.anim){
      /* each canvas repaints ITS OWN image (cv._img) — pushing the
         now-playing S._img into every dither canvas made album tiles show
         the playing track's cover after any theme/accent redraw (the
         "albums view shows the playing song's cover" bug). np/widget pass
         the playing image explicitly in their own painters (correct). */
      document.querySelectorAll("canvas.art-dither").forEach(cv=>{if(cv._img)drawDither(cv,cv._img)});
      artDirty=false;
    }
  }
  lyricFollow();
  /* owner broadcast: merged time+bars tick @30Hz (T7) */
  if(IS_OWNER&&!hidden){
    const t=now-lastTickEmit;
    if(t>=1000/30){
      lastTickEmit=now;
      emitTick(posSec());
      emitBars();
    }
  }
  if(!rafPending){rafPending=true;requestAnimationFrame(loop)}
}
function g_clear(cv){const g=cv.getContext("2d");if(g)g.clearRect(0,0,cv.width,cv.height)}
window.requestAnimationFrame(loop);

/* ============================================================
   CONTEXT MENU (DOM fallback; widget may prefer native)
   ============================================================ */
function openCtx(x,y,items){
  closeCtx();
  const m=document.createElement("div");
  m.className="menu ctxroot open";
  const build=(list,host)=>{
    let i=0;
    for(const it of list){
      if(it.sep){const s=document.createElement("div");s.className="menu-sep";host.appendChild(s);continue}
      const w=document.createElement("div");
      try{
        if(it.children){
          w.className="ctxwrap";
          const b=document.createElement("button");b.className="menu-item";
          b.innerHTML=`<span class="ctxdot"></span>${esc(it.label)}<span class="ctxarrow">\u25B8</span>`;
          w.appendChild(b);
          const sub=document.createElement("div");sub.className="menu sub open";
          build(it.children,sub);
          w.appendChild(sub);
        }else{
          const b=document.createElement("button");b.className="menu-item"+(it.checked?" on":"");
          b.innerHTML=`<span class="ctxdot"></span>${esc(it.label)}`;
          b.onclick=()=>{closeCtx();it.onClick&&it.onClick()};
          w.appendChild(b);
        }
        host.appendChild(w);
      }catch(err){
        /* a broken item must not take down the whole menu in the real app */
        console.error("openCtx: item "+i+" failed to build",err);
        if(!w.firstChild){w.className="menu-item";w.style.opacity=".45";w.textContent="ITEM UNAVAILABLE"}
        host.appendChild(w);
      }
      i++;
    }
  };
  build(items,m);
  document.body.appendChild(m);
  const r=m.getBoundingClientRect();
  /* widget windows are short: a tall menu can exceed innerHeight — clamp to
     the biggest safe slot and let the menu itself scroll (ctx-scroll) */
  m.style.left=clamp(x,4,Math.max(4,innerWidth-r.width-4))+"px";
  m.style.top=clamp(y,4,Math.max(4,innerHeight-r.height-4))+"px";
  if(r.height>innerHeight-8)m.classList.add("ctx-scroll");
  /* submenus open to the right; flip to the left side when they would
     cross the window edge (frameless windows clip, no OS menu manager) */
  m.querySelectorAll(".menu.sub").forEach(s=>{
    const sr=s.getBoundingClientRect();
    if(sr.width&&sr.right>innerWidth-4)s.classList.add("sub-flip");
  });
  /* outside-close. WebView2 delivers a real right click as
     pointerdown -> mousedown -> contextmenu, so a plain pointerdown
     listener fired the instant our own right click landed; ignore every
     right-button event and also watch mousedown/click/auxclick/touchstart
     (focus/pointer-capture quirks can swallow pointerdown in WebView2). */
  const close=e=>{if(e.button===2)return;if(!m.contains(e.target))closeCtx()};
  const onKey=e=>{
    if(e.key==="Escape"){closeCtx()}
    else if(e.key==="ContextMenu"||(e.key==="F10"&&e.shiftKey)){
      /* Windows convention: ContextMenu/Shift+F10 opens for the focused
         element — re-dispatch a synthetic contextmenu at its centre so the
         window's own menu builders run unchanged */
      e.preventDefault();closeCtx();
      const t=document.activeElement;
      if(t&&t.dispatchEvent){
        const r2=t.getBoundingClientRect?t.getBoundingClientRect():null;
        const cx=r2&&r2.width?r2.left+r2.width/2:innerWidth/2;
        const cy=r2&&r2.height?r2.top+Math.min(r2.height/2,150):innerHeight/2;
        t.dispatchEvent(new MouseEvent("contextmenu",{bubbles:true,cancelable:true,clientX:cx,clientY:cy}));
      }
    }
    else if(e.key==="ArrowRight"){
      const t=document.activeElement;
      if(t&&t.classList&&t.classList.contains("menu-item")&&t.parentElement.classList.contains("ctxwrap")){
        const f=t.parentElement.querySelector(".menu.sub .menu-item");
        if(f){e.preventDefault();f.focus()}
      }
    }
  };
  const onBlur=()=>closeCtx();
  ctxCleanup=()=>{
    for(const ev of ["pointerdown","mousedown","click","auxclick","touchstart"])
      document.removeEventListener(ev,close,true);
    document.removeEventListener("keydown",onKey,true);
    window.removeEventListener("blur",onBlur);
    ctxCleanup=null;
  };
  for(const ev of ["pointerdown","mousedown","click","auxclick","touchstart"])
    document.addEventListener(ev,close,true);
  document.addEventListener("keydown",onKey,true);
  window.addEventListener("blur",onBlur);
  const first=m.querySelector(".menu-item");
  if(first)try{first.focus({preventScroll:true})}catch(_){first.focus()}
  return m;
}
let ctxCleanup=null;
function closeCtx(){
  document.querySelectorAll(".menu.ctxroot").forEach(m=>m.remove());
  if(ctxCleanup){try{ctxCleanup()}catch(_){};ctxCleanup=null}
}
window.openCtx=openCtx;window.closeCtx=closeCtx;
function accentMenuItems(){
  return [["album","FROM ALBUM ART"],["mint","MINT"],["sky","SKY"],["violet","VIOLET"],["rose","ROSE"],["amber","AMBER"],["red","RED"],["custom","CUSTOM"]].map(([v,l])=>({
    label:l,checked:ACC.mode===v,onClick:()=>setAccentMode(v)
  }));
}
window.accentMenuItems=accentMenuItems;

/* ============================================================
   DRAG-AND-DROP FOLDER SCAN
   ============================================================ */
function wireDropZone(target){
  if(!target)return;
  target.addEventListener("dragover",e=>{e.preventDefault();e.dataTransfer.dropEffect="copy"});
  target.addEventListener("drop",async e=>{
    e.preventDefault();
    const files=[...(e.dataTransfer.files||[])];
    if(!files.length)return;
    const path=files[0].path||files[0].name;
    if(!path)return;
    const dir=path.replace(/[\\/][^\\/]+$/,"");
    await scanLibrary(dir);
    render&&render();
  });
}
window.wireDropZone=wireDropZone;

/* ============================================================
   BOOT SEQUENCE
   ============================================================ */
(async function boot(){
  wireEvents();
  /* 1. settings load (shared JSON via backend, debounced saves) */
  if(window.__htSettingsReady){await window.__htSettingsReady}
  else if(window.htSettings){await window.htSettings.ready()}
  if(window.htSettings){
    const v=window.htSettings.all()||{};
    /* mirror into S.cfg for engine code */
    for(const [k,val] of Object.entries(v))S.cfg[k]=val;
  }
  recalibrateBgLum();
  updateSwitchStyles();
  /* 2. library snapshot (never a full broadcast at boot — T7) */
  await refreshLibSnapshot();
  /* 3. owner: restore last track, resume if set */
  if(IS_OWNER&&S.lib.length){
    const last=S.cfg.lastTrack||0;
    if(S.cfg.resume===true&&S.cfg.lastTrack!=null)await loadTrack(clamp(last,0,S.lib.length-1),false);
    else await loadTrack(clamp(last,0,S.lib.length-1),false);
  }
  /* 4. viewer: announce; owner answers with authoritative sync */
  if(!IS_OWNER&&T&&T.event&&T.event.emit){
    try{T.event.emit("halftone:hello",{}).catch(()=>{})}catch(_){}
  }
  document.dispatchEvent(new CustomEvent("halftone:booted"));
})();
window.__booted=true;
/* QA hook: report the last boot error (set by the onerror reporter in
   main.html/index.html) to the console so headless runs surface it */
setTimeout(()=>{
  if(window.__HT_BOOT_ERR){
    console.error("HT-BOOT-ERR["+(window.IS_VIEWER?"widget":"main")+"]",
      __HT_BOOT_ERR.msg,"line",__HT_BOOT_ERR.line,"\n"+(__HT_BOOT_ERR.stack||""));
  }
},1200);
