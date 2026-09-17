/* ============================================================
   PHASE 2 — viewer position smoothing (UI-thread only)
   ============================================================
   The OWNER's halftone:time broadcast (~30fps) stays the single
   source of truth. Between ticks the viewer advances its own
   view of position by WALL-CLOCK elapsed time at rate=1.0 (the
   audio element cannot be time-stretched), so the seek bar and
   lyric follow move smoothly instead of stepping ~30x/sec.
   Every broadcast OVERWRITES the prediction ("predict, then
   discard"); a wall-clock-scaled error beyond SNAP_MS (i.e. a
   throttle hiccup or missed ticks) snaps instantly instead of
   easing. This is smoothing of one source of truth, NOT
   position estimation: no audio clocks, no drift math, and it
   only exists on viewers (IS_OWNER renders element truth).
   ============================================================ */
const SNAP_MS = 250;        /* wall-clock-scaled disagreement that snaps */
const SNAPPING = 0;         /* set >0 only in tests */
function viewerSmoothTick(){
  if(IS_OWNER)return S._pos||0;
  const now=Date.now();
  if(S._smoothLast&&S.playing){
    const dt=now-S._smoothLast;
    if(dt>0&&dt<1000)S._pos=(S._pos||0)+dt/1000;   /* rate 1.0 */
  }
  S._smoothLast=now;
  return S._pos||0;
}
/* called from applySync when a broadcast lands: reconcile */
function viewerReconcile(t){
  if(IS_OWNER||t==null)return;
  const now=Date.now();
  const predicted=(S._pos||0)+((S._smoothLast&&(S.playing))?(now-S._smoothLast)/1000:0);
  const err=Math.abs(t-predicted)*1000;   /* ms of wall-clock disagreement */
  S._pos=t;
  S._smoothLast=now;
  if(err>SNAP_MS){document.dispatchEvent(new CustomEvent("halftone:snapped"))}
  document.dispatchEvent(new CustomEvent("halftone:tick"));
}
window.viewerSmoothTick=viewerSmoothTick;window.viewerReconcile=viewerReconcile;
