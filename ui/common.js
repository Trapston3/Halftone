/* ============================================================
   HALFTONE shared engine — accent, dither art, meter + sweep,
   collections, transport, layout config. Both windows use it.
   ============================================================ */

/* ============ Tauri bridge ============ */
const T=window.__TAURI__;
const invoke=T?T.core.invoke:null;
const curWin=T?T.window.getCurrentWindow():null;

/* ============ persistent state ============ */
function loadStore(){
  try{return JSON.parse(localStorage.getItem("halftone.store")||"{}")}catch(_){return {}}
}
function saveStore(){
  const s={root:S.root,liked:[...S.liked],playlists:S.playlists.map(p=>({name:p.name,paths:[...p.paths]})),
           vol:S.vol,lastTrack:S.i,lyricsOpen:S.lyricsOpen,pinned:S.pinned,
           shuffle:S.shuffle,repeat:S.repeat,cfg:S.cfg};
  localStorage.setItem("halftone.store",JSON.stringify(s));
}
const S={lib:[],i:-1,playing:false,vol:.8,pinned:false,drag:null,lidx:-1,
         lyricsOpen:false,lyrics:[],meta:null,root:null,
         liked:new Set(),playlists:[],view:"tracks",
         shuffle:false,repeat:"off",
         cfg:{cover:"small",grid:32,queueSide:true,tech:true},
         _managed:true,_prog:false,_img:null};
window.S=S;

(()=>{const st=loadStore();
  if(st.vol!=null)S.vol=st.vol;
  if(st.liked)S.liked=new Set(st.liked);
  if(st.playlists)S.playlists=st.playlists.map(p=>({name:p.name,paths:new Set(p.paths)}));
  if(st.lyricsOpen)S.lyricsOpen=true;
  if(st.pinned)S.pinned=true;
  if(st.root)S.root=st.root;
  if(st.lastTrack!=null)S._lastTrack=st.lastTrack;
  if(st.shuffle)S.shuffle=true;
  if(st.repeat)S.repeat=st.repeat;
  if(st.cfg)S.cfg=Object.assign(S.cfg,st.cfg);
})();

const trk=()=>S.lib[S.i];
const fmt=s=>{s=Math.max(0,Math.round(s));return String(Math.floor(s/60)).padStart(2,"0")+":"+String(s%60).padStart(2,"0")};

/* ============ accent system ============ */
function rgb2hsl(r,g,b){const mx=Math.max(r,g,b),mn=Math.min(r,g,b);let h=0,s=0;const l=(mx+mn)/2;
  if(mx!==mn){const d=mx-mn;s=l>.5?d/(2-mx-mn):d/(mx+mn);
    switch(mx){case r:h=(g-b)/d+(g<b?6:0);break;case g:h=(b-r)/d+2;break;default:h=(r-g)/d+4}h/=6}
  return {h,s,l}}
function hsl2rgb(h,s,l){const f=n=>{const k=(n+h*12)%12;const a=s*Math.min(l,1-l);return l-a*Math.max(-1,Math.min(k-3,9-k,1))};return [f(0)*255,f(8)*255,f(4)*255]}
const lin=c=>{c/=255;return c<=.03928?c/12.92:Math.pow((c+.055)/1.055,2.4)};
const lum=(r,g,b)=>.2126*lin(r)+.7152*lin(g)+.0722*lin(b);
const ratio=(l1,l2)=>(Math.max(l1,l2)+.05)/(Math.min(l1,l2)+.05);
const BG_LUM=lum(0x16,0x18,0x1A);
function extractAccent(img){const c=document.createElement("canvas");c.width=c.height=24;
  const g=c.getContext("2d",{willReadFrequently:true});g.drawImage(img,0,0,24,24);
  const d=g.getImageData(0,0,24,24).data;
  const W=new Array(12).fill(0),Hh=new Array(12).fill(0),Ss=new Array(12).fill(0),Ll=new Array(12).fill(0);
  for(let i=0;i<d.length;i+=4){const {h,s,l}=rgb2hsl(d[i]/255,d[i+1]/255,d[i+2]/255);
    if(s>.25&&l>.12&&l<.92){const w=s*Math.max(l,.15),b=Math.min(11,Math.floor(h*12));
      W[b]+=w;Hh[b]+=h*w;Ss[b]+=s*w;Ll[b]+=l*w}}
  let best=-1;for(let i=0;i<12;i++)if(W[i]>0&&(best<0||W[i]>W[best]))best=i;
  if(best<0)return {h:.44,s:.62,l:.56};
  return {h:Hh[best]/W[best],s:Ss[best]/W[best],l:Ll[best]/W[best]}}
function clampAccent(a){const h=a.h,s=Math.min(.9,Math.max(.5,a.s));let l=Math.min(.68,Math.max(.45,a.l));
  while(l<.82){const [r,g,b]=hsl2rgb(h,s,l);if(ratio(lum(r,g,b),BG_LUM)>=3)break;l+=.02}
  return {h,s,l}}
const ACC={cur:{h:.44,s:.62,l:.56},from:null,to:null,t0:0,dur:420,anim:false,hex:[102,224,194]};
const easeIO=k=>k<.5?4*k*k*k:1-Math.pow(-2*k+2,3)/2;
const lerpHue=(a,b,k)=>{let d=b-a;if(d>.5)d-=1;if(d<-.5)d+=1;return (a+d*k+1)%1};
function applyAccent(){const [r,g,b]=hsl2rgb(ACC.cur.h,ACC.cur.s,ACC.cur.l).map(v=>Math.round(v));
  ACC.hex=[r,g,b];const rs=document.documentElement.style;
  rs.setProperty("--accent",`rgb(${r},${g},${b})`);
  rs.setProperty("--accent-22",`rgba(${r},${g},${b},.22)`);
  rs.setProperty("--accent-12",`rgba(${r},${g},${b},.12)`)}
function tweenAccent(to){ACC.from={...ACC.cur};ACC.to=to;ACC.t0=performance.now();ACC.anim=true}
function tickAccent(now){if(!ACC.anim)return false;
  const k=easeIO(Math.min(1,(now-ACC.t0)/ACC.dur));
  ACC.cur.h=lerpHue(ACC.from.h,ACC.to.h,k);ACC.cur.s=ACC.from.s+(ACC.to.s-ACC.from.s)*k;
  ACC.cur.l=ACC.from.l+(ACC.to.l-ACC.from.l)*k;applyAccent();
  if(k>=1)ACC.anim=false;return true}

/* ============ art: Bayer dither LED grid ============
   cfg.grid = cells per 96px of canvas (32 standard / 48 fine /
   64 ultra) so the CELL SIZE is the design constant. gridFor()
   maps that cell size onto any canvas — widget art, album cards,
   hero art, now-playing thumbnail all share one halftone scale. */
const BAYER=[[0,32,8,40,2,34,10,42],[48,16,56,24,50,18,58,26],[12,44,4,36,14,46,6,38],
  [60,28,52,20,62,30,54,22],[3,35,11,43,1,33,9,41],[51,19,59,27,49,17,57,25],
  [15,47,7,39,13,45,5,37],[63,31,55,23,61,29,53,21]].map(r=>r.map(v=>v/64));
function gridFor(size){const cellPx=96/(S.cfg.grid||32);return Math.max(10,Math.round(size/cellPx))}
function drawDither(cv,img,grid){
  const dpr=Math.min(2,devicePixelRatio||1);
  const W=cv.clientWidth||cv.width/dpr,H=cv.clientHeight||cv.height/dpr;
  if(!grid)grid=gridFor(Math.min(W,H));
  cv.width=Math.round(W*dpr);cv.height=Math.round(H*dpr);
  const g=cv.getContext("2d");
  g.fillStyle="#12171A";g.fillRect(0,0,cv.width,cv.height);
  if(!img)return;
  const off=document.createElement("canvas");off.width=off.height=grid;
  const og=off.getContext("2d",{willReadFrequently:true});og.drawImage(img,0,0,grid,grid);
  const px=og.getImageData(0,0,grid,grid).data;const cell=cv.width/grid;const [ar,ag,ab]=ACC.hex;
  for(let y=0;y<grid;y++)for(let x=0;x<grid;x++){
    const i=(y*grid+x)*4;
    const lumv=(px[i]*.2126+px[i+1]*.7152+px[i+2]*.0722)/255;
    const v=lumv+(BAYER[y%8][x%8]-.5)*.55;
    if(v>.62)g.fillStyle=`rgb(${ar},${ag},${ab})`;
    else if(v>.38)g.fillStyle=`rgba(${ar},${ag},${ab},.45)`;
    else continue;
    g.fillRect(x*cell,y*cell,cell-.75,cell-.75)}}

/* ============ album-themed edge: perimeter dither band ============
   The widget frame samples the album art's own edges and redraws
   them as an accent Bayer dither hugging the inside of the 1px
   accent border — the frame is the album's silhouette.       */
function edgePixels(img){
  if(S._edgeImg===img&&S._edgePx)return S._edgePx;
  const c=document.createElement("canvas");c.width=c.height=64;
  const g=c.getContext("2d",{willReadFrequently:true});
  if(img)g.drawImage(img,0,0,64,64);
  S._edgeImg=img;S._edgePx=g.getImageData(0,0,64,64).data;
  return S._edgePx;
}
function drawEdge(cv,img){
  const dpr=Math.min(2,devicePixelRatio||1);
  const W=cv.clientWidth,H=cv.clientHeight;if(!W||!H)return;
  const dw=Math.round(W*dpr),dh=Math.round(H*dpr);
  if(cv.width!==dw||cv.height!==dh){cv.width=dw;cv.height=dh}
  const g=cv.getContext("2d");g.clearRect(0,0,dw,dh);
  if(!img)return;                    /* no art -> plain accent border only */
  const px=edgePixels(img),N=64,[ar,ag,ab]=ACC.hex;
  const C=Math.max(3,Math.round(4*dpr)),in1=Math.round(dpr);
  const nx=Math.max(1,Math.floor((dw-2*in1)/C)),ny=Math.max(1,Math.floor((dh-2*in1)/C));
  const lumAt=(ix,iy)=>{const i=(iy*N+ix)*4;return (px[i]*.2126+px[i+1]*.7152+px[i+2]*.0722)/255};
  const cell=(x,y,bx,by,v0)=>{
    const v=v0+(BAYER[by%8][bx%8]-.5)*.5;
    if(v>.58)g.fillStyle=`rgba(${ar},${ag},${ab},.5)`;
    else if(v>.38)g.fillStyle=`rgba(${ar},${ag},${ab},.26)`;
    else return;
    g.fillRect(x,y,C-.75,C-.75)};
  for(let i=0;i<nx;i++){const u=Math.min(63,Math.round(i/nx*(N-1))),x=in1+i*C;
    cell(x,in1,i,0,lumAt(u,0));
    cell(x,dh-in1-C,i,0,lumAt(u,N-1))}
  for(let j=0;j<ny;j++){const v=Math.min(63,Math.round(j/ny*(N-1))),y=in1+j*C;
    cell(in1,y,0,j,lumAt(0,v));
    cell(dw-in1-C,y,0,j,lumAt(N-1,v))}
}

/* ============ analyser ============ */
let AC=null,analyser=null,DATA=null,BINS=null,gainNode=null;
const NBARS=48,EMA=new Float32Array(NBARS);
function ensureAudio(){if(AC)return true;
  try{
    AC=new (window.AudioContext||window.webkitAudioContext)();
    analyser=AC.createAnalyser();analyser.fftSize=2048;analyser.smoothingTimeConstant=.8;
    gainNode=AC.createGain();gainNode.gain.value=S.vol;
    analyser.connect(gainNode);gainNode.connect(AC.destination);
    const ms=AC.createMediaElementSource(el.aud);ms.connect(analyser);
    DATA=new Uint8Array(analyser.frequencyBinCount);
    const fMin=40,fMax=Math.min(15000,AC.sampleRate/2),K=AC.sampleRate/analyser.fftSize;BINS=[];
    for(let i=0;i<NBARS;i++){const f0=fMin*Math.pow(fMax/fMin,i/NBARS),f1=fMin*Math.pow(fMax/fMin,(i+1)/NBARS);
      let b0=Math.max(0,Math.floor(f0/K)),b1=Math.min(DATA.length,Math.max(b0+1,Math.ceil(f1/K)));BINS.push([b0,b1])}
    return true;
  }catch(e){console.error("audio init",e);return false}}
function bars(){const out=new Float32Array(NBARS);
  if(analyser&&S.playing)analyser.getByteFrequencyData(DATA);
  for(let i=0;i<NBARS;i++){let v=0;
    if(analyser&&S.playing){const [b0,b1]=BINS[i];let s=0;for(let b=b0;b<b1;b++)s+=DATA[b];
      v=Math.min(1,(s/(b1-b0))/255*(.55+.75*Math.pow((i+1)/NBARS,.6)))}
    const p=EMA[i];EMA[i]=v>p?p+(v-p)*.5:p+(v-p)*.14;if(!S.playing)EMA[i]*=.9;out[i]=EMA[i]}
  return out}

/* ============ meter with ILLUMINATION SWEEP drag physics ============ */
const SWEEP={t:0,dir:1,v:0};
function sweepTick(){
  if(S.drag==null){SWEEP.v*=.88;if(SWEEP.v<.01)SWEEP.v=0}
  else SWEEP.v+=(1-SWEEP.v)*.35;
}
function drawMeter(cv,H){
  const box=cv.parentElement,W=box.clientWidth;if(!W||!H)return;
  const dpr=Math.min(2,devicePixelRatio||1);
  if(cv.width!==Math.round(W*dpr)||cv.height!==Math.round(H*dpr)){cv.width=Math.round(W*dpr);cv.height=Math.round(H*dpr)}
  const g=cv.getContext("2d");g.setTransform(dpr,0,0,dpr,0,0);g.clearRect(0,0,W,H);
  const spec=bars(),gap=2,cw=Math.max(2,(W-gap*(NBARS-1))/NBARS);
  const rows=Math.max(2,Math.floor((H-2)/4));
  const dur=el.aud.duration||0;
  const prog=dur>0?el.aud.currentTime/dur:0;
  const dragP=S.drag!=null?S.drag.p:prog;
  const [ar,ag,ab]=ACC.hex;
  const durS=dur||1;
  for(let i=0;i<NBARS;i++){
    const segT=(i+.5)/NBARS*durS;
    const played=(i+.5)/NBARS<dragP;
    const lit=Math.round(Math.max(.08,spec[i])*rows);
    let alpha=played?1:.22;
    if(S.drag!=null){
      const dist=(segT-SWEEP.t)/durS*NBARS;
      if(dist>0){const decay=Math.exp(-dist*.14);alpha=.22+.6*decay*SWEEP.v}
      else alpha=1;
    }
    for(let r=0;r<rows;r++){
      const y=H-2-(r+1)*4;
      if(r<lit){
        g.fillStyle=played?`rgba(${ar},${ag},${ab},${alpha})`:`rgba(${ar},${ag},${ab},${.22*alpha})`;
      }else{
        g.fillStyle=played?`rgba(${ar},${ag},${ab},${.08})`:"#232A2E";
      }
      g.fillRect(i*(cw+gap),y,cw,3);
    }
    if(S.drag!=null&&Math.abs(segT-SWEEP.t)<durS/NBARS){
      g.fillStyle=`rgba(255,255,255,${.85*SWEEP.v})`;
      g.fillRect(i*(cw+gap),H-2-rows*4,cw,rows*4-1);
    }
  }
  const px=dragP*W;
  g.fillStyle="#F2E8CF";g.fillRect(px-.75,0,1.5,H);
  g.beginPath();g.moveTo(px-3,0);g.lineTo(px+3,0);g.lineTo(px,4);g.closePath();g.fill();
}

/* ============ lyrics ============ */
function buildLyrics(){el.lyrWrap.innerHTML="";
  S.lyrics.forEach(L=>{const d=document.createElement("div");d.className="line";d.textContent=L.text;d.dataset.t=L.t;
    d.onclick=()=>{el.aud.currentTime=L.t;S.lidx=-1};  /* click a line to seek */
    el.lyrWrap.appendChild(d)});
  S.lidx=-1}
function updateLyrics(){if(!S.lyricsOpen)return;
  const t=el.aud.currentTime;const lines=[...el.lyrWrap.children];
  let idx=-1;for(let i=0;i<lines.length;i++){if(t>=+lines[i].dataset.t)idx=i}
  if(idx===S.lidx)return;S.lidx=idx;
  lines.forEach((l,i)=>l.classList.toggle("active",i===idx));
  if(idx>=0){const ln=lines[idx];el.lyrWrap.style.transform=`translateY(${64-(ln.offsetTop+ln.offsetHeight/2)}px)`}}

/* ============ library ============ */
async function scanLibrary(dir){
  if(el.libstat)el.libstat.textContent="scanning...";
  try{
    const res=await invoke("scan_library",{dir});
    S.lib=res.tracks;S.root=dir;saveStore();
    if(el.libstat)el.libstat.textContent=res.tracks.length+" TRACKS"+(res.skipped.length?" / "+res.skipped.length+" SKIPPED":"");
    return res;
  }catch(e){if(el.libstat)el.libstat.textContent="scan failed: "+e;return null}}

async function loadTrack(i,autoplay=true){
  if(!S.lib.length)return;
  S.i=(i+S.lib.length)%S.lib.length;
  const meta=await invoke("open_track",{path:S.lib[S.i].path});
  S.meta=meta;
  S.lyrics=await invoke("read_lyrics",{path:S.lib[S.i].path});
  const url=await invoke("flac_url",{path:S.lib[S.i].path});
  el.aud.src=url;
  buildLyrics();
  if(meta.cover){
    const img=new Image();
    img.onload=()=>{S._img=img;tweenAccent(clampAccent(extractAccent(img)));};
    img.src="data:"+meta.cover.mime+";base64,"+meta.cover.data_b64;
  }else{S._img=null;tweenAccent({h:.44,s:.62,l:.56})}
  saveStore();
  if(autoplay&&S.playing)el.aud.play().catch(()=>{});
  document.dispatchEvent(new CustomEvent("halftone:track"));
}
async function setPlaying(p){
  if(p&&S.i<0)await loadTrack(0);
  if(p&&!ensureAudio())return;
  S.playing=p;
  const ip=document.getElementById("icoPlay"),ipa=document.getElementById("icoPause");
  if(ip)ip.style.display=p?"none":"block";
  if(ipa)ipa.style.display=p?"block":"none";
  if(p){AC.resume();el.aud.play().catch(e=>console.warn("play",e))}
  else el.aud.pause();
  document.dispatchEvent(new CustomEvent("halftone:state"));
}

/* ============ transport: shuffle / repeat ============ */
async function nextTrack(auto=false){
  if(!S.lib.length)return;
  if(S.repeat==="one"&&auto){el.aud.currentTime=0;el.aud.play();return}
  let i;
  if(S.shuffle){
    if(S.lib.length===1){el.aud.currentTime=0;el.aud.play();return}
    do{i=Math.floor(Math.random()*S.lib.length)}while(i===S.i);
  }else{
    i=S.i+1;
  }
  if(i>=S.lib.length){
    if(S.repeat==="all"||!auto)i=0;
    else{await setPlaying(false);return}
  }
  loadTrack(i);
}
function cycleRepeat(){S.repeat=S.repeat==="off"?"all":S.repeat==="all"?"one":"off";saveStore();
  document.dispatchEvent(new CustomEvent("halftone:state"))}

/* ============ seek pointer wiring (shared) ============ */
function wireSeek(seek){
  seek.addEventListener("pointerenter",()=>seek.classList.add("open"));
  seek.addEventListener("pointerleave",()=>{if(S.drag==null)seek.classList.remove("open")});
  seek.addEventListener("pointerdown",e=>{
    try{seek.setPointerCapture(e.pointerId)}catch(_){}
    const r=seek.getBoundingClientRect();
    const p=Math.min(1,Math.max(0,(e.clientX-r.left)/r.width));
    S.drag={p};SWEEP.t=p*(el.aud.duration||0);SWEEP.v=0;
    seek.classList.add("open","dragging");
    seekTip();
  });
  seek.addEventListener("pointermove",e=>{
    if(S.drag==null)return;
    const r=seek.getBoundingClientRect();
    const p=Math.min(1,Math.max(0,(e.clientX-r.left)/r.width));
    SWEEP.dir=p>S.drag.p?1:-1;
    S.drag.p=p;SWEEP.t=p*(el.aud.duration||0);
    seekTip();
  });
  const end=()=>{
    if(S.drag==null)return;
    el.aud.currentTime=S.drag.p*(el.aud.duration||0);
    S.drag=null;S.lidx=-1;
    seek.classList.remove("dragging");
    if(!seek.matches(":hover"))seek.classList.remove("open");
  };
  seek.addEventListener("pointerup",end);
  seek.addEventListener("pointercancel",end);
  /* wheel-seek QoL: scroll over the meter = +/-5s */
  seek.addEventListener("wheel",e=>{
    e.preventDefault();
    if(el.aud.duration)el.aud.currentTime=Math.max(0,Math.min(el.aud.duration,el.aud.currentTime+(e.deltaY<0?5:-5)));
  },{passive:false});
}
function seekTip(){
  const seek=el.seek;const dur=el.aud.duration||0;
  if(el.tip){el.tip.textContent=fmt(S.drag.p*dur)+" / "+fmt(dur);
    el.tip.style.left=(S.drag.p*seek.clientWidth)+"px"}
}

/* ============ JS window dragging ============
   data-tauri-drag-region proved unreliable inside WebView2; call
   startDragging() explicitly, with a 4px threshold so plain clicks
   and double-clicks (pin toggle) stay clean.                  */
function wireDrag(zone){
  let sx=0,sy=0,armed=false;
  zone.addEventListener("pointerdown",e=>{
    if(e.button!==0||!curWin)return;
    if(e.target.closest("button,input,a,.seek,.leds,.lyr-view,.pop,.tbtn,.cbtn"))return;
    sx=e.clientX;sy=e.clientY;armed=true;
  });
  zone.addEventListener("pointermove",e=>{
    if(!armed)return;
    if(Math.abs(e.clientX-sx)+Math.abs(e.clientY-sy)>4){
      armed=false;
      curWin.startDragging().catch(()=>{});
    }
  });
  const done=()=>armed=false;
  zone.addEventListener("pointerup",done);
  zone.addEventListener("pointercancel",done);
}

/* ============ collections ============ */
function toggleLike(path){S.liked.has(path)?S.liked.delete(path):S.liked.add(path);saveStore();
  document.dispatchEvent(new CustomEvent("halftone:collect"))}
function makePlaylist(name,paths){S.playlists.push({name,paths:new Set(paths)});saveStore();
  document.dispatchEvent(new CustomEvent("halftone:collect"))}

/* ============ context menu (right-click configurator) ============ */
function buildCtxMenu(items){
  const m=document.createElement("div");
  m.className="pop ctxmenu";
  items.forEach(it=>{
    if(it.sep){const s=document.createElement("div");s.className="ctxsep";m.appendChild(s);return}
    const b=document.createElement("button");
    b.className="mitem ctxitem"+(it.checked?" on":"");
    b.innerHTML=`<span class="ctxdot"></span>${it.label}`;
    b.onclick=()=>{closeCtx();it.onClick&&it.onClick()};
    m.appendChild(b);
  });
  return m;
}
function closeCtx(){document.querySelectorAll(".ctxmenu").forEach(m=>m.remove())}
function openCtx(x,y,items){
  closeCtx();
  const m=buildCtxMenu(items);
  document.body.appendChild(m);
  m.classList.add("open");
  const w=m.offsetWidth,h=m.offsetHeight;
  m.style.left=Math.min(x,innerWidth-w-8)+"px";
  m.style.top=Math.min(y,innerHeight-h-8)+"px";
  setTimeout(()=>addEventListener("pointerdown",function h(ev){
    if(!m.contains(ev.target)){closeCtx();removeEventListener("pointerdown",h)}},0),0);
}
addEventListener("keydown",e=>{if(e.key==="Escape")closeCtx()});

/* ============ shared element refs (page fills el) ============ */
var el={};
window.el=el;
