/* ============================================================
   HALFTONE widget page script — pure view + controller.
   common.js has already provided: invoke/listen/emitCmd/clamp/
   esc/fmt/toast/curWin/wireDrag/S/NBARS/themeCanvas sync/
   applySync (state mirror)/buildLyrics/lyricFollow/drawDither/
   fitCanvas/IS_OWNER=false. This file adds the widget's OWN
   DOM ids, stage scaling (T4) and control wiring.

   SCALING (T4): the .widget-stage is designed at a base logical
   size per preset and scaled with transform:scale(s). Its logical
   size is innerWidth/s x innerHeight/s so it FILLS the window —
   no letterboxing, no body zoom, no enforceBounds, no UIZ division.
   Pointer math uses getBoundingClientRect (transform-aware) or
   the un-scaled .w-overlay layer for menus.
   ============================================================ */
const W={s:1,preset:"card",scaleMode:"fit",scalePct:100};
const WID={};
["stage","wCover","wDither","wTitle","wArtist","wTech","wTime","wGear","wPin","wMain","wClose",
 "wSeek","wVol","wPrev","wPlay","wNext","wIcoPlay","wIcoPause","wShuf","wRep",
 "wLyrHost","wLyrView","wLyrStatus","wSheet","wSheetTitle","wSheetBody","wArt"].forEach(k=>WID[k]=document.getElementById(k));

/* QA probes need these on the window (page scripts use closure scope) */
window.W=W;window.WID=WID;
/* ============================================================
   T4: stage scaling
   ============================================================ */
/* base logical sizes per preset — the design height for "strip" here
   was 120, but the actual CSS (index.html wp-strip rule) is authored
   for 132 (--w-base-h:132). The mismatch made the auto "fit" zoom
   (s = min(w/baseW, h/baseH)) pick a slightly too-large scale for strip
   windows sized to their natural content height, nudging rows past
   their own padding and compounding the glow-clipping symptom (owner
   bug: widget "scaling weird"). Keep this in sync with the CSS. */
const BASE={card:[380,260],strip:[380,132],square:[300,400],lyrics:[380,300]};
function baseSize(){return BASE[W.preset]||BASE.card}
function applyStage(){
  const [bw,bh]=baseSize();
  let s;
  if(W.scaleMode==="fixed")s=clamp((+W.scalePct||100)/100,0.7,2);
  else{
    s=Math.min(innerWidth/bw,innerHeight/bh);
    s=clamp(s,0.5,3);
  }
  W.s=s;
  /* stage logical size = window/s by construction below, so the stage
     ALWAYS fills the window (no letterbox, even in fixed mode). Fixed
     % therefore zooms the fixed-px chrome; fluid lyric type (cqh in
     index.html) stays proportionate to the window in both modes. */
  const lw=innerWidth/s,lh=innerHeight/s;
  const st=WID.stage;
  st.style.setProperty("--s",s.toFixed(4));
  st.style.transform="scale("+s.toFixed(4)+")";
  st.style.width=lw+"px";
  st.style.height=lh+"px";
  document.documentElement.style.setProperty("--s",s.toFixed(4));
}
function scheduleStage(){requestAnimationFrame(applyStage)}
addEventListener("resize",scheduleStage);
window.applyWidgetScale=applyStage;
setTimeout(applyStage,0);
/* settle re-runs: headless/iframes resize after load; ResizeObserver on
   body catches window-driven size changes without fighting the user */
setTimeout(applyStage,300);
setTimeout(applyStage,900);
if(window.ResizeObserver){
  try{new ResizeObserver(scheduleStage).observe(document.body)}catch(_){}
}

/* ---------- viewer-side theme token mirror ---------- */
document.addEventListener("halftone:theme",()=>{
  const t=window.Theme||{};
  if(t.canvas)themeCanvas={bg:[...t.canvas.bg],off:[...t.canvas.off],ink:[...t.canvas.ink]};
  themeSeekStyle=t.seekStyle||"led";
  themeVolStyle=t.volStyle||"leds";
  themeArtDefault=t.artDefault||"dither";
  updateSwitchAttrs();
  buildVolDom();
  requestRedraw();
});

/* ---------- art ---------- */
async function loadArt(){
  if(!S.meta||!S.meta.path)return;
  try{
    const u=await invoke("cover_url",{path:S.meta.path});
    if(!u)return;
    S._artUrl=u;
    const im=new Image();im.crossOrigin="anonymous";
    im.onload=()=>{S._img=im;requestRedraw();paintArt()};
    im.src=u;
  }catch(e){console.warn("cover_url",e)}
}
window.loadArtFromMeta=loadArt;
function paintArt(){
  const mode=(window.getCfg&&window.getCfg("art"))||"theme";
  const eff=mode==="theme"?themeArtDefault:mode;
  const showReal=eff==="real"&&S._artUrl;
  WID.wCover.style.display=showReal?"":"none";
  WID.wDither.style.display=showReal?"none":"";
  if(showReal){if(WID.wCover.src!==S._artUrl)WID.wCover.src=S._artUrl}
  else if(S._img)drawDither(WID.wDither,S._img);
  const art=WID.wArt;
  art.classList.remove("art-swap");void art.offsetWidth;art.classList.add("art-swap");
}

/* ---------- info + state painting ---------- */
function paintInfo(){
  const m=S.meta;
  WID.wTitle.textContent=m?m.title:"\u2014";
  WID.wArtist.textContent=m?m.artist:"\u2014";
  WID.wTech.innerHTML=m?(esc(m.streaminfo.sample_rate/1000+" kHz / "+m.streaminfo.bits+"-BIT / "+(m.streaminfo.channels===2?"STEREO":m.streaminfo.channels+"CH")+" / "+(m.format||"FLAC").toUpperCase())+
    ' <span class="badge '+(m.lossless===false?"lossy":"lossless")+'">'+(m.lossless===false?"LOSSY":"LOSSLESS")+"</span>"):"\u2014";
  const showTech=window.getCfg?window.getCfg("tech")!==false:true;
  WID.wTech.style.display=showTech?"":"none";
}
function paintState(){
  WID.wIcoPlay.style.display=S.playing?"none":"block";
  WID.wIcoPause.style.display=S.playing?"block":"none";
  WID.wShuf.classList.toggle("on",S.shuffle);
  WID.wRep.classList.toggle("on",S.repeat!=="off");
  WID.wPin.classList.toggle("on",S.pinned);
  /* duration mirrors durSec() (owner element / meta fallback) */
  S.dur=durSec();
  WID.wTime.textContent=fmt(S.pos)+" / "+fmt(S.dur);
}
function paintLyricsStatus(){
  /* lyrics host visible whenever the preset needs it or lyrics exist */
  const wantLyrics=(window.getCfg&&window.getCfg("widgetLyrics"))||S.lyrics.length>0||S.lyricsPlain||S.lyricsStatus==="searching";
  WID.wLyrHost.style.display=(W.preset==="strip")?"none":(wantLyrics?"flex":"none");
  if(S.lyrics.length){
    WID.wLyrStatus.innerHTML='<span class="badge lossless">'+(S._lyricsSynced===false?"PLAIN":"SYNCED")+"</span>";
    WID.wLyrView.style.display="";
  }else if(S.lyricsPlain){
    /* unsynced lyrics: same pane, no highlight (mirrors main's paintLyrStatus) */
    WID.wLyrStatus.innerHTML='<span class="badge lossless">PLAIN</span>';
    WID.wLyrView.style.display="";
  }else if(S.lyricsStatus==="searching"){
    WID.wLyrStatus.innerHTML='<span class="h-caps mono">SEARCHING\u2026</span>';
    WID.wLyrView.style.display="";
  }else{
    WID.wLyrStatus.innerHTML='<span class="h-caps mono">NO LYRICS</span> <button class="btn" id="wLrcRetry">RETRY</button>';
    const b=document.getElementById("wLrcRetry");
    if(b)b.onclick=()=>emitCmd({cmd:"lrcfetch",path:S.meta&&S.meta.path,artist:S.meta&&S.meta.artist,title:S.meta&&S.meta.title});
    WID.wLyrView.style.display="none";
  }
}
function paintAll(){paintInfo();paintArt();paintState();paintLyricsStatus()}
function buildLyricsDom(){buildLyrics()}
function paintVolDom(){
  const vb=WID.wVol;if(!vb)return;
  const leds=vb.querySelector(".vol-leds");
  if(leds&&getComputedStyle(leds).display!=="none"){
    fitCanvas(leds);
    const g=leds.getContext("2d");
    if(g){
      const Wc=leds.width,Hc=leds.height;
      g.clearRect(0,0,Wc,Hc);
      const n=Math.max(4,Math.floor(Wc/9));
      const segW=Math.max(3,Math.floor(Wc/n)-3);
      for(let i=0;i<n;i++){
        /* riso light prints LEDs in the spot ink (accent is never a fill) */
        let on=ACC.cur;
        try{
          if(getComputedStyle(document.documentElement).getPropertyValue("--dither-ink").trim()==="1")
            on=themeCanvas.ink;
        }catch(_){}
        g.fillStyle=(S.vol>=(i+1)/n-.01)?css(on,.95):css(themeCanvas.off,.9);
        g.fillRect(i*(segW+3),1,segW,Hc-2);
      }
    }
  }
  const gel=vb.querySelector(".vol-gel");
  if(gel)vb.style.setProperty("--val",(S.vol*100).toFixed(1)+"%");
}
function drawSeekLed(cv,p){
  /* dot-matrix spectrum strip — same renderer as the main window
     (bars arrive via halftone:bars; the widget is a pure viewer).
     common.js loads BEFORE widget.js, so the shared renderer is already
     defined here — no late binding, no self-recursion possible. */
  if(typeof drawSeekLedShared==="function"){drawSeekLedShared(cv,p);return}
  fitCanvas(cv);
  const g=cv.getContext("2d");if(!g)return;
  const Wc=cv.width,Hc=cv.height;
  g.clearRect(0,0,Wc,Hc);
  const segW=Math.max(3,Math.round(Wc/60));
  const gap=2;
  const n=Math.max(1,Math.floor(Wc/(segW+gap)));
  const litN=Math.floor(p*n);
  for(let i=0;i<n;i++){
    g.fillStyle=i<=litN?css(ACC.cur,.95):css(themeCanvas.off,.9);
    g.fillRect(i*(segW+gap),1,segW,Hc-2);
  }
  g.fillStyle=css(themeCanvas.ink,.9);
  g.fillRect(clamp(Math.round(p*Wc)-1,0,Wc-2),0,2,Hc);
}
/* (shared renderer is drawSeekLedShared from common.js — loaded above) */
/* single rAF loop lives in common.js; hook the widget's paints into it */
window.htPagePaint=function(){
  /* keep time/pos fresh between syncs (viewer smoothing owns S.pos) */
  S.pos=posSec();S.dur=durSec();
  const p=S.dur?clamp(S.pos/S.dur,0,1):0;
  const cv=WID.wSeek.querySelector(".seek-led");
  if(cv&&getComputedStyle(cv).display!=="none")drawSeekLed(cv,p);
  const gel=WID.wSeek.querySelector(".seek-gel");
  if(gel&&getComputedStyle(gel).display!=="none"&&typeof paintGel==="function")paintGel(WID.wSeek,p);
  else WID.wSeek.style.setProperty("--val",(p*100).toFixed(2)+"%");
  paintVolDom();
  if(S._img&&WID.wDither.style.display!=="none")drawDither(WID.wDither,S._img);
  lyricFollow();
};

/* ---------- controls ---------- */
WID.wPlay.onclick=()=>emitCmd({cmd:"play"});
WID.wNext.onclick=()=>emitCmd({cmd:"next"});
WID.wPrev.onclick=()=>emitCmd({cmd:"prev"});
WID.wShuf.onclick=()=>emitCmd({cmd:"shuffle"});
WID.wRep.onclick=()=>emitCmd({cmd:"repeat"});
WID.wPin.onclick=()=>{window.setCfg&&window.setCfg("widgetPin",!S.pinned);emitCmd({cmd:"pin",v:!S.pinned})};
WID.wMain.onclick=async()=>{
  try{
    if(T&&T.window){
      const all=await T.window.getAllWindows();
      const main=all.find(w=>w.label==="main");
      if(main){await main.show();await main.setFocus()}
    }
  }catch(e){console.warn(e)}
};
/* widget close = REAL quit (load-bearing: main's close only hides) */
WID.wClose.onclick=async()=>{
  try{window.curWin&&window.curWin.destroy()}catch(e){console.warn(e)}
};
/* seek: pointer math via getBoundingClientRect (transform-aware) */
(function wireSeek(){
  const sk=WID.wSeek;
  const frac=e=>{
    const r=sk.getBoundingClientRect();
    return clamp((e.clientX-r.left)/Math.max(1,r.width),0,1);
  };
  sk.addEventListener("pointerdown",e=>{
    try{sk.setPointerCapture(e.pointerId)}catch(_){}
    W._drag=true;W._dragP=frac(e);
  });
  sk.addEventListener("pointermove",e=>{if(W._drag)W._dragP=frac(e)});
  const end=e=>{
    if(!W._drag)return;
    emitCmd({cmd:"seek",t:(W._dragP||0)*(S.dur||0)});
    W._drag=false;W._dragP=null;
  };
  sk.addEventListener("pointerup",end);
  sk.addEventListener("pointercancel",end);
  sk.addEventListener("wheel",ev=>{ev.preventDefault();emitCmd({cmd:"nudge",d:ev.deltaY<0?5:-5})},{passive:false});
})();
/* volume control (rebuilt per theme token) */
function buildVolDom(){
  const vb=WID.wVol;
  if(vb._built===themeVolStyle)return;
  vb._built=themeVolStyle;
  vb.innerHTML="";
  if(themeVolStyle==="gel"){
    const wrap=document.createElement("div");wrap.className="vol-gel";
    wrap.innerHTML='<div class="track"><div class="fill"></div><div class="knob"></div></div>';
    vb.appendChild(wrap);
  }else{
    const cv=document.createElement("canvas");cv.className="vol-leds";
    vb.appendChild(cv);
  }
}
(function wireVol(){
  const vb=WID.wVol;
  const set=e=>{
    const leds=vb.querySelector(".vol-leds"),gel=vb.querySelector(".vol-gel");
    const target=(leds&&getComputedStyle(leds).display!=="none")?leds:gel;
    if(!target)return;
    const r=target.getBoundingClientRect();
    emitCmd({cmd:"vol",v:clamp((e.clientX-r.left)/Math.max(1,r.width),0,1)});
  };
  let down=false;
  vb.addEventListener("pointerdown",e=>{down=true;try{vb.setPointerCapture(e.pointerId)}catch(_){}set(e)});
  vb.addEventListener("pointermove",e=>down&&set(e));
  vb.addEventListener("pointerup",()=>down=false);
  vb.addEventListener("pointercancel",()=>down=false);
})();
addEventListener("keydown",e=>{if(e.code==="Space"&&!e.target.closest("input")){e.preventDefault();emitCmd({cmd:"play"})}});
addEventListener("wheel",e=>{
  /* volume-on-wheel only outside scrollables (lyrics scroll freely) and
     only when the wheel is actually over the widget chrome — events from
     nested elements (seek strip has its own handler) must not double-fire */
  if(e.target.closest(".lyr-view,.w-seek,.w-sheet,.menu"))return;
  if(!(e.target===document.body||e.target===WID.stage||WID.stage.contains(e.target)))return;
  e.preventDefault();
  emitCmd({cmd:"vol",v:clamp(S.vol+(e.deltaY<0?.05:-.05),0,1)});
},{passive:false});

/* ============================================================
   WIDGET SHEET (⚙) — rendered in the UNSCALED overlay
   ============================================================ */
function openWidgetSheet(){
  const body=WID.wSheetBody;
  body.innerHTML="";
  const seg=document.createElement("div");seg.className="seg";seg.style.width="100%";
  [["card","CARD"],["strip","STRIP"],["square","SQUARE"],["lyrics","LYRICS"]].forEach(([v,l])=>{
    const b=document.createElement("button");b.textContent=l;b.classList.toggle("on",W.preset===v);
    b.style.flex="1";b.onclick=()=>{window.setCfg&&window.setCfg("widgetPreset",v);closeSheet()};
    seg.appendChild(b);
  });
  body.appendChild(seg);
  /* widget-scoped settings rows (generated from the schema) */
  const host=document.createElement("div");host.style.width="100%";
  body.appendChild(host);
  if(window.htSettings){
    window.htSettings.surface="widget";
    window.htSettings.render(host);
  }
  const toggles=document.createElement("div");
  toggles.style.cssText="display:flex;gap:6px;flex-wrap:wrap;padding-top:8px";
  const mk=(label,key,cur)=>{const b=document.createElement("button");b.className="pill"+(cur?" active":"");
    b.textContent=label;b.onclick=()=>{window.setCfg&&window.setCfg(key,!cur);closeSheet();openWidgetSheet()};return b};
  toggles.appendChild(mk("LYRICS","widgetLyrics",!!(window.getCfg&&window.getCfg("widgetLyrics"))));
  toggles.appendChild(mk("SEEK","widgetSeek",window.getCfg?window.getCfg("widgetSeek")!==false:true));
  toggles.appendChild(mk("PIN","widgetPin",S.pinned));
  body.appendChild(toggles);
  WID.wSheet.classList.add("open");
}
function closeSheet(){WID.wSheet.classList.remove("open")}
WID.wGear.onclick=e=>{e.stopPropagation();WID.wSheet.classList.contains("open")?closeSheet():openWidgetSheet()};
document.addEventListener("pointerdown",e=>{
  if(!WID.wSheet.classList.contains("open"))return;
  if(WID.wSheet.contains(e.target)||WID.wGear.contains(e.target))return;
  closeSheet();
});
/* right-click: preset picker + widget settings + hide */
document.addEventListener("contextmenu",e=>{
  if(e.target.closest("input,select,.w-sheet"))return;
  e.preventDefault();
  openCtx(e.clientX,e.clientY,[
    {label:"PRESET: CARD",checked:W.preset==="card",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","card")}},
    {label:"PRESET: STRIP",checked:W.preset==="strip",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","strip")}},
    {label:"PRESET: SQUARE",checked:W.preset==="square",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","square")}},
    {label:"PRESET: LYRICS",checked:W.preset==="lyrics",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","lyrics")}},
    {sep:1},
    {label:"WIDGET SETTINGS\u2026",onClick:()=>openWidgetSheet()},
    {label:"HIDE WIDGET",onClick:()=>{try{window.curWin&&window.curWin.hide()}catch(_){}}},
  ]);
});
/* right-click ON THE ART itself: "Change cover art" (task 10) — the owner
   main window owns the sheet/import/search flows, so it forwards via cmd.
   NOTE: no stopPropagation here — the document-level contextmenu handler
   opens the full preset menu; the art case is expressed by prepending the
   cover item so both fire exactly once. stopPropagation made the art's
   menu silently swallow the document one in the real app (v0.2.1 bug:
   right-click on widget art did nothing). */
WID.wArt.addEventListener("contextmenu",e=>{
  if(!S.meta||!S.meta.path)return;   /* fall through to the default menu */
  e.preventDefault();
  openCtx(e.clientX,e.clientY,[
    {label:"CHANGE COVER ART",onClick:()=>emitCmd({cmd:"cover",path:S.meta.path})},
    {sep:1},
    {label:"PRESET: CARD",checked:W.preset==="card",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","card")}},
    {label:"PRESET: STRIP",checked:W.preset==="strip",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","strip")}},
    {label:"PRESET: SQUARE",checked:W.preset==="square",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","square")}},
    {label:"PRESET: LYRICS",checked:W.preset==="lyrics",onClick:()=>{window.setCfg&&window.setCfg("widgetPreset","lyrics")}},
    {sep:1},
    {label:"WIDGET SETTINGS\u2026",onClick:()=>openWidgetSheet()},
    {label:"HIDE WIDGET",onClick:()=>{try{window.curWin&&window.curWin.hide()}catch(_){}}},
  ]);
});

/* ============================================================
   SETTINGS-DRIVEN WIDGET BEHAVIOUR
   ============================================================ */
function applyWidgetPreset(v){
  W.preset=["card","strip","square","lyrics"].includes(v)?v:"card";
  WID.stage.className="widget-stage wp-"+W.preset;
  scheduleStage();
  const wantLyrics=W.preset==="lyrics"||(W.preset!=="strip"&&S.lyricsOpen);
  WID.wLyrHost.style.display=(W.preset==="strip")?"none":(wantLyrics?"flex":"none");
  requestRedraw();
}
window.applyWidgetPreset=applyWidgetPreset;
function applyWidgetOpacity(v){
  WID.stage.style.background="color-mix(in srgb,var(--panel-bg) "+Math.round(v)+"%,transparent)";
}
window.applyWidgetOpacity=applyWidgetOpacity;
function applyWidgetPin(v){try{window.curWin&&window.curWin.setAlwaysOnTop(!!v)}catch(e){}}
window.applyWidgetPin=applyWidgetPin;
function applyWidgetLyrics(v){
  S.lyricsOpen=!!v;
  /* the lyrics PRESET is the pane: only a preset change can hide it */
  if(W.preset!=="strip"&&W.preset!=="lyrics")WID.wLyrHost.style.display=v?"flex":"none";
}
window.applyWidgetLyrics=applyWidgetLyrics;
function applyWidgetSeek(v){WID.wSeek.style.display=v?"":"none"}
window.applyWidgetSeek=applyWidgetSeek;
function applyWidgetVol(v){WID.wVol.style.display=v?"":"none"}
window.applyWidgetVol=applyWidgetVol;
function updateSwitchAttrs(){
  WID.wSeek.setAttribute("data-seek",themeSeekStyle);
  WID.wVol.setAttribute("data-vol",themeVolStyle);
}

/* ---------- owner broadcasts ---------- */
document.addEventListener("halftone:track",()=>{paintInfo();loadArt()});
document.addEventListener("halftone:state",paintAll);
document.addEventListener("halftone:lyrics",()=>{paintLyricsStatus();buildLyricsDom()});
/* after ANY lyrics (re)build: preserve scroll position when the content
   is identical (applySync re-sends lyrics on EVERY sync — play, seek,
   volume — and common.js buildLyrics() wipes innerHTML each time; that
   wipe is what made the pane impossible to scroll by hand). Only a
   genuinely NEW line set re-centres on the active line.
   MutationObserver because every rebuild path (applySync on sync,
   lyrics fetch, retry) runs inside common.js closures — this is the
   one widget-owned hook that sees them all. */
(function(){
  const wrap=document.getElementById("lyrWrap"),view=WID.wLyrView;
  if(!wrap||!view||!window.MutationObserver)return;
  const settle=()=>{
    requestAnimationFrame(()=>{
      try{
        const lines=wrap.children;
        const first=lines[0],last=lines[lines.length-1];
        const sig=lines.length+"|"+(first?first.dataset.t:"")+"|"+(last?last.dataset.t:"");
        if(S._lyrSig===sig)return;             /* identical rebuild: keep scroll */
        S._lyrSig=sig;
        if(!lines.length)return;
        const idx=Math.max(0,S.lidx);
        const ln=lines[Math.min(idx,lines.length-1)];
        view.scrollTop=Math.max(0,ln.offsetTop-view.clientHeight/2+ln.offsetHeight/2);
      }catch(_){}
    });
  };
  new MutationObserver(settle).observe(wrap,{childList:true});
})();
/* manual scroll detection (widget surface): delegate to common.js's
   canonical lyrPause/lyrResume so the widget behaves EXACTLY like main —
   one timer (3s resume), the ● LIVE pill, mask-off `.manual` state.
   common.js wireLyrScroll already binds #wLyrView (same element), so
   this block only marks pause on scrollbar press-drags, which the
   common.js gutter heuristic can miss in the transform-scaled stage. */
(function(){
  const view=WID.wLyrView;if(!view)return;
  view.addEventListener("pointerdown",e=>{
    const line=e.target.closest&&e.target.closest(".lyric-line");
    if(!line&&window.lyrPause)lyrPause();
  });
})();
/* scale settings changed from the MAIN window: widget.js closure vars W
   are only synced from the store at boot, so a live change of
   widgetScaleMode / widgetScale in main's settings never re-staged
   (fixed mode silently kept the old mode/% until widget reload) */
document.addEventListener("halftone:settings",e=>{
  const d=e.detail||{},vals=d.values||{};
  try{
    if("widgetScaleMode" in vals)W.scaleMode=vals.widgetScaleMode;
    if("widgetScale" in vals)W.scalePct=+vals.widgetScale;
    if("widgetScaleMode" in vals||"widgetScale" in vals)scheduleStage();
  }catch(_){}
});

/* ---------- boot ---------- */
(async function boot(){
  if(window.__htSettingsReady){await window.__htSettingsReady}
  else if(window.htSettings){await window.htSettings.ready()}
  /* QA override (?preset= in the frame URL) — outside the htSettings
     guard so it applies even if the settings boot fails */
  const qaPreset=(new URLSearchParams(location.search).get("preset"));
  if(window.htSettings){
    const st=window.htSettings.all()||{};
    W.scaleMode=st.widgetScaleMode||"fit";
    W.scalePct=st.widgetScale||100;
    applyWidgetPreset(qaPreset||st.widgetPreset||"card");
    applyWidgetOpacity(st.widgetBgOpacity!=null?st.widgetBgOpacity:92);
    if(st.widgetSeek===false)applyWidgetSeek(false);
    if(st.widgetVol===false)applyWidgetVol(false);
  }else if(qaPreset){
    applyWidgetPreset(qaPreset);
  }
  updateSwitchAttrs();
  buildVolDom();
  try{const r=await invoke("library_snapshot");const arr=Array.isArray(r)?r:(r&&r.tracks);if(Array.isArray(arr))S.lib=arr}catch(e){}
  if(T&&T.event&&T.event.emit){try{T.event.emit("halftone:hello",{}).catch(()=>{})}catch(_){}}
  emitCmd({cmd:"hello"});
  applyStage();
  paintAll();
})();
/* drag region: native drag must NOT start over interactive surfaces —
   lyrics (click-to-seek + free scroll), transport, sheets, the seek
   strip. With Webview2 a native drag swallows pointer events, so the
   lyrics pane was unclickable/unscrollable (owner bugs 2+3). */
wireDrag(WID.stage,e=>!e.target.closest(
  ".lyr-view,.lyric-line,.w-seek,.w-transport,.vol,.btn,.btn-icon,.btn-orb,.w-sheet,.menu,input,select,button"));
