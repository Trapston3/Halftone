/* ============================================================
   Halftone test harness loader (ui/test/loader.js)
   Loaded FIRST in main.html and index.html (before common.js).

   In the Tauri app `window.__TAURI__` exists at parse time, so this
   file is a no-op. In the BROWSER HARNESS the page is opened with
   ?harness=1 (the harness frame src) — we then block script
   execution of the rest of the page with a synchronous prompt
   substitute: document.write of a script tag that wires
   window.__HT_TAURI_MOCK__ from the PARENT harness page.

   Why: the old harness set window properties on the frame window
   before navigation, but navigation replaces the window, so pages
   booted with NO mock (library always empty, settings dead).
   Injecting from the parent at load time is also too late for
   parse-time code. The only reliable no-build-step hook IS this
   loader: it runs first, sees the harness param, and synchronously
   pulls the mock from window.parent (same origin) before the
   engine scripts parse.
   ============================================================ */
(function(){
  try{
    if(window.__TAURI__)return;                    /* real app: nothing to do */
    var q=new URLSearchParams(location.search);
    var isHarness=q.has("harness")||(window.parent&&window.parent!==window&&window.parent.__HT_HARNESS__);
    if(!isHarness)return;                          /* plain file open: no mock */
    var label=(window.IS_VIEWER||document.body&&document.body.dataset.surface==="widget")?"widget":"main";
    /* surface attr is set on <body>; for main.html the <html> has data-surface="main" */
    if(!window.IS_VIEWER&&document.documentElement.dataset.surface==="main")label="main";
    var P=window.parent;
    if(P&&P.__HT_MOCKS__&&P.__HT_MOCKS__[label]){
      var mk=P.__HT_MOCKS__[label];
      Object.defineProperty(window,"__HT_TAURI_MOCK__",{value:mk,configurable:true});
      if(mk.__invoke)Object.defineProperty(window,"__invoke",{value:mk.__invoke,configurable:true});
    }
  }catch(e){/* never break the real app */}
})();
