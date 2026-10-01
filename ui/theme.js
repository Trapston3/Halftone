/* ============================================================
   HALFTONE theme engine (foundation-owned)
   - Sets root attributes from the UI contract:
       data-theme, data-mode, data-motion, data-density,
       data-surface, data-nav (+ data-nav-labels)
   - "system" mode follows prefers-color-scheme LIVE.
   - Reads canvas tokens (--canvas-bg/off/ink) + switch tokens
     (--seek-style/--vol-style/--art-default/--ambient-default),
     caches them, dispatches halftone:theme.
   - Theme/mode switch animation: circular clip-path view
     transition from the pointer (fallback: html.theme-fading
     crossfade for WebKitGTK). Respects data-motion.
   ============================================================ */
(function(){
"use strict";
const T=window.__TAURI__;
const IS_OWNER=window.IS_OWNER!==false&&(!T||!T.window||!window.curWinLabel||window.curWinLabel!=="widget");

/* ---------- current resolved state (persisted through settings.js) ---------- */
const Theme={
  name:"analogue", mode:"system", resolved:"dark",
  motion:"full", density:"cozy", nav:"left", navLabels:"icons", navState:"expanded",
  canvas:{bg:[18,23,26],off:[35,42,46],ink:[242,232,207]},
  seekStyle:"led", volStyle:"leds", artDefault:"dither", ambientDefault:"dither"
};
window.Theme=Theme;

/* ---------- color parsing ---------- */
function parseColor(css){
  if(!css||css==="none")return null;
  let m=css.match(/rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i);
  if(m)return [+m[1],+m[2],+m[3]];
  const hex=css.trim();
  if(/^#[0-9a-f]{6}$/i.test(hex))return [parseInt(hex.slice(1,3),16),parseInt(hex.slice(3,5),16),parseInt(hex.slice(5,7),16)];
  if(/^#[0-9a-f]{3}$/i.test(hex))return [parseInt(hex[1]+hex[1],16),parseInt(hex[2]+hex[2],16),parseInt(hex[3]+hex[3],16)];
  return null;
}

/* ---------- read tokens from computed style ---------- */
function readTokens(){
  const cs=getComputedStyle(document.documentElement);
  const bg=parseColor(cs.getPropertyValue("--canvas-bg"))||Theme.canvas.bg;
  const off=parseColor(cs.getPropertyValue("--canvas-off"))||Theme.canvas.off;
  const ink=parseColor(cs.getPropertyValue("--canvas-ink"))||Theme.canvas.ink;
  const changed=JSON.stringify([bg,off,ink])!==JSON.stringify(Theme.canvas);
  Theme.canvas={bg,off,ink};
  Theme.seekStyle=(cs.getPropertyValue("--seek-style")||"led").trim()==="gel"?"gel":"led";
  Theme.volStyle=(cs.getPropertyValue("--vol-style")||"leds").trim()==="gel"?"gel":"leds";
  Theme.artDefault=(cs.getPropertyValue("--art-default")||"dither").trim()==="real"?"real":"dither";
  const amb=(cs.getPropertyValue("--ambient-default")||"dither").trim();
  Theme.ambientDefault=["halo","aurora","off"].includes(amb)?amb:"dither";
  return changed;
}

/* ---------- apply root attributes ---------- */
function applyAttrs(){
  const r=document.documentElement;
  const set=(k,v)=>{if(r.getAttribute(k)!==v)r.setAttribute(k,v)};
  set("data-theme",Theme.name);
  set("data-mode",Theme.resolved);
  set("data-motion",Theme.motion);
  set("data-density",Theme.density);
  set("data-surface",document.body.dataset.surface||"main");
  if(document.body.dataset.surface==="main")set("data-nav",Theme.nav);
  set("data-nav-labels",Theme.navLabels);
  set("data-nav-state",Theme.navState);
  /* maximized / fullscreen windows lose their rounded corners */
  set("data-max",window.__HT_MAX?"1":"0");
  /* keep legacy aliases alive for any old CSS */
  const st=r.style;
  st.setProperty("--cream","var(--fg)");
  st.setProperty("--dim","var(--fg-2)");
  st.setProperty("--dim2","var(--fg-3)");
  st.setProperty("--bg2","var(--bg-2)");
  st.setProperty("--bg3","var(--bg-3)");
  st.setProperty("--line2","var(--line-2)");
  st.setProperty("--mono","var(--font-mono)");
  st.setProperty("--sans","var(--font-ui)");
  st.setProperty("--r-w","var(--R-w,14px)");
  st.setProperty("--e-pop","var(--shadow-1)");
}

/* ---------- broadcast + notify ----------
   applyAttrs FIRST, then readTokens: token values are read from the
   computed style of :root, which only resolves to the new theme's CSS
   block after data-theme/data-mode changed. Reading before applying
   served every switch one theme late (digital booted with analogue's
   --seek-style:led → gel seek hidden, LED canvas hidden → no seekbar). */
function dispatchTheme(){
  applyAttrs();
  readTokens();
  document.dispatchEvent(new CustomEvent("halftone:theme",{detail:{
    theme:Theme.name,mode:Theme.resolved,motion:Theme.motion,density:Theme.density,
    nav:Theme.nav,canvas:{...Theme.canvas},seek:Theme.seekStyle,vol:Theme.volStyle,
    art:Theme.artDefault,ambient:Theme.ambientDefault}}));
}

/* ---------- system mode follows the OS live ---------- */
const mq=window.matchMedia?window.matchMedia("(prefers-color-scheme: dark)"):null;
if(mq&&mq.addEventListener){
  mq.addEventListener("change",()=>{
    if(Theme.mode!=="system")return;
    Theme.resolved=mq.matches?"dark":"light";
    switchAnimated(null,null);  /* no pointer — quiet swap */
  });
}

/* ---------- switch animation (circular reveal / crossfade) ---------- */
let vtBusy=false;
function setVar(key,val){document.documentElement.style.setProperty(key,val)}
function switchAnimated(x,y){
  const motion=Theme.motion;
  if(motion==="off"||vtBusy){dispatchTheme();return}
  if(motion==="reduced"||!document.startViewTransition){
    /* fallback: crossfade via html.theme-fading (WebKitGTK, reduced) */
    dispatchTheme();
    if(motion==="reduced")return;
    const r=document.documentElement;
    r.classList.add("theme-fading");
    setTimeout(()=>r.classList.remove("theme-fading"),450);
    return;
  }
  if(x==null){const c=innerWidth/2,d=innerHeight/2;x=c;y=d}
  setVar("--vt-x",x+"px");setVar("--vt-y",y+"px");
  vtBusy=true;
  const dt=document.startViewTransition(()=>{dispatchTheme()});
  dt.ready.then(()=>{
    const r=Math.hypot(Math.max(x,innerWidth-x),Math.max(y,innerHeight-y));
    document.documentElement.animate(
      {clipPath:[`circle(0px at ${x}px ${y}px)`,`circle(${r}px at ${x}px ${y}px)`]},
      {duration:600,easing:"cubic-bezier(.2,.9,.2,1)",pseudoElement:"::view-transition-new(root)"}
    );
  }).catch(()=>{}).finally(()=>setTimeout(()=>{vtBusy=false},650));
}
window.htThemeSwitch=switchAnimated;
/* quiet re-apply of root attributes (no reveal) — e.g. maximize flips data-max */
window.htThemeApply=function(){dispatchTheme()};

/* ---------- public API (settings.js calls these) ----------
   QA/harness boot (?qa=1) applies state silently: no circular
   reveal — otherwise a headless screenshot taken before the ~600ms
   view-transition settles shows the shell mid-reveal (clipped shell
   = "dead band" artifact). Interactive switches keep the reveal. */
function qaQuiet(){return new URLSearchParams(location.search).has("qa")}
window.htSetTheme=function(name,x,y){
  if(!["analogue","digital"].includes(name))name="analogue";
  if(name!==Theme.name){Theme.name=name;if(qaQuiet())dispatchTheme();else switchAnimated(x,y);syncOther()}
};
window.htSetMode=function(mode,x,y){
  if(!["system","light","dark"].includes(mode))mode="system";
  const resolved=mode==="system"?((mq&&mq.matches)?"dark":"light"):mode;
  /* no-change guard: breaks cross-window echo loops (settings applyAll) */
  if(Theme.mode===mode&&Theme.resolved===resolved)return;
  Theme.mode=mode;Theme.resolved=resolved;
  if(qaQuiet())dispatchTheme();else switchAnimated(x,y);syncOther();
};
window.htSetMotion=function(m){
  if(!["full","reduced","off"].includes(m))m="full";
  if(Theme.motion===m)return;
  Theme.motion=m;dispatchTheme();syncOther();
};
window.htSetDensity=function(d){
  if(!["compact","cozy","comfy"].includes(d))d="cozy";
  if(Theme.density===d)return;
  Theme.density=d;dispatchTheme();syncOther();
};
window.htSetNav=function(nav){if(!["left","right","top","bottom","hidden"].includes(nav))nav="left";
  if(Theme.nav===nav)return;
  Theme.nav=nav;dispatchTheme();syncOther()};
window.htSetNavLabels=function(l){const v=l==="labels"?"labels":"icons";
  if(Theme.navLabels===v)return;
  Theme.navLabels=v;dispatchTheme();syncOther()};
/* nav rail state: expanded | collapsed | hidden (icons-only / fully hidden) */
window.htSetNavState=function(st){
  if(!["expanded","collapsed","hidden"].includes(st))st="expanded";
  if(Theme.navState===st)return;
  Theme.navState=st;dispatchTheme();syncOther()};

/* ---------- both windows switch together ---------- */
function syncOther(){
  if(!(T&&T.event&&T.event.emit))return;
  try{T.event.emit("halftone:settings",{keys:["theme","mode","motion","density","nav","navLabels","navState"],
    themeState:{theme:Theme.name,mode:Theme.resolved,motion:Theme.motion,density:Theme.density,
      nav:Theme.nav,navLabels:Theme.navLabels,navState:Theme.navState}}).catch(()=>{})}catch(_){}
}
/* receive: the other window applied theme state first — mirror it exactly */
if(T&&T.event&&T.event.listen){
  try{T.event.listen("halftone:settings",e=>{
    const p=e.payload||{},ts=p.themeState;
    if(!ts)return;
    let quiet=false;
    if(ts.theme&&ts.theme!==Theme.name){Theme.name=ts.theme;quiet=true}
    if(ts.mode){Theme.mode=ts.mode;Theme.resolved=ts.resolved||ts.mode;quiet=true}
    if(ts.motion&&ts.motion!==Theme.motion){Theme.motion=ts.motion;quiet=true}
    if(ts.density&&ts.density!==Theme.density){Theme.density=ts.density;quiet=true}
    if(ts.nav&&ts.nav!==Theme.nav){Theme.nav=ts.nav;quiet=true}
    if(ts.navLabels&&ts.navLabels!==Theme.navLabels){Theme.navLabels=ts.navLabels;quiet=true}
    if(ts.navState&&ts.navState!==Theme.navState){Theme.navState=ts.navState;quiet=true}
    if(quiet)dispatchTheme();
  }).catch(()=>{})}catch(_){}
}

/* ---------- boot ---------- */
readTokens();
applyAttrs();
dispatchTheme();
})();
