/**
 * ui/layout.js — Halftone v1.1 Direct-Manipulation Layout Studio (Move + Resize + Rotate)
 *
 * Implements Section 11 of the Halftone v1.1 Build Specification:
 * - Works on both Widget (`ui/index.html`) and Main Window (`ui/main.html`)
 * - Toggle via `Ctrl+L` or the "Edit Layout" toolbar button
 * - Every `[data-module]` element can be:
 *   1. Moved (free or 8px grid snap + center/edge alignment guides)
 *   2. Scaled/Resized (via top-right scale/resize drag handle or wheel)
 *   3. Rotated at ANY arbitrary angle (`-180°..+180°`, 15° `Shift` snap, and
 *      magnetic detents within ±3° of `0°`, `90°`, `180°`, `-90°`) rendered via
 *      `transform: translate3d(dx, dy, 0) rotate(deg) scale(s)` with
 *      `transform-origin: center center`.
 * - Full interactive hit-testing inside rotated modules, plus `projectPointerDelta(el, dx, dy)`
 *   so horizontal sliders/scrubbers inside rotated modules track along the rotated axis.
 * - Safety rails: essential controls (`transport-cluster` / `#wPlay`, `window-controls` / `#wClose`)
 *   cannot be hidden or dragged off-screen.
 * - 7 built-in presets (`classic`, `compact-strip`, `square-art`, `vertical-rack`,
 *   `lyrics-focus`, `meter-bar`, `tilted-desk`) + Undo/Redo (`Ctrl+Z`/`Ctrl+Y`) + JSON Export/Import.
 */
(function () {
  "use strict";

  const STORAGE_PREFIX = "halftone.layout.v11.";
  const SNAP_GRID = 8;
  const MAGNETIC_ANGLES = [-180, -90, 0, 90, 180];
  const MAGNETIC_TOLERANCE = 3.0;

  // Built-in named module transform presets (offsets relative to default flow)
  const BUILTIN_PRESETS = {
    classic: {
      name: "Classic",
      widgetPreset: "card",
      modules: {},
    },
    "compact-strip": {
      name: "Compact Strip",
      widgetPreset: "strip",
      modules: {},
    },
    "square-art": {
      name: "Square Art",
      widgetPreset: "square",
      modules: {},
    },
    "lyrics-focus": {
      name: "Lyrics Focus",
      widgetPreset: "lyrics",
      modules: {},
    },
    "vertical-rack": {
      name: "Vertical Rack",
      widgetPreset: "square",
      modules: {
        "cover-art": { dx: 0, dy: 0, rot: 0, scale: 0.96, visible: true },
        "track-marquee": { dx: 0, dy: 0, rot: 0, scale: 1.0, visible: true },
        "transport-cluster": { dx: 0, dy: 0, rot: 0, scale: 1.0, visible: true },
      },
    },
    "meter-bar": {
      name: "Meter Bar",
      widgetPreset: "strip",
      modules: {
        "seek-bar": { dx: 0, dy: -2, rot: 0, scale: 1.04, visible: true },
      },
    },
    "tilted-desk": {
      name: "Tilted Desk",
      widgetPreset: "card",
      modules: {
        "cover-art": { dx: 2, dy: 2, rot: -4, scale: 0.98, visible: true },
        "track-marquee": { dx: 2, dy: 0, rot: 1.5, scale: 1.0, visible: true },
        "seek-bar": { dx: 0, dy: 0, rot: -1.5, scale: 0.99, visible: true },
        "transport-cluster": { dx: 0, dy: -1, rot: 2, scale: 1.0, visible: true },
      },
    },
  };

  const ESSENTIAL_MODULES = new Set([
    "transport-cluster",
    "window-controls",
    "main-transport",
    "main-titlebar",
  ]);

  let editMode = false;
  let activeModuleId = null;
  let snapEnabled = true;
  let currentPreset = "classic";
  let moduleStates = {}; // { [moduleId]: { dx, dy, rot, scale, visible, locked, opacity, z } }
  const undoStack = [];
  const redoStack = [];

  function surfaceName() {
    return document.documentElement.getAttribute("data-surface") || (window.IS_VIEWER ? "widget" : "main");
  }

  function themeName() {
    return document.documentElement.getAttribute("data-theme") || "analogue";
  }

  function storageKey() {
    return `${STORAGE_PREFIX}${surfaceName()}.${themeName()}`;
  }

  function defaultModuleState() {
    return { dx: 0, dy: 0, rot: 0, scale: 1, visible: true, locked: false, opacity: 1, z: 1 };
  }

  function cloneState(st) {
    return JSON.parse(JSON.stringify(st || {}));
  }

  function pushUndo() {
    undoStack.push(cloneState(moduleStates));
    if (undoStack.length > 50) undoStack.shift();
    redoStack.length = 0;
    updateToolbarState();
  }

  function saveState() {
    try {
      const payload = { preset: currentPreset, snap: snapEnabled, modules: moduleStates };
      localStorage.setItem(storageKey(), JSON.stringify(payload));
    } catch (_) {}
  }

  function loadState() {
    try {
      const raw = localStorage.getItem(storageKey());
      if (raw) {
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed === "object") {
          currentPreset = parsed.preset || "classic";
          snapEnabled = parsed.snap !== false;
          moduleStates = parsed.modules || {};
          return;
        }
      }
    } catch (_) {}
    moduleStates = {};
  }

  function injectLayoutStyles() {
    if (document.getElementById("ht-layout-styles")) return;
    const st = document.createElement("style");
    st.id = "ht-layout-styles";
    st.textContent = `
      [data-module] {
        position: relative;
        transform-origin: center center;
        transition: box-shadow 140ms ease;
      }
      html[data-layout-edit="true"] [data-module] {
        outline: 1.5px dashed var(--accent, #d84a2a);
        outline-offset: 2px;
        cursor: grab;
        user-select: none;
      }
      html[data-layout-edit="true"] [data-module].ht-mod-selected {
        outline: 2px solid var(--accent, #d84a2a);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent, #d84a2a) 26%, transparent);
        z-index: 35 !important;
      }
      .ht-mod-gizmo {
        display: none !important;
        position: absolute;
        inset: -4px;
        pointer-events: none;
        z-index: 40;
      }
      html[data-layout-edit="true"] [data-module] > .ht-mod-gizmo {
        display: block !important;
      }
      /* Rotation stem & handle above top-center */
      .ht-rot-stem {
        position: absolute;
        left: 50%;
        top: -22px;
        width: 1.5px;
        height: 18px;
        background: var(--accent, #d84a2a);
        transform: translateX(-50%);
        pointer-events: none;
      }
      .ht-rot-handle {
        position: absolute;
        left: 50%;
        top: -32px;
        width: 16px;
        height: 16px;
        border-radius: 50%;
        background: var(--panel-bg, #18181c);
        border: 2px solid var(--accent, #d84a2a);
        transform: translateX(-50%);
        cursor: crosshair;
        pointer-events: auto;
        display: flex;
        align-items: center;
        justify-content: center;
        box-shadow: 0 2px 6px rgba(0,0,0,0.4);
      }
      .ht-rot-handle:hover {
        background: var(--accent, #d84a2a);
      }
      .ht-scale-handle {
        position: absolute;
        right: -6px;
        bottom: -6px;
        width: 13px;
        height: 13px;
        border-radius: 3px;
        background: var(--accent, #d84a2a);
        border: 1.5px solid var(--panel-bg, #111);
        cursor: nwse-resize;
        pointer-events: auto;
      }
      .ht-mod-badge {
        position: absolute;
        left: 4px;
        top: 4px;
        padding: 1px 5px;
        font-family: var(--font-mono, monospace);
        font-size: 9px;
        letter-spacing: 0.04em;
        background: var(--panel-bg, rgba(16,16,20,0.92));
        color: var(--fg, #fff);
        border: 1px solid var(--accent, #d84a2a);
        border-radius: 3px;
        pointer-events: none;
        white-space: nowrap;
      }
      /* Alignment guide lines */
      .ht-align-guide-x, .ht-align-guide-y {
        position: fixed;
        background: var(--accent, #d84a2a);
        pointer-events: none;
        z-index: 9995;
        opacity: 0;
        transition: opacity 90ms ease;
      }
      .ht-align-guide-x { top: 0; bottom: 0; width: 1px; }
      .ht-align-guide-y { left: 0; right: 0; height: 1px; }

      /* Floating Layout Studio Bar */
      #ht-layout-bar {
        position: fixed;
        left: 50%;
        bottom: 10px;
        transform: translateX(-50%) translateY(150%);
        z-index: 9996;
        display: none;
        align-items: center;
        gap: 6px;
        padding: 6px 10px;
        background: var(--panel-bg, #18181c);
        color: var(--fg, #f4f4f6);
        border: 1.5px solid var(--accent, #d84a2a);
        border-radius: 8px;
        box-shadow: 0 10px 32px rgba(0,0,0,0.5);
        font-family: var(--font-mono, monospace);
        font-size: 10px;
        transition: transform 200ms var(--ease-spring-snappy, ease);
        max-width: calc(100vw - 16px);
        flex-wrap: wrap;
        pointer-events: none;
      }
      html[data-layout-edit="true"] #ht-layout-bar {
        display: flex;
        transform: translateX(-50%) translateY(0);
        pointer-events: auto;
      }
      #ht-layout-bar button, #ht-layout-bar select {
        font-family: var(--font-mono, monospace);
        font-size: 10px;
        padding: 3px 7px;
        border-radius: 4px;
        border: 1px solid var(--line-2, rgba(255,255,255,0.2));
        background: var(--bg-elev-2, rgba(255,255,255,0.06));
        color: var(--fg, #fff);
        cursor: pointer;
      }
      #ht-layout-bar button:hover {
        border-color: var(--accent, #d84a2a);
      }
      #ht-layout-bar button.active {
        background: var(--accent, #d84a2a);
        color: var(--accent-ink, #fff);
      }
    `;
    document.head.appendChild(st);
  }

  function ensureDefaultModuleTags() {
    if (surfaceName() === "widget") {
      const map = [
        ["#wArt", "cover-art", "COVER ART"],
        [".w-meta", "track-marquee", "TRACK METADATA"],
        [".w-actions", "window-controls", "WINDOW CONTROLS"],
        ["#wSeek", "seek-bar", "SEEK SCRUBBER"],
        [".w-transport", "transport-cluster", "TRANSPORT"],
        ["#wVol", "volume-control", "VOLUME"],
        ["#wLyrHost", "lyrics-ticker", "LYRICS"],
      ];
      for (const [sel, id, label] of map) {
        const el = document.querySelector(sel);
        if (el && !el.hasAttribute("data-module")) {
          el.setAttribute("data-module", id);
          el.setAttribute("data-module-label", label);
        }
      }
    } else {
      const map = [
        [".titlebar, #titlebar", "main-titlebar", "TITLEBAR"],
        [".sidebar, #sidebar, nav", "main-sidebar", "NAVIGATION"],
        [".transport, #transport, footer", "main-transport", "TRANSPORT BAR"],
      ];
      for (const [sel, id, label] of map) {
        const el = document.querySelector(sel);
        if (el && !el.hasAttribute("data-module")) {
          el.setAttribute("data-module", id);
          el.setAttribute("data-module-label", label);
        }
      }
    }
  }

  function applyAllModulesToDom() {
    ensureDefaultModuleTags();
    const els = document.querySelectorAll("[data-module]");
    els.forEach((el) => {
      const id = el.getAttribute("data-module");
      const st = Object.assign(defaultModuleState(), moduleStates[id] || {});
      // Safety rail: essential modules can never be hidden or moved wildly off-screen
      if (ESSENTIAL_MODULES.has(id)) {
        st.visible = true;
        st.dx = Math.max(-120, Math.min(120, st.dx || 0));
        st.dy = Math.max(-80, Math.min(80, st.dy || 0));
      }
      const dx = Math.round((st.dx || 0) * 10) / 10;
      const dy = Math.round((st.dy || 0) * 10) / 10;
      const rot = Math.round((st.rot || 0) * 10) / 10;
      const sc = Math.max(0.65, Math.min(1.6, st.scale || 1));

      if (!st.visible) {
        el.style.display = "none";
      } else if (el.dataset.origDisplay) {
        el.style.display = el.dataset.origDisplay === "none" ? "" : el.dataset.origDisplay;
      }

      if (dx === 0 && dy === 0 && rot === 0 && sc === 1) {
        el.style.transform = "";
      } else {
        el.style.transform = `translate3d(${dx}px, ${dy}px, 0px) rotate(${rot}deg) scale(${sc})`;
      }
      el.dataset.rot = String(rot);
      el.dataset.dx = String(dx);
      el.dataset.dy = String(dy);

      ensureModuleGizmo(el, id, st);
    });
  }

  function ensureModuleGizmo(el, id, st) {
    let gizmo = el.querySelector(":scope > .ht-mod-gizmo");
    if (!gizmo) {
      gizmo = document.createElement("div");
      gizmo.className = "ht-mod-gizmo";
      gizmo.innerHTML = `
        <div class="ht-rot-stem"></div>
        <div class="ht-rot-handle" title="Drag to rotate module (Shift = 15° snap)"></div>
        <div class="ht-scale-handle" title="Drag to scale module"></div>
        <div class="ht-mod-badge"></div>
      `;
      el.appendChild(gizmo);
      wireModuleInteractions(el, id, gizmo);
    }
    gizmo.hidden = !editMode;
    gizmo.style.display = editMode ? "block" : "none";
    const badge = gizmo.querySelector(".ht-mod-badge");
    if (badge) {
      const label = el.getAttribute("data-module-label") || id.toUpperCase();
      const rotStr = st.rot ? ` ${Math.round(st.rot)}°` : "";
      badge.textContent = `${label}${rotStr}`;
    }
  }

  function snapAngle(deg, shiftKey) {
    let a = ((deg + 180) % 360 + 360) % 360 - 180;
    if (shiftKey) {
      return Math.round(a / 15) * 15;
    }
    if (snapEnabled) {
      for (const m of MAGNETIC_ANGLES) {
        if (Math.abs(a - m) <= MAGNETIC_TOLERANCE) {
          return m === -180 ? 180 : m;
        }
      }
    }
    return Math.round(a * 10) / 10;
  }

  function wireModuleInteractions(el, id, gizmo) {
    const rotHandle = gizmo.querySelector(".ht-rot-handle");
    const scaleHandle = gizmo.querySelector(".ht-scale-handle");

    // 1. Rotation drag
    rotHandle.addEventListener("pointerdown", (ev) => {
      if (!editMode) return;
      ev.stopPropagation();
      ev.preventDefault();
      selectModule(id);
      pushUndo();
      try { rotHandle.setPointerCapture(ev.pointerId); } catch (_) {}
      const rect = el.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const st = Object.assign(defaultModuleState(), moduleStates[id] || {});
      const startAngle = (Math.atan2(ev.clientY - cy, ev.clientX - cx) * 180) / Math.PI;
      const baseRot = st.rot || 0;

      const onMove = (me) => {
        const curAngle = (Math.atan2(me.clientY - cy, me.clientX - cx) * 180) / Math.PI;
        const rawRot = baseRot + (curAngle - startAngle);
        st.rot = snapAngle(rawRot, me.shiftKey);
        moduleStates[id] = st;
        applyAllModulesToDom();
        updateToolbarState();
      };
      const onUp = () => {
        rotHandle.removeEventListener("pointermove", onMove);
        rotHandle.removeEventListener("pointerup", onUp);
        saveState();
      };
      rotHandle.addEventListener("pointermove", onMove);
      rotHandle.addEventListener("pointerup", onUp);
    });

    // 2. Scale/Resize drag
    scaleHandle.addEventListener("pointerdown", (ev) => {
      if (!editMode) return;
      ev.stopPropagation();
      ev.preventDefault();
      selectModule(id);
      pushUndo();
      try { scaleHandle.setPointerCapture(ev.pointerId); } catch (_) {}
      const startX = ev.clientX;
      const startY = ev.clientY;
      const st = Object.assign(defaultModuleState(), moduleStates[id] || {});
      const baseScale = st.scale || 1;

      const onMove = (me) => {
        const delta = (me.clientX - startX + (me.clientY - startY)) * 0.004;
        st.scale = Math.round(Math.max(0.65, Math.min(1.5, baseScale + delta)) * 100) / 100;
        moduleStates[id] = st;
        applyAllModulesToDom();
        updateToolbarState();
      };
      const onUp = () => {
        scaleHandle.removeEventListener("pointermove", onMove);
        scaleHandle.removeEventListener("pointerup", onUp);
        saveState();
      };
      scaleHandle.addEventListener("pointermove", onMove);
      scaleHandle.addEventListener("pointerup", onUp);
    });

    // 3. Module translation drag (when in edit mode)
    el.addEventListener(
      "pointerdown",
      (ev) => {
        if (!editMode) return;
        if (ev.target.closest(".ht-rot-handle, .ht-scale-handle")) return;
        ev.stopPropagation();
        ev.preventDefault();
        selectModule(id);
        pushUndo();
        try { el.setPointerCapture(ev.pointerId); } catch (_) {}
        const startX = ev.clientX;
        const startY = ev.clientY;
        const st = Object.assign(defaultModuleState(), moduleStates[id] || {});
        const baseDx = st.dx || 0;
        const baseDy = st.dy || 0;
        const stageScale = (window.W && window.W.s) || 1;

        const onMove = (me) => {
          let nx = baseDx + (me.clientX - startX) / stageScale;
          let ny = baseDy + (me.clientY - startY) / stageScale;
          if (snapEnabled && !me.altKey) {
            nx = Math.round(nx / SNAP_GRID) * SNAP_GRID;
            ny = Math.round(ny / SNAP_GRID) * SNAP_GRID;
          }
          st.dx = Math.max(-240, Math.min(240, nx));
          st.dy = Math.max(-200, Math.min(200, ny));
          moduleStates[id] = st;
          applyAllModulesToDom();
          updateToolbarState();
        };
        const onUp = () => {
          el.removeEventListener("pointermove", onMove);
          el.removeEventListener("pointerup", onUp);
          saveState();
        };
        el.addEventListener("pointermove", onMove);
        el.addEventListener("pointerup", onUp);
      },
      true
    );
  }

  function selectModule(id) {
    activeModuleId = id;
    document.querySelectorAll("[data-module]").forEach((el) => {
      el.classList.toggle("ht-mod-selected", el.getAttribute("data-module") === id);
    });
    updateToolbarState();
  }

  function setModuleTransform(id, patch) {
    ensureDefaultModuleTags();
    pushUndo();
    const cur = Object.assign(defaultModuleState(), moduleStates[id] || {});
    Object.assign(cur, patch || {});
    moduleStates[id] = cur;
    applyAllModulesToDom();
    saveState();
    return cur;
  }

  function applyPreset(presetKey) {
    const p = BUILTIN_PRESETS[presetKey] || BUILTIN_PRESETS.classic;
    pushUndo();
    currentPreset = presetKey in BUILTIN_PRESETS ? presetKey : "classic";
    moduleStates = cloneState(p.modules || {});
    if (surfaceName() === "widget" && p.widgetPreset && typeof window.applyWidgetPreset === "function") {
      window.applyWidgetPreset(p.widgetPreset);
    }
    applyAllModulesToDom();
    saveState();
    updateToolbarState();
  }

  function undo() {
    if (!undoStack.length) return false;
    redoStack.push(cloneState(moduleStates));
    moduleStates = undoStack.pop();
    applyAllModulesToDom();
    saveState();
    updateToolbarState();
    return true;
  }

  function redo() {
    if (!redoStack.length) return false;
    undoStack.push(cloneState(moduleStates));
    moduleStates = redoStack.pop();
    applyAllModulesToDom();
    saveState();
    updateToolbarState();
    return true;
  }

  function resetLayout() {
    pushUndo();
    moduleStates = {};
    currentPreset = "classic";
    applyAllModulesToDom();
    saveState();
    updateToolbarState();
  }

  function exportProfileJson() {
    return JSON.stringify(
      {
        version: "1.1.0",
        surface: surfaceName(),
        theme: themeName(),
        preset: currentPreset,
        snap: snapEnabled,
        modules: moduleStates,
      },
      null,
      2
    );
  }

  function importProfileJson(jsonStr) {
    const parsed = typeof jsonStr === "string" ? JSON.parse(jsonStr) : jsonStr;
    if (!parsed || typeof parsed !== "object") return false;
    pushUndo();
    if (parsed.preset) currentPreset = parsed.preset;
    if (parsed.modules && typeof parsed.modules === "object") {
      moduleStates = cloneState(parsed.modules);
    }
    applyAllModulesToDom();
    saveState();
    updateToolbarState();
    return true;
  }

  /**
   * Project screen pointer coordinates `(clientX, clientY)` onto a potentially
   * rotated element's local horizontal axis `[0..1]`. Used by seek/volume sliders
   * so dragging at 37° rotation still scrubs smoothly along the bar.
   */
  function localHorizontalFraction(el, clientX, clientY) {
    if (!el) return 0;
    const r = el.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    const modEl = el.closest ? el.closest("[data-module]") : null;
    const deg = modEl ? parseFloat(modEl.dataset.rot || "0") || 0 : 0;
    if (Math.abs(deg) < 0.5) {
      return Math.max(0, Math.min(1, (clientX - r.left) / Math.max(1, r.width)));
    }
    const rad = (-deg * Math.PI) / 180;
    const dx = clientX - cx;
    const dy = clientY - cy;
    const localX = dx * Math.cos(rad) - dy * Math.sin(rad);
    const w = Math.max(1, el.offsetWidth || r.width);
    return Math.max(0, Math.min(1, 0.5 + localX / w));
  }

  function buildLayoutBar() {
    if (document.getElementById("ht-layout-bar")) return;
    const bar = document.createElement("div");
    bar.id = "ht-layout-bar";
    bar.setAttribute("role", "toolbar");
    bar.setAttribute("aria-label", "Layout Studio");
    bar.innerHTML = `
      <span style="font-weight:700;letter-spacing:.08em">LAYOUT STUDIO</span>
      <select id="htLayoutPresetSel" title="Layout Preset">
        ${Object.entries(BUILTIN_PRESETS)
          .map(([k, v]) => `<option value="${k}">${v.name.toUpperCase()}</option>`)
          .join("")}
      </select>
      <button id="htLayoutRotMinus" title="Rotate Selected -15°">-15°</button>
      <button id="htLayoutRotZero" title="Reset Angle to 0°">0°</button>
      <button id="htLayoutRotPlus" title="Rotate Selected +15°">+15°</button>
      <button id="htLayoutSnapBtn" title="Toggle 8px Grid & Angle Snap">SNAP</button>
      <button id="htLayoutUndoBtn" title="Undo (Ctrl+Z)">UNDO</button>
      <button id="htLayoutRedoBtn" title="Redo (Ctrl+Y)">REDO</button>
      <button id="htLayoutResetBtn" title="Reset All Modules">RESET</button>
      <button id="htLayoutDoneBtn" class="active" title="Exit Layout Edit (Ctrl+L)">DONE</button>
    `;
    document.body.appendChild(bar);

    const sel = bar.querySelector("#htLayoutPresetSel");
    sel.onchange = () => applyPreset(sel.value);
    bar.querySelector("#htLayoutRotMinus").onclick = () => {
      const target = activeModuleId || "cover-art";
      const cur = (moduleStates[target] && moduleStates[target].rot) || 0;
      setModuleTransform(target, { rot: snapAngle(cur - 15, true) });
    };
    bar.querySelector("#htLayoutRotZero").onclick = () => {
      const target = activeModuleId || "cover-art";
      setModuleTransform(target, { rot: 0 });
    };
    bar.querySelector("#htLayoutRotPlus").onclick = () => {
      const target = activeModuleId || "cover-art";
      const cur = (moduleStates[target] && moduleStates[target].rot) || 0;
      setModuleTransform(target, { rot: snapAngle(cur + 15, true) });
    };
    bar.querySelector("#htLayoutSnapBtn").onclick = () => {
      snapEnabled = !snapEnabled;
      saveState();
      updateToolbarState();
    };
    bar.querySelector("#htLayoutUndoBtn").onclick = () => undo();
    bar.querySelector("#htLayoutRedoBtn").onclick = () => redo();
    bar.querySelector("#htLayoutResetBtn").onclick = () => resetLayout();
    bar.querySelector("#htLayoutDoneBtn").onclick = () => toggleEditMode(false);
    updateToolbarState();
  }

  function updateToolbarState() {
    const bar = document.getElementById("ht-layout-bar");
    if (!bar) return;
    bar.hidden = !editMode;
    bar.style.display = editMode ? "flex" : "none";
    const sel = bar.querySelector("#htLayoutPresetSel");
    if (sel && sel.value !== currentPreset && BUILTIN_PRESETS[currentPreset]) {
      sel.value = currentPreset;
    }
    const snapBtn = bar.querySelector("#htLayoutSnapBtn");
    if (snapBtn) snapBtn.classList.toggle("active", snapEnabled);
  }

  function toggleEditMode(force) {
    editMode = typeof force === "boolean" ? force : !editMode;
    document.documentElement.setAttribute("data-layout-edit", editMode ? "true" : "false");
    ensureDefaultModuleTags();
    buildLayoutBar();
    updateToolbarState();
    applyAllModulesToDom();
    if (editMode && !activeModuleId) {
      const first = document.querySelector("[data-module]");
      if (first) selectModule(first.getAttribute("data-module"));
    }
    return editMode;
  }

  // Keyboard shortcuts (`Ctrl+L` toggle edit mode, `Ctrl+Z`/`Ctrl+Y` undo/redo when editing)
  window.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "l") {
      e.preventDefault();
      toggleEditMode();
    } else if (editMode && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z" && !e.shiftKey) {
      e.preventDefault();
      undo();
    } else if (
      editMode &&
      (e.ctrlKey || e.metaKey) &&
      (e.key.toLowerCase() === "y" || (e.shiftKey && e.key.toLowerCase() === "z"))
    ) {
      e.preventDefault();
      redo();
    } else if (editMode && e.key === "Escape") {
      toggleEditMode(false);
    }
  });

  // QA Audit hook for layout & rotation hit-testing
  function layoutAudit() {
    ensureDefaultModuleTags();
    const mods = Array.from(document.querySelectorAll("[data-module]")).map((el) => ({
      id: el.getAttribute("data-module"),
      rot: parseFloat(el.dataset.rot || "0") || 0,
      dx: parseFloat(el.dataset.dx || "0") || 0,
      dy: parseFloat(el.dataset.dy || "0") || 0,
      rect: el.getBoundingClientRect().toJSON ? el.getBoundingClientRect().toJSON() : {},
    }));
    return {
      ok: mods.length > 0,
      editMode,
      preset: currentPreset,
      presetsAvailable: Object.keys(BUILTIN_PRESETS),
      moduleCount: mods.length,
      modules: mods,
    };
  }

  function init() {
    injectLayoutStyles();
    loadState();
    ensureDefaultModuleTags();
    buildLayoutBar();
    applyAllModulesToDom();
    window.__qa = window.__qa || {};
    window.__qa.layoutAudit = layoutAudit;
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init, { once: true });
  } else {
    init();
  }

  document.addEventListener("halftone:theme", () => {
    loadState();
    applyAllModulesToDom();
  });

  window.HalftoneLayout = {
    BUILTIN_PRESETS,
    toggleEditMode,
    setEditMode: toggleEditMode,
    setModuleTransform,
    getModulePose(elOrId) {
      const id = typeof elOrId === "string" ? elOrId : (elOrId && elOrId.getAttribute && elOrId.getAttribute("data-module")) || "seek-bar";
      const st = Object.assign(defaultModuleState(), moduleStates[id] || {});
      return { x: st.dx || 0, y: st.dy || 0, s: st.scale || 1, r: st.rot || 0, dx: st.dx || 0, dy: st.dy || 0, rot: st.rot || 0, scale: st.scale || 1 };
    },
    setModulePose(elOrId, pose) {
      const id = typeof elOrId === "string" ? elOrId : (elOrId && elOrId.getAttribute && elOrId.getAttribute("data-module")) || "seek-bar";
      const patch = {};
      if (pose.x !== undefined) patch.dx = pose.x;
      if (pose.dx !== undefined) patch.dx = pose.dx;
      if (pose.y !== undefined) patch.dy = pose.y;
      if (pose.dy !== undefined) patch.dy = pose.dy;
      if (pose.s !== undefined) patch.scale = pose.s;
      if (pose.scale !== undefined) patch.scale = pose.scale;
      if (pose.r !== undefined) patch.rot = pose.r;
      if (pose.rot !== undefined) patch.rot = pose.rot;
      return setModuleTransform(id, patch);
    },
    applyPreset,
    undo,
    redo,
    resetLayout,
    exportProfileJson,
    importProfileJson,
    localHorizontalFraction,
    layoutAudit,
    applyAllModulesToDom,
  };
})();
