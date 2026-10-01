/* TDD harness for ui/smoothing.js — run via browser (needs IS_OWNER + S globals).
   Contract under test (Phase 2):
   1. viewerSmoothTick advances position at rate 1.0 between broadcasts
   2. every broadcast (viewerReconcile) OVERWRITES the prediction
   3. disagreement beyond SNAP_MS (250ms wall-clock) snaps instantly
   4. IS_OWNER pages never interpolate (returns element truth S._pos untouched)
   5. paused state does not advance
*/
const results=[];
function assert(name,cond,detail){
  results.push({name,pass:!!cond,detail:detail||""});
}
/* --- minimal globals smoothing.js expects (viewer context) --- */
IS_OWNER=false;S={_pos:0,playing:true,_smoothLast:0};

/* 1. advances at rate 1.0 */
S._pos=10;S._smoothLast=Date.now()-500;
const p1=viewerSmoothTick();
assert("advances_500ms",Math.abs(p1-10.5)<0.02,"p1="+p1);

/* 2. reconcile overwrites prediction (no snap for small err) */
viewerSmoothTick();                       /* establishes _smoothLast=now */
const authoritative=99.5;
viewerReconcile(authoritative);
assert("reconcile_overwrites",S._pos===authoritative,"pos="+S._pos);

/* 3. snap event on big wall-clock disagreement */
let snapped=false;
document.addEventListener("halftone:snapped",()=>{snapped=true},{once:true});
S._pos=0;S._smoothLast=Date.now()-5000;   /* 5s of predicted advance vs tiny t */
viewerReconcile(0.01);
assert("snap_on_large_err",snapped,"snapped="+snapped);

/* 4. owner pages never interpolate */
IS_OWNER=true;S._pos=42;
assert("owner_passthrough",viewerSmoothTick()===42,"owner got "+viewerSmoothTick());
IS_OWNER=false;

/* 5. paused does not advance */
S.playing=false;S._pos=7;S._smoothLast=Date.now()-400;
const p5=viewerSmoothTick();
assert("paused_no_advance",Math.abs(p5-7)<0.001,"p5="+p5);

/* render results */
document.title="DONE";
const pre=document.createElement("pre");
pre.id="results";
pre.textContent=JSON.stringify(results);
document.body.appendChild(pre);
