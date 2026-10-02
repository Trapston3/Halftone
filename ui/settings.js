/* ============================================================
   HALFTONE settings system (foundation-owned)

   ONE declarative SCHEMA drives everything: the generated
   settings UI (searchable, grouped, per-row reset), defaults,
   type coercion and live apply. The store is a nested object
   persisted through the Tauri `settings_load`/`settings_save`
   commands (one file shared by both windows), debounced 300ms.
   Every change broadcasts `halftone:settings` to the other
   window and applies live — no reload, ever.

   Row DOM (styled by themes):
   .set-row > .set-info(.set-label,.set-hint) + .set-ctl
   ============================================================ */
(function(){
"use strict";
const T=window.__TAURI__||window.__HT_TAURI_MOCK__||null;
/* invoke: same chain as common.js (mock first, then real Tauri) */
function invoke(cmd,args){
  args=args||{};
  if(window.__invoke&&window.__invoke._ht)return window.__invoke(cmd,args);
  if(T&&T.core&&T.core.invoke)return T.core.invoke(cmd,args);
  return Promise.resolve(null);
}
/* __ACC_SILENT: set by common.js setAccentMode while it syncs the
   "accent" setting, preventing setAccentMode -> htSet -> apply ->
   setAccentMode infinite recursion. */

/* ============================================================
   SCHEMA — {key, group, label, hint?, type, options?, min/max/step?,
             default, surface:"main"|"widget"|"both", apply(v), alias?}
   alias = legacy localStorage["halftone.store"].cfg path to migrate from.
   ============================================================ */
const SCHEMA=[
/* ---------------- Appearance ---------------- */
{key:"theme",group:"Appearance",label:"Theme",type:"select",surface:"both",default:"analogue",
  options:[["analogue","Analogue (dither + LED)"],["digital","Digital (liquid glass)"]],
  apply:v=>window.htSetTheme&&window.htSetTheme(v)},
{key:"mode",group:"Appearance",label:"Color mode",type:"seg",surface:"both",default:"dark",
  options:[["system","SYS"],["light","LIGHT"],["dark","DARK"]],
  apply:v=>window.htSetMode&&window.htSetMode(v)},
{key:"palette",group:"Appearance",label:"Analogue palette",type:"select",surface:"both",default:"classic",
  hint:"Only applies when Theme = Analogue.",
  options:[["classic","Classic (default)"],["catppuccin","Catppuccin"],["tokyonight","Tokyo Night"],
           ["gruvbox","Gruvbox"],["nord","Nord"],["rosepine","Rosé Pine"],["dracula","Dracula"]],
  apply:v=>window.htSetPalette&&window.htSetPalette(v)},
{key:"accent",group:"Appearance",label:"Accent",type:"select",surface:"both",default:"album",
  options:[["album","From album art"],["mint","#66E0C2"],["sky","#6EC5E8"],["violet","#A78BFA"],
           ["rose","#F27DA0"],["amber","#E0B966"],["red","#E06666"],["custom","Custom"]],
  alias:"accentMode",
  apply:v=>window.setAccentMode&&!window.__ACC_SILENT&&window.setAccentMode(v)},
{key:"accentCustom",group:"Appearance",label:"Custom accent color",type:"color",surface:"both",default:"#66E0C2",
  apply:v=>{if(window.setCustomAccent)window.setCustomAccent(v)}},
{key:"art",group:"Appearance",label:"Album art style",type:"seg",surface:"both",default:"theme",
  options:[["theme","THEME"],["dither","DITHER"],["real","REAL"]],
  apply:v=>window.applyArtSetting&&window.applyArtSetting(v)},
{key:"grid",group:"Appearance",label:"Dither grid density",type:"select",surface:"both",default:32,
  options:[[32,"32 — coarse"],[48,"48 — fine"],[64,"64 — ultra"]],
  apply:v=>window.setDitherDensity&&window.setDitherDensity(+v)},
{key:"ambient",group:"Appearance",label:"Ambient background",type:"select",surface:"main",default:"theme",
  options:[["theme","Theme default"],["dither","Dither bloom"],["halo","Halo glow"],["aurora","Aurora"],["off","Off"]],
  apply:v=>window.applyAmbientSetting&&window.applyAmbientSetting(v)},
{key:"glassBlur",group:"Appearance",label:"Glass blur",type:"range",surface:"both",default:24,min:0,max:40,step:1,
  apply:v=>document.documentElement.style.setProperty("--glass-blur",(+v).toFixed(0)+"px")},
{key:"radiusScale",group:"Appearance",label:"Corner radius scale",type:"range",surface:"both",default:1,min:0.5,max:1.5,step:0.05,
  apply:v=>document.documentElement.style.setProperty("--radius-scale",(+v).toFixed(2))},
{key:"fontScale",group:"Appearance",label:"UI font scale",type:"range",surface:"both",default:1,min:0.85,max:1.3,step:0.05,
  apply:v=>document.documentElement.style.setProperty("--font-scale",(+v).toFixed(2))},
{key:"density",group:"Appearance",label:"List density",type:"seg",surface:"both",default:"cozy",
  options:[["compact","COMPACT"],["cozy","COZY"],["comfy","COMFY"]],
  apply:v=>window.htSetDensity&&window.htSetDensity(v)},
{key:"motion",group:"Appearance",label:"Motion",type:"seg",surface:"both",default:"full",
  options:[["full","FULL"],["reduced","REDUCED"],["off","OFF"]],
  apply:v=>window.htSetMotion&&window.htSetMotion(v)},

/* ---------------- Layout (main window) ---------------- */
{key:"nav",group:"Layout",label:"Navigation position",type:"select",surface:"main",default:"left",
  options:[["left","Left rail"],["right","Right rail"],["top","Top pill tabs"],["bottom","Floating bottom dock"],["hidden","Hidden (Ctrl+K palette)"]],
  apply:v=>window.htSetNav&&window.htSetNav(v)},
{key:"navLabels",group:"Layout",label:"Nav labels",type:"seg",surface:"main",default:"icons",
  options:[["icons","ICONS"],["labels","ICONS + LABELS"]],
  apply:v=>window.htSetNavLabels&&window.htSetNavLabels(v)},
{key:"navState",group:"Layout",label:"Nav rail state",type:"seg",surface:"main",default:"expanded",
  options:[["expanded","FULL"],["collapsed","ICONS"],["hidden","HIDDEN"]],
  apply:v=>window.htSetNavState&&window.htSetNavState(v)},
{key:"queueSide",group:"Layout",label:"Queue panel",type:"select",surface:"main",default:"off",
  options:[["off","Off (popover only)"],["left","Left"],["right","Right"]],
  apply:v=>window.applyQueueSide&&window.applyQueueSide(v)},
{key:"npLayout",group:"Layout",label:"Now-playing layout",type:"select",surface:"main",default:"split",
  options:[["split","Split — art + lyrics"],["stacked","Stacked"],["hero","Hero art"]],
  apply:v=>window.applyNpLayout&&window.applyNpLayout(v)},
{key:"tech",group:"Layout",label:"Show tech line",type:"toggle",surface:"both",default:true,alias:"tech",
  apply:v=>window.applyTechLine&&window.applyTechLine(v)},
{key:"miniBar",group:"Layout",label:"Now-playing mini-bar",type:"select",surface:"main",default:"bottom",
  options:[["bottom","Bottom"],["top","Top"],["off","Off"]],
  apply:v=>window.applyMiniBar&&window.applyMiniBar(v)},
{key:"defaultView",group:"Layout",label:"Library default view",type:"select",surface:"main",default:"tracks",
  options:[["tracks","Songs"],["albums","Albums"],["liked","Liked"],["playlists","Playlists"]],
  apply:()=>{}},

/* ---------------- Widget ---------------- */
{key:"widgetPreset",group:"Widget",label:"Widget layout preset",type:"select",surface:"widget",default:"card",
  options:[["card","Card"],["strip","Compact strip"],["square","Square art"],["lyrics","Lyrics focus"]],
  apply:v=>window.applyWidgetPreset&&window.applyWidgetPreset(v)},
{key:"widgetScaleMode",group:"Widget",label:"Widget scale mode",type:"select",surface:"widget",default:"fit",
  options:[["fit","Fit to window"],["fixed","Fixed %"]],
  apply:v=>window.applyWidgetScale&&window.applyWidgetScale()},
{key:"widgetScale",group:"Widget",label:"Fixed widget scale",type:"range",surface:"widget",default:100,min:70,max:200,step:5,
  apply:()=>window.applyWidgetScale&&window.applyWidgetScale()},
{key:"widgetBgOpacity",group:"Widget",label:"Widget background opacity",type:"range",surface:"widget",default:92,min:40,max:100,step:2,
  apply:v=>{const el=document.querySelector(".shell");if(el)el.style.setProperty("--panel-alpha",(v/100).toFixed(2));
    if(el)el.style.backgroundColor="";window.applyWidgetOpacity&&window.applyWidgetOpacity(v)}},
{key:"widgetPin",group:"Widget",label:"Always on top",type:"toggle",surface:"widget",default:true,alias:"pinned",
  apply:v=>window.applyWidgetPin&&window.applyWidgetPin(!!v)},
{key:"widgetLyrics",group:"Widget",label:"Show lyrics pane",type:"toggle",surface:"widget",default:false,alias:"lyricsOpen",
  apply:v=>window.applyWidgetLyrics&&window.applyWidgetLyrics(!!v)},
{key:"widgetSeek",group:"Widget",label:"Show seek bar",type:"toggle",surface:"widget",default:true,
  apply:v=>window.applyWidgetSeek&&window.applyWidgetSeek(!!v)},
{key:"widgetVol",group:"Widget",label:"Show volume",type:"toggle",surface:"widget",default:true,
  apply:v=>window.applyWidgetVol&&window.applyWidgetVol(!!v)},

/* ---------------- Lyrics ---------------- */
{key:"lyricsAuto",group:"Lyrics",label:"Auto-fetch lyrics online",type:"toggle",surface:"both",default:true,alias:"lyricsAuto",
  apply:v=>window.applyLyricsAuto&&window.applyLyricsAuto(!!v)},
{key:"lyricsFontSize",group:"Lyrics",label:"Lyrics font size",type:"range",surface:"both",default:28,min:14,max:48,step:1,
  apply:v=>document.documentElement.style.setProperty("--lyr-fs",(+v/16).toFixed(3)+"rem")},
{key:"lyricsAlign",group:"Lyrics",label:"Lyrics alignment",type:"seg",surface:"both",default:"left",
  options:[["center","CENTER"],["left","LEFT"]],
  apply:v=>document.documentElement.style.setProperty("--lyr-align",v==="center"?"center":"left")},
{key:"lyricsOffset",group:"Lyrics",label:"Sync offset",type:"range",surface:"both",default:0,min:-2000,max:2000,step:50,
  hint:"Shifts synced-lyric timing in ms. Positive = lyrics LATER (lines highlight after their timestamp); negative = earlier.",
  apply:v=>{document.documentElement.style.setProperty("--lyr-offset",(+v|0));window.applyLyricsOffset&&window.applyLyricsOffset(+v)}},
{key:"lyricsDimPast",group:"Lyrics",label:"Dim past lines",type:"toggle",surface:"both",default:true,
  apply:v=>document.documentElement.classList.toggle("lyr-nodim",!v)},
{key:"lyricsShowMain",group:"Lyrics",label:"Show lyrics (main)",type:"toggle",surface:"main",default:true,alias:"lyrics",
  apply:v=>window.applyLyricsShowMain&&window.applyLyricsShowMain(!!v)},

/* ---------------- Playback ---------------- */
{key:"rg",group:"Playback",label:"ReplayGain mode",type:"select",surface:"both",default:"off",alias:"rg",
  options:[["off","Off"],["track","Track gain"],["album","Album gain"]],
  apply:v=>window.applyRgSetting&&window.applyRgSetting(v)},
{key:"sink",group:"Playback",label:"Output device",type:"device",surface:"both",default:"",alias:"sink",
  apply:v=>window.applySinkSetting&&window.applySinkSetting(v)},
{key:"resume",group:"Playback",label:"Resume on launch",type:"toggle",surface:"both",default:false,alias:"resumeOnLaunch",
  apply:()=>{}},
{key:"sleepTimer",group:"Playback",label:"Sleep timer",type:"select",surface:"main",default:"off",
  options:[["off","Off"],["15","15 min"],["30","30 min"],["60","60 min"],["track","End of track"]],
  apply:v=>window.applySleepSetting&&window.applySleepSetting(v)},

/* ---------------- Library ---------------- */
{key:"root",group:"Library",label:"Music folder",type:"folder",surface:"both",default:"",alias:"root",
  apply:v=>window.applyLibraryRoot&&window.applyLibraryRoot(v)},
{key:"watch",group:"Library",label:"Watch folder (auto-rescan)",type:"toggle",surface:"main",default:true,alias:"watch",
  apply:v=>window.applyWatchSetting&&window.applyWatchSetting(v)},
{key:"coverAutoNet",group:"Library",label:"Fetch missing cover art online",type:"toggle",surface:"main",default:true,
  hint:"Looks up art for tracks without embedded covers (iTunes / MusicBrainz).",
  apply:()=>{}},

/* ---------------- Actions ---------------- */
{key:"rescan",group:"Library",label:"Rescan library now",type:"action",surface:"main",default:"",
  apply:()=>window.rescanLibrary&&window.rescanLibrary()},
{key:"checkUpdates",group:"About",label:"Check for updates",type:"action",surface:"main",default:"",
  apply:()=>window.checkOta&&window.checkOta()},
];

/* legacy cfg-key → schema-key for one-time migration */
const ALIAS={};SCHEMA.forEach(s=>{if(s.alias)ALIAS[s.alias]=s.key});

/* ============================================================
   STORE
   ============================================================ */
const Store={
  v:null,            /* nested settings object */
  _t:null,
  surface:"main"
};
function defaults(){
  const o={};
  for(const s of SCHEMA)o[s.key]=s.default;
  return o;
}
function getIn(path){const ks=path.split(".");let o=Store.v;for(const k of ks){if(o==null)return undefined;o=o[k]}return o}
function setIn(path,val){const ks=path.split(".");let o=Store.v;for(let i=0;i<ks.length-1;i++){if(o[ks[i]]==null||typeof o[ks[i]]!=="object")o[ks[i]]={};o=o[ks[i]]}o[ks[ks.length-1]]=val}

function coerce(s,v){
  if(s.type==="range")return +v;
  if(s.type==="toggle")return !!v;
  if(s.type==="select"&&s.key==="grid")return +v;
  if(s.type==="select"||s.type==="seg")return String(v);
  return v;
}

/* ---------- persistence ---------- */
function scheduleSave(){
  clearTimeout(Store._t);
  Store._t=setTimeout(saveNow,300);
}
async function saveNow(){
  if(!Store.v)return;
  try{await invoke("settings_save",{v:Store.v})}catch(e){console.warn("settings_save",e)}
}
window.htSaveSettingsNow=saveNow;

/* ---------- apply every setting live (boot + import) ----------
   _applyingRemote guards the whole apply phase so schema apply()
   callbacks that call htSet() don't re-emit and echo back. */
function applyAll(){
  if(!Store.v)return;
  _applyingRemote=true;
  try{
    for(const s of SCHEMA){
      if(s.type==="action")continue; /* buttons run on click only, never on boot/sync */
      let v=getIn(s.key);
      if(v===undefined){v=s.default;setIn(s.key,s.default)}
      try{s.apply&&s.apply(v)}catch(e){console.warn("apply",s.key,e)}
    }
  }finally{_applyingRemote=false}
}
window.htApplyAllSettings=applyAll;

/* ---------- one-time migration from old localStorage cfg ---------- */
function migrate(){
  /* QA/harness shots must be deterministic: the temp Chrome profile
     persists localStorage across navigations, so a light-mode scenario
     would re-theme every later dark scenario through this path. */
  if(new URLSearchParams(location.search).has("qa"))return;
  try{
    const raw=localStorage.getItem("halftone.store");
    if(!raw)return;
    const old=JSON.parse(raw);
    const cfg=old&&(old.cfg||old);
    if(cfg&&typeof cfg==="object"){
      let moved=false;
      for(const [k,v] of Object.entries(cfg)){
        const key=ALIAS[k]||k;
        const s=SCHEMA.find(x=>x.key===key);
        if(!s)continue;
        if(getIn(key)===s.default){setIn(key,coerce(s,v));moved=true}
      }
      if(moved)scheduleSave();
    }
    localStorage.setItem("halftone.store.migrated","1");
  }catch(e){console.warn("migrate",e)}
}

/* ---------- boot: load from backend ---------- */
async function boot(){
  Store.v=await (async()=>{
    try{
      const v=await invoke("settings_load");
      if(v&&typeof v==="object"&&!Array.isArray(v))return Object.assign(defaults(),v);
    }catch(e){console.warn("settings_load",e)}
    return defaults();
  })();
  migrate();
  applyAll();
  document.dispatchEvent(new CustomEvent("halftone:settings-ready",{detail:{store:Store.v}}));
}

/* ============================================================
   CHANGE API
   ============================================================ */
function set(key,value,opts){
  opts=opts||{};
  const s=SCHEMA.find(x=>x.key===key);
  if(!s)return;
  value=coerce(s,value);
  const prev=getIn(key);
  if(prev===value&&!opts.force)return;
  setIn(key,value);
  try{s.apply&&s.apply(value)}catch(e){console.warn("apply",key,e)}
  if(!opts.noSave)scheduleSave();
  /* notify the other window + listeners */
  if(!(T&&T.event&&T.event.emit)&&!opts.fromRemote){
    /* harness (no Tauri) — DOM event only */
  }else if(T&&T.event&&T.event.emit&&!_applyingRemote&&!opts.fromRemote){
    /* never re-broadcast while applying a remote/boot store: that echoed
       between main + widget forever (startup freeze) */
    try{T.event.emit("halftone:settings",{keys:[key],value,store:Store.v,from:WIN_ID}).catch(()=>{})}catch(_){}
  }
  document.dispatchEvent(new CustomEvent("halftone:settings",{detail:{keys:[key],values:{[key]:value},store:Store.v,fromRemote:!!opts.fromRemote}}));
  refreshRow(key);
}
window.htSet=set;

/* incoming from the other window (or our own bridged emit — guard loops) */
let _applyingRemote=false;
const WIN_ID=Math.random().toString(36).slice(2);
if(T&&T.event&&T.event.listen){
  try{T.event.listen("halftone:settings",e=>{
    if(_applyingRemote)return;
    const p=e.payload||{};
    if(p.from===WIN_ID)return; /* our own emit echoed back */
    if(p.store){
      _applyingRemote=true;
      try{
        Store.v=Object.assign(defaults(),p.store);
        applyAll();
        document.dispatchEvent(new CustomEvent("halftone:settings",{detail:{keys:p.keys||[],values:{},store:Store.v,fromRemote:true}}));
        document.dispatchEvent(new CustomEvent("halftone:settings-remote",{detail:{store:Store.v}}));
      }finally{_applyingRemote=false}
    }
  }).catch(()=>{})}catch(_){}
}

/* convenience getters used across the app */
window.getCfg=function(key){return getIn(key)};
window.setCfg=function(key,v,opts){set(key,v,opts)};

/* ============================================================
   GENERATED SETTINGS UI
   Searchable, grouped, per-row reset. Renders into `root`.
   Row shape per contract: .set-row > .set-info + .set-ctl
   ============================================================ */
function icoReset(){return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/></svg>'}

function ctlFor(s,cur){
  const id="set-"+s.key;
  if(s.type==="toggle")return `<label class="switch"><input type="checkbox" ${cur?"checked":""} data-key="${s.key}"><span class="sw-track"></span></label>`;
  if(s.type==="select"){
    const o=(s.options||[]).map(([v,l])=>`<option value="${v}">${l}</option>`).join("");
    return `<select class="field" id="${id}" data-key="${s.key}">${o}</select>`;
  }
  if(s.type==="seg"){
    const o=(s.options||[]).map(([v,l])=>`<button class="${String(cur)===String(v)?"on":""}" data-k="${s.key}" data-v="${v}">${l}</button>`).join("");
    return `<span class="seg" data-key="${s.key}">${o}</span>`;
  }
  if(s.type==="range")return `<input type="range" class="slider" data-key="${s.key}" min="${s.min}" max="${s.max}" step="${s.step}" value="${cur}" style="--val:${pct(s,cur)}%"><span class="range-val">${fmtVal(s,cur)}</span>`;
  if(s.type==="color")return `<input type="color" class="set-color" data-key="${s.key}" value="${cur}">`;
  if(s.type==="folder")return `<input class="field" data-key="${s.key}" value="${(cur||"").replace(/"/g,"&quot;")}" spellcheck="false" placeholder="C:\\Music"><button class="btn" data-browse="${s.key}">BROWSE</button>`;
  if(s.type==="device")return `<select class="field" data-key="${s.key}" data-device="1"><option value="">SYSTEM DEFAULT</option></select>`;
  if(s.type==="action")return `<button class="btn" data-action="${s.key}">${(s.label||"RUN").toUpperCase()}</button>`;
  return "";
}
function fillPct(s,v){return (((+v)-(+s.min))/(+s.max-+s.min)*100).toFixed(1)+"%"}
const pct=fillPct;
function fmtVal(s,v){
  if(s.key==="glassBlur")return (+v)+"px";
  if(s.key==="radiusScale"||s.key==="fontScale")return "×"+(+v).toFixed(2);
  if(s.key==="widgetScale")return (+v)+"%";
  if(s.key==="widgetBgOpacity")return (+v)+"%";
  if(s.key==="lyricsOffset")return ((+v)>0?"+":"")+(+v)+"ms";
  if(s.key==="lyricsFontSize")return (+v)+"px";
  return String(v);
}

function render(root,filter){
  if(!Store.v)return;
  root.innerHTML="";
  const wrap=document.createElement("div");wrap.className="set-wrap";
  /* search box (generated view is searchable; filters label+group+key) */
  const sbox=document.createElement("input");
  sbox.type="search";sbox.className="field set-search";
  sbox.placeholder="Search settings\u2026";sbox.spellcheck=false;
  sbox.setAttribute("aria-label","Search settings");
  sbox.value=filter||"";
  sbox.oninput=()=>{const v=sbox.value;render(root,v);const n=root.querySelector(".set-search");if(n){n.focus();n.setSelectionRange(n.value.length,n.value.length)}};
  wrap.appendChild(sbox);
  const q=(filter||"").trim().toLowerCase();
  const groups=[];
  for(const s of SCHEMA){
    if(s.surface!=="both"&&s.surface!==Store.surface)continue;
    if(q&&!(s.label+" "+s.group+" "+s.key).toLowerCase().includes(q))continue;
    let g=groups.find(x=>x.name===s.group);
    if(!g){g={name:s.group,rows:[]};groups.push(g)}
    g.rows.push(s);
  }
  for(const g of groups){
    const sec=document.createElement("div");sec.className="set-group";
    sec.innerHTML=`<div class="h-caps">${g.name.toUpperCase()}</div>`;
    for(const s of g.rows){
      const cur=s.key==="theme"?ThemeCur():s.key==="palette"?PaletteCur():getIn(s.key);
      const row=document.createElement("div");row.className="set-row";row.dataset.key=s.key;
      row.innerHTML=`<div class="set-info"><span class="set-label">${s.label}</span>${s.hint?`<span class="set-hint">${s.hint}</span>`:""}</div>
        <div class="set-ctl">${ctlFor(s,cur)}<button class="set-reset" title="Reset to default" aria-label="Reset ${s.label}">${icoReset()}</button></div>`;
      wireRow(row,s);
      sec.appendChild(row);
    }
    wrap.appendChild(sec);
  }
  root.appendChild(wrap);
}

/* theme value lives in theme.js state, not the store copy */
function ThemeCur(){return (window.Theme&&window.Theme.name)||"analogue"}
function PaletteCur(){return (window.Theme&&window.Theme.palette)||"classic"}

function refreshRow(key){
  const s=SCHEMA.find(x=>x.key===key);if(!s)return;
  const row=document.querySelector(`.set-row[data-key="${key}"]`);if(!row)return;
  const cur=s.key==="theme"?ThemeCur():s.key==="palette"?PaletteCur():getIn(key);
  row.querySelector(".set-ctl").innerHTML=ctlFor(s,cur)+"<button class=\"set-reset\" title=\"Reset to default\">"+icoReset()+"</button>";
  wireRow(row,s);
}

function wireRow(row,s){
  const cur=s.key==="theme"?ThemeCur():s.key==="palette"?PaletteCur():getIn(s.key);
  if(s.type==="toggle"){
    const inp=row.querySelector("input");
    inp.checked=!!cur;
    inp.onchange=()=>set(s.key,inp.checked);
  }else if(s.type==="select"||s.type==="device"){
    const sel=row.querySelector("select");
    sel.value=String(cur);
    if(s.type==="device")window.htDeviceOptions=window.htDeviceOptions||[];
    sel.onchange=()=>{
      if(s.type==="device"&&window.htSinkPick)window.htSinkPick(sel.value);
      else set(s.key,sel.value);
    };
  }else if(s.type==="seg"){
    row.querySelectorAll(".seg button").forEach(b=>{b.onclick=()=>set(s.key,b.dataset.v)});
  }else if(s.type==="range"){
    const inp=row.querySelector("input"),val=row.querySelector(".range-val");
    inp.oninput=()=>{
      val.textContent=fmtVal(s,inp.value);
      inp.style.setProperty("--val",pct(s,inp.value));
      set(s.key,+inp.value);
    };
  }else if(s.type==="color"){
    const inp=row.querySelector("input");
    inp.oninput=()=>set(s.key,inp.value);
  }else if(s.type==="folder"){
    const inp=row.querySelector("input");
    inp.onchange=()=>set(s.key,inp.value.trim());
    row.querySelector("[data-browse]").onclick=async()=>{
      try{const dir=await window.__invoke("pick_folder");if(dir){inp.value=dir;set(s.key,dir)}}catch(e){console.warn(e)}
    };
  }else if(s.type==="action"){
    row.querySelector("[data-action]").onclick=()=>{try{s.apply(s.default)}catch(e){console.warn(e)}};
  }
  const rst=row.querySelector(".set-reset");
  if(rst)rst.onclick=()=>set(s.key,s.default,{force:true});
}

/* ---------- public surface ---------- */
window.htSettings={
  schema:SCHEMA,
  get:(k)=>getIn(k),
  set:set,
  all:()=>Store.v?JSON.parse(JSON.stringify(Store.v)):null,
  render,
  ready:boot
};
/* self-boot: common.js parses BEFORE this file, so it can't call ready()
   itself. The ready promise resolves after settings_load + applyAll; the
   halftone:settings-ready event fires after all page scripts are parsed
   (boot awaits the backend), so page listeners catch it reliably. */
window.__htSettingsReady=boot();
})();
